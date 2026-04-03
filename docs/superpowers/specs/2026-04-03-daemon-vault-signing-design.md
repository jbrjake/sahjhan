# Daemon, Vault, and Signing Service — Design Spec

**Issue:** #23
**Date:** 2026-04-03

## Problem

Sahjhan stores secrets as plaintext files on disk (`session.key`, plus downstream consumer data like quiz banks). The PreToolUse hook system blocks reads via string-matching on tool call arguments, but an LLM agent with arbitrary code execution can bypass this trivially — write a Python script that constructs the path as a variable, run it via Bash, and the hook never sees the protected path. The string-matching approach has infinite bypasses.

The only place a secret can live that is inaccessible to a same-user adversary with code execution is in the memory of a protected process.

## Architecture Decision

**CLI-mediated daemon.** Sahjhan gains a foreground daemon mode that holds secrets exclusively in process memory. Hooks continue to call sahjhan CLI subcommands (`sahjhan sign`, `sahjhan vault read`), which connect to the daemon over a Unix domain socket internally. The daemon authenticates callers via kernel-enforced socket peer credentials and a static manifest of trusted hook scripts.

Key decisions from brainstorming:

- **CLI-mediated connections only** — hooks call `sahjhan sign`/`sahjhan vault *`, not the socket directly. Minimizes hook generation changes; direct socket connections are a future optimization.
- **Foreground-only** — no daemonization logic in Rust. The caller (hook harness, CI, user) backgrounds the process. Avoids the fork+tokio footgun.
- **Best-effort `mlock`** — try to lock secret pages; warn and continue on failure. Primary defense is in-memory-only storage + anti-debug + peer auth, not swap protection.
- **`SOCK_STREAM` + newline-delimited JSON** — single protocol, both platforms. `SOCK_SEQPACKET` is Linux-only; not worth the conditional logic.

## New Source Modules

```
src/
├── daemon/
│   ├── mod.rs          # DaemonServer struct, socket accept loop, signal handling
│   ├── auth.rs         # Peer credential extraction, PID-to-script resolution, manifest checking
│   ├── protocol.rs     # Request/response types, JSON parsing, dispatch to handlers
│   ├── vault.rs        # In-memory key-value store with zeroize-on-drop
│   └── platform.rs     # #[cfg] layer: peer creds, proc info, anti-debug, mlock
├── cli/
│   ├── daemon_cmd.rs   # `daemon start` / `daemon stop` subcommands
│   ├── sign_cmd.rs     # `sign` subcommand (connects to daemon, returns proof)
│   └── vault_cmd.rs    # `vault store/read/delete/list` subcommands
```

### `daemon/platform.rs` — Platform Abstraction

All `#[cfg(target_os)]` code lives here behind a clean API:

| Function | macOS | Linux |
|----------|-------|-------|
| `get_peer_pid(socket)` | `LOCAL_PEERCRED` | `SO_PEERCRED` |
| `get_exe_path(pid)` | `proc_pidpath()` | `/proc/pid/exe` |
| `get_cmdline(pid)` | `sysctl kern.procargs2` | `/proc/pid/cmdline` |
| `get_parent_pid(pid)` | `proc_pidinfo` | `/proc/pid/status` |
| `deny_debug_attach()` | `ptrace(PT_DENY_ATTACH)` | `prctl(PR_SET_DUMPABLE, 0)` |
| `try_mlock(ptr, len)` | `mlock()` best-effort | `mlock()` best-effort |

No other module touches platform-specific APIs directly.

## Daemon Lifecycle

### `DaemonServer`

```rust
struct DaemonServer {
    socket_path: PathBuf,          // {data_dir}/sahjhan.sock
    pid_path: PathBuf,             // {data_dir}/sahjhan.pid
    session_key: Zeroizing<[u8; 32]>,
    vault: Vault,
    config_dir: PathBuf,           // protocol TOML + trusted-callers.toml
    data_dir: PathBuf,             // runtime state (ledger, socket, pid)
    trusted_callers: TrustedCallersManifest,
}
```

### Startup (`daemon start`)

