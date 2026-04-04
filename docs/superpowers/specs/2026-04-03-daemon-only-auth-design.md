# Daemon-Only Auth — Design Spec

**Issue:** #23 (follow-up)
**Date:** 2026-04-03

## Problem

The initial daemon implementation has two gaps:

1. **Auth not wired.** The daemon accepts all local socket connections. The auth module (TrustedCallersManifest, PID resolution, hash verification) is built and tested but not called from `handle_connection`. Any process on the machine can sign anything and read the vault.

2. **Legacy disk keys still exist.** `sahjhan init` writes `session.key` to disk. `authed-event` reads it from disk to verify proofs. The daemon generates a separate key in memory. These are different keys — the `sign` → `authed-event` flow is broken. The disk key is the exact thing the daemon was built to eliminate.

## Changes

### 1. Wire caller auth into `handle_connection`

On every new connection, before processing any request:

1. `get_peer_pid(socket)` — kernel-provided
2. `get_exe_path(peer_pid)` vs `std::env::current_exe()` — if match, CLI-mediated, walk to parent
3. `get_parent_pid(peer_pid)` → `get_cmdline(parent_pid)` → `extract_script_path()` → canonicalize
4. Relativize to config dir's parent → `manifest.verify_caller()`
5. Reject if any step fails

**Exception:** The `status` op is exempt from auth. This allows health checks from any process with socket access (0600). All other ops (`sign`, `verify`, `vault_*`) require authentication.

To implement this, `handle_connection` needs access to the `TrustedCallersManifest` and `config_dir` (for relativization). These are already on `DaemonServer` — pass them to `handle_connection`.

### 2. Add `verify` op to daemon protocol

New request:
```json
{"op": "verify", "event_type": "quiz_answered", "fields": {"score": "5"}, "proof": "a1b2c3..."}
```

Response:
```json
{"ok": true}
```
or:
```json
{"ok": false, "error": "invalid_proof", "message": "proof does not match"}
```

The daemon recomputes the HMAC from its in-memory key and compares. The key never leaves the daemon.

New CLI command:
```
sahjhan verify --event-type <type> --field k=v [--field k=v ...] --proof <hex>
```
Connects to daemon, sends verify request, exits 0 if valid, non-zero if invalid.

### 3. Rewrite `authed-event` to use daemon

`cmd_authed_event` changes:
- Remove: reading `session.key` from disk, local HMAC computation
- Add: connect to daemon socket, send `verify` request with event_type, fields, proof
- If daemon says ok: record the event (existing logic unchanged)
- If daemon not running: fail with "sahjhan daemon is not running"
- No fallback to disk key

`cmd_reseal` same treatment:
- Send `{"op": "verify", "event_type": "config_reseal", "fields": {}, "proof": "..."}` to daemon
- If valid: proceed with reseal (existing logic unchanged)
- If daemon not running: fail

### 4. Remove all disk-based key code

**`src/cli/init.rs`:** Remove the session.key generation block (lines 134-143). Init no longer creates a key file.

**`src/cli/ledger.rs`:** Remove per-ledger session.key generation from `cmd_ledger_create` (lines 180-194).

**`src/cli/authed_event.rs`:** Remove `resolve_session_key_path` function. Remove local HMAC verification from `cmd_authed_event` and `cmd_reseal` (replaced by daemon verify).

**`src/cli/config_cmd.rs`:** Remove `cmd_session_key_path` command. Replace with `cmd_daemon_socket_path` that prints the daemon socket path (or remove entirely — `sahjhan daemon status` covers this).

**`src/main.rs`:** Remove `Config { SessionKeyPath }` subcommand. Optionally replace with socket path variant.

**`src/cli/guards.rs`:** Remove auto-inclusion of `session.key` in read_blocked. No key file to protect.

### 5. Update tests

**`tests/auth_tests.rs`:** Rewrite all tests that read `session.key` from disk or compute HMAC proofs locally. Tests now start a daemon, use `sahjhan sign` to get proofs, and `sahjhan authed-event` to verify+record. Tests that checked for `session.key` file existence become tests that check daemon is required.

**`tests/config_integrity_tests.rs`:** Rewrite `test_cli_reseal_with_valid_proof_succeeds` to use daemon signing.

**`tests/daemon_signing_tests.rs`:** Add test for the full `sign` → `authed-event` flow (now that both use the daemon's key, this works).

### 6. Consolidate `build_canonical_payload`

Currently duplicated in `cli/authed_event.rs` and `daemon/mod.rs`. Remove the copy in `authed_event.rs`. The canonical source is `daemon::build_canonical_payload` (already `pub`). Any code that needs it imports from `daemon`.

## What Stays the Same

- Ledger format, hash chain, config seals — unchanged
- Gate evaluation, hook evaluation, state machine — unchanged
- The `event` command (unrestricted events) — unchanged, no key needed
- Wire protocol for `sign`, `vault_*`, `status` — unchanged
- Daemon startup, shutdown, signal handling — unchanged
- Vault operations — unchanged