1. Check for `LD_PRELOAD` / `DYLD_INSERT_LIBRARIES` in env — refuse to start if set
2. Clean stale `sahjhan.sock` / `sahjhan.pid` if present (verify PID is dead; if alive, exit with error "daemon already running")
3. Generate 32-byte session key via `getrandom`, wrap in `Zeroizing<>`, best-effort `mlock`
4. Call `deny_debug_attach()`
5. Load `trusted-callers.toml` from config dir
6. Bind Unix socket at `{data_dir}/sahjhan.sock`, chmod `0600`
7. Write PID to `{data_dir}/sahjhan.pid`
8. Enter tokio accept loop — one spawned task per connection

`--foreground` flag exists for forward compatibility but is a no-op in v1 (always foreground). The caller is responsible for backgrounding.

### Shutdown (`daemon stop` or SIGTERM)

1. `Zeroizing` zeros session key on drop; `Vault` zeros all entries
2. Remove socket file
3. Remove PID file
4. Exit 0

`daemon stop` from CLI: reads PID file, sends SIGTERM, waits up to 5s, then SIGKILL if still alive.

Signal handling via `tokio::signal` for SIGTERM/SIGINT — triggers graceful shutdown.

### Unclean Death

OS reclaims memory. Secrets gone. Stale socket/PID cleaned on next `daemon start`.

## Caller Authentication

Every new socket connection goes through authentication once. Since we use CLI-mediated connections, the connecting process is always a `sahjhan` CLI binary, and the actual caller is its parent.

### Auth Flow

1. **Get peer PID** — `get_peer_pid(socket)` via platform layer. Kernel-provided, unspoofable.
2. **Self-detection** — `get_exe_path(peer_pid)` compared to `std::env::current_exe()`. If they match, this is CLI-mediated — walk to parent.
3. **Get parent PID** — `get_parent_pid(peer_pid)`. This is the hook script's interpreter.
4. **Resolve script path** — `get_exe_path(parent_pid)` gives the interpreter. `get_cmdline(parent_pid)` gives the argument list. First non-flag argument is the script path. Canonicalize it.
5. **Manifest lookup** — Relativize script path to config dir's parent (plugin root). Look up in `trusted-callers.toml`. Not found = reject.
6. **Hash verification** — SHA-256 the script file at the resolved path. Compare to manifest. Mismatch = reject.
7. **Accept** — Connection authenticated for its lifetime. No re-auth per request.

### Edge Cases

- **Parent is `bash`/`zsh`** (agent calling directly): no script path in cmdline. Not in manifest. Rejected.
- **Parent already exited**: `get_parent_pid` or `get_exe_path` fails. Auth fails. Connection rejected.
- **More than one layer of shell wrapping**: not supported in v1. Document the limitation.

## Trusted-Callers Manifest

```toml
# enforcement/trusted-callers.toml
# Paths relative to config dir's parent (plugin root).
# Hashes are SHA-256 of file contents at install time.

[callers]
"enforcement/hooks/_common.py" = "sha256:a1b2c3d4e5f6..."
"enforcement/hooks/lens_quiz.py" = "sha256:7890abcdef01..."
"enforcement/hooks/stop_hook.py" = "sha256:2345678901ab..."
```

Static file, staged at plugin install time. Lives under `enforcement/`, which is already write-protected by the bootstrap hook. No runtime mutation API.

## Vault

```rust
struct Vault {
    entries: HashMap<String, Zeroizing<Vec<u8>>>,
}
```

- `store(name, data)` — Insert or overwrite. Data wrapped in `Zeroizing`.
- `read(name) -> Option<&[u8]>` — Return reference.
- `delete(name)` — `Zeroizing` zeros on drop.
- `list() -> Vec<&str>` — Names only.

No persistence. No encryption at rest. Vault exists purely in daemon memory.

## Signing

Reuses existing HMAC-SHA256 logic. Daemon receives `{event_type, fields}`, builds canonical payload (`event_type\0field1=value1\0field2=value2`, fields sorted lexicographically), computes HMAC with in-memory session key, returns hex proof.

## Wire Protocol

JSON-over-Unix-socket. `SOCK_STREAM`, newline-delimited. Each request is one JSON line, each response is one JSON line.

### Requests

```json
{"op": "sign", "event_type": "quiz_answered", "fields": {"perspective": "security", "answer": "B"}}
{"op": "vault_store", "name": "quiz-bank", "data": "<base64>"}
{"op": "vault_read", "name": "quiz-bank"}
{"op": "vault_delete", "name": "quiz-bank"}
{"op": "vault_list"}
{"op": "status"}
```

### Responses

```json
{"ok": true, "proof": "a1b2c3d4..."}
{"ok": true}
{"ok": true, "data": "<base64>"}
{"ok": true, "names": ["quiz-bank"]}
{"ok": true, "pid": 12345, "uptime_seconds": 3600, "vault_entries": 2}
{"ok": false, "error": "auth_failed", "message": "..."}
```

Error codes: `auth_failed`, `not_found`, `invalid_request`.

## CLI Subcommands

### New Commands

```
sahjhan daemon start [--foreground]
sahjhan daemon stop
sahjhan daemon status
sahjhan sign --event-type <type> --field k=v [--field k=v ...]
sahjhan vault store --name <name> --file <path>
sahjhan vault read --name <name>
sahjhan vault delete --name <name>
sahjhan vault list
```

All privileged commands (`sign`, `vault *`) connect to `{data_dir}/sahjhan.sock`. If socket missing or connection fails: `"Error: sahjhan daemon is not running. Start it with 'sahjhan daemon start'."`.

### Existing Commands

Unchanged. `authed-event`, `event`, `transition`, `status`, `hook eval` etc. continue reading ledger files directly. No daemon required for non-privileged operations.

### Hook Generation

Minimal changes. Generated hooks still delegate to `sahjhan hook eval` via subprocess for enforcement. Downstream consumer hooks that need to record restricted events call `sahjhan sign` to get a proof, then `sahjhan authed-event --proof <proof>`. Hook generation gains awareness of daemon mode in comments/documentation but no structural change to the generated Python.

## New Dependencies

| Crate | Purpose |
|-------|---------|
| `zeroize` | Secure memory zeroing for secrets on drop |
| `libc` | `mlock`, `prctl`, `ptrace`, socket credential structs |
| `base64` | Vault data encoding on the wire |
| `nix` (optional) | Higher-level Unix API for socket options, if `libc` is too raw |

`tokio`, `serde`, `serde_json`, `sha2`, `hmac`, `hex`, `getrandom` are already dependencies.

## Adversarial Resistance

| Attack | Defense |
|--------|---------|
| Agent calls `sahjhan sign` via Bash | Parent PID is `bash`, not a trusted hook. Rejected. |
| Agent writes a script that calls sahjhan | Script path not in manifest. Rejected. |
| Agent writes into `enforcement/hooks/` | Bootstrap hook blocks writes to `enforcement/`. Never created. |
| Agent modifies a trusted hook | Write-blocked. Even if bypassed: SHA-256 mismatch. Rejected. |
| Agent copies a hook to another path | New path not in manifest. Rejected. |
| Agent spoofs `argv[0]` | Daemon reads actual exe path, not `argv[0]`. |
| Agent reads `/proc/pid/mem` | `PR_SET_DUMPABLE=0` / `PT_DENY_ATTACH`. Permission denied. |
| Agent uses `LD_PRELOAD` | Daemon refuses to start if set. |
| Agent kills daemon | Secrets vanish. Denial of service, not exfiltration. |
| Agent reads vault files on disk | No vault files on disk. Pure in-memory. |

## Testing Strategy

### Unit Tests

- `daemon/vault.rs` — store/read/delete/list, overwrite, zeroize on drop
- `daemon/protocol.rs` — JSON parsing all request/response types, malformed input, unknown ops
- `daemon/auth.rs` — manifest loading, path relativization, hash comparison (mock platform layer)
- `daemon/platform.rs` — compile-only per platform

### Integration Tests

- `tests/daemon_tests.rs` — Lifecycle: start, socket exists, PID file, stop, cleanup. Stale cleanup. LD_PRELOAD rejection. SIGTERM handling.
- `tests/signing_tests.rs` — Start daemon, `sahjhan sign`, verify proof accepted by `authed-event`. Proof matches manual HMAC. Error when daemon not running.
- `tests/vault_tests.rs` — Store/read/delete/list end-to-end. Overwrite. Not-found errors.
- `tests/caller_auth_tests.rs` — Trusted script accepted. Not-in-manifest rejected. Hash mismatch rejected. Untrusted caller rejected.

### Not Tested in CI

`mlock` behavior and `ptrace`/`prctl` — platform-dependent, may fail in containers. Get `#[ignore]` tests for manual verification.

### Pattern

Same as existing: `tempfile::TempDir` for isolation, `assert_cmd` for CLI, daemon started in foreground via `std::process::Command` in a background thread, commands run against it, then stopped.
