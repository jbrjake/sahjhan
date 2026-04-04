# Daemon-Only Auth Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire caller authentication into the daemon, add a `verify` op, rewrite `authed-event`/`reseal` to use the daemon, and remove all disk-based session key code.

**Architecture:** Five tasks: (1) add `verify` op to protocol + daemon, (2) wire caller auth into `handle_connection`, (3) rewrite `authed-event`/`reseal` to use daemon, (4) remove disk key code from init/ledger/guards/config, (5) update all tests. Each task is independently testable.

**Tech Stack:** Existing Rust crates — no new dependencies.

---

## File Structure

```
Modified files:
  src/daemon/protocol.rs      # Add Verify variant to Request
  src/daemon/mod.rs            # Wire auth into handle_connection, add Verify handler
  src/cli/authed_event.rs      # Rewrite to verify via daemon, remove disk key code
  src/cli/init.rs              # Remove session.key generation
  src/cli/ledger.rs            # Remove per-ledger session.key generation
  src/cli/guards.rs            # Remove session.key auto-inclusion
  src/cli/config_cmd.rs        # Remove cmd_session_key_path
  src/cli/verify_cmd.rs        # New: verify CLI command
  src/cli/mod.rs               # Add verify_cmd, remove config_cmd if empty
  src/main.rs                  # Add Verify command, remove Config SessionKeyPath

Modified test files:
  tests/daemon_protocol_tests.rs   # Add verify request/response tests
  tests/daemon_signing_tests.rs    # Add sign→authed-event e2e test
  tests/auth_tests.rs              # Rewrite to use daemon instead of disk keys
  tests/config_integrity_tests.rs  # Rewrite reseal test to use daemon
```

---

### Task 1: Add `verify` Op to Protocol and Daemon

**Files:**
- Modify: `src/daemon/protocol.rs`
- Modify: `src/daemon/mod.rs`
- Modify: `tests/daemon_protocol_tests.rs`

- [ ] **Step 1: Add verify tests to protocol tests**

Add to `tests/daemon_protocol_tests.rs`:

```rust
#[test]
fn test_parse_verify_request() {
    let json = r#"{"op": "verify", "event_type": "quiz_answered", "fields": {"score": "5"}, "proof": "abcdef"}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    match req {
        Request::Verify {
            event_type,
            fields,
            proof,
        } => {
            assert_eq!(event_type, "quiz_answered");
            assert_eq!(fields.get("score").unwrap(), "5");
            assert_eq!(proof, "abcdef");
        }
        _ => panic!("Expected Verify request"),
    }
}

#[test]
fn test_serialize_ok_verified_response() {
    let resp = Response::ok_verified();
    let json = serde_json::to_string(&resp).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["verified"], true);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test daemon_protocol_tests`
Expected: compilation error — `Verify` variant doesn't exist

- [ ] **Step 3: Add Verify variant to Request enum**

In `src/daemon/protocol.rs`, add to the `Request` enum after `Status`:

```rust
    #[serde(rename = "verify")]
    Verify {
        event_type: String,
        fields: HashMap<String, String>,
        proof: String,
    },
```

- [ ] **Step 4: Add `ok_verified` constructor to Response**

In `src/daemon/protocol.rs`, add a `verified` field to the `Response` struct:

```rust
    #[serde(skip_serializing_if = "Option::is_none")]
    pub verified: Option<bool>,
```

Add to every existing constructor `verified: None,` in each field initializer.

Add new constructor:

```rust
    pub fn ok_verified() -> Self {
        Self {
            ok: true,
            proof: None,
            data: None,
            names: None,
            pid: None,
            uptime_seconds: None,
            vault_entries: None,
            verified: Some(true),
            error: None,
            message: None,
        }
    }
```

- [ ] **Step 5: Add Verify handler to daemon**

In `src/daemon/mod.rs`, add to the `handle_request` match after `Request::Status`:

```rust
        Request::Verify {
            event_type,
            fields,
            proof,
        } => {
            let expected = compute_sign(session_key, &event_type, &fields);
            if proof == expected {
                Response::ok_verified()
            } else {
                Response::err("invalid_proof", "proof does not match")
            }
        }
```

- [ ] **Step 6: Run tests**

Run: `cargo test daemon_protocol_tests`
Expected: all 16 tests pass (14 existing + 2 new)

- [ ] **Step 7: Commit**

```bash
git add src/daemon/protocol.rs src/daemon/mod.rs tests/daemon_protocol_tests.rs
git commit -m "feat: add verify op to daemon protocol"
```

---

### Task 2: Wire Caller Auth into `handle_connection`

**Files:**
- Modify: `src/daemon/mod.rs`
- Modify: `src/daemon/auth.rs`

- [ ] **Step 1: Add `authenticate_peer` function to `daemon/auth.rs`**

Add to the end of `src/daemon/auth.rs`:

```rust
use crate::daemon::platform;
use std::os::unix::net::UnixStream;

/// Authenticate a connected peer.
///
/// For CLI-mediated connections (peer exe matches our own binary):
///   peer PID → parent PID → cmdline → script path → manifest lookup → hash check
///
/// Returns Ok(()) if authenticated, Err(AuthError) if rejected.
pub fn authenticate_peer(
    stream: &UnixStream,
    manifest: &TrustedCallersManifest,
    plugin_root: &Path,
) -> Result<(), AuthError> {
    let peer_pid = platform::get_peer_pid(stream)
        .map_err(|e| AuthError::Platform(format!("cannot get peer PID: {}", e)))?;

    // Determine if this is a CLI-mediated connection (peer is our own binary).
    let peer_exe = platform::get_exe_path(peer_pid)
        .map_err(|e| AuthError::Platform(format!("cannot get peer exe: {}", e)))?;
    let our_exe = std::env::current_exe()
        .map_err(|e| AuthError::Platform(format!("cannot get own exe: {}", e)))?;

    let target_pid = if peer_exe == our_exe {
        // CLI-mediated: walk to parent (the hook script's interpreter).
        platform::get_parent_pid(peer_pid)
            .map_err(|e| AuthError::Platform(format!("cannot get parent PID: {}", e)))?
    } else {
        peer_pid
    };

    // Get the target's command line and extract the script path.
    let cmdline = platform::get_cmdline(target_pid)
        .map_err(|e| AuthError::Platform(format!("cannot get cmdline for PID {}: {}", target_pid, e)))?;

    let script_path_str = extract_script_path(&cmdline)
        .ok_or(AuthError::NoScriptPath)?;

    // Canonicalize and relativize.
    let script_path = std::path::Path::new(&script_path_str);
    let canonical = script_path.canonicalize()
        .map_err(|e| AuthError::Platform(format!("cannot canonicalize {}: {}", script_path_str, e)))?;
    let plugin_root_canonical = plugin_root.canonicalize()
        .map_err(|e| AuthError::Platform(format!("cannot canonicalize plugin root: {}", e)))?;

    let relative = canonical
        .strip_prefix(&plugin_root_canonical)
        .map_err(|_| AuthError::NotInManifest {
            path: canonical.display().to_string(),
        })?;

    let relative_str = relative.to_string_lossy();
    manifest.verify_caller(&plugin_root_canonical, &relative_str)
}
```

- [ ] **Step 2: Update `handle_connection` in `daemon/mod.rs`**

Change the `handle_connection` signature to accept auth context:

```rust
fn handle_connection(
    stream: UnixStream,
    vault: Arc<Mutex<Vault>>,
    session_key: Zeroizing<Vec<u8>>,
    start_time: Instant,
    trusted_callers: &TrustedCallersManifest,
    plugin_root: &Path,
) {
```

At the top of the function, before the read loop, add auth:

```rust
    // Authenticate the caller. Status requests are exempt (checked per-request below),
    // but we do auth once at connection open for all other ops.
    let authenticated = match auth::authenticate_peer(&stream, trusted_callers, plugin_root) {
        Ok(()) => true,
        Err(e) => {
            eprintln!("auth: {}", e);
            false
        }
    };
```

Then in the request dispatch, gate non-status ops:

```rust
        let response = match serde_json::from_str::<Request>(&line) {
            Ok(Request::Status) => {
                // Status is always allowed (health check).
                handle_request(Request::Status, &vault, &session_key, start_time)
            }
            Ok(req) => {
                if !authenticated {
                    Response::err("auth_failed", "caller not authenticated")
                } else {
                    handle_request(req, &vault, &session_key, start_time)
                }
            }
            Err(e) => Response::err("parse_error", &format!("invalid request: {}", e)),
        };
```

- [ ] **Step 3: Update the accept loop call site in `start()`**

In `DaemonServer::start()`, update the call to `handle_connection`:

```rust
                    let trusted_callers = &self.trusted_callers;
                    let plugin_root = self.config_dir.parent().unwrap_or(&self.config_dir);
                    handle_connection(
                        stream,
                        vault,
                        key,
                        start_time,
                        trusted_callers,
                        plugin_root,
                    );
```

- [ ] **Step 4: Verify it compiles and existing tests pass**

Run: `cargo check && cargo test daemon_`
Expected: compiles, all existing daemon tests pass (status tests still work because status is exempt from auth; other e2e tests pass because when the caller is the test binary, auth may fail but the `#[ignore]` tests spawn the CLI which is the sahjhan binary itself — walk to parent finds the test runner, which won't be in the manifest. This means the e2e tests need the manifest to include the test runner, OR we need a way to bypass auth in test mode.)

**IMPORTANT:** The existing `#[ignore]` e2e tests will now fail because the test binary → daemon connection won't pass auth. This is expected and will be fixed in Task 5 when we rewrite the tests.

Run: `cargo test daemon_platform && cargo test daemon_vault_tests && cargo test daemon_protocol && cargo test daemon_auth`
Expected: all non-e2e daemon tests pass

- [ ] **Step 5: Commit**

```bash
git add src/daemon/mod.rs src/daemon/auth.rs
git commit -m "feat: wire caller authentication into daemon handle_connection"
```

---

### Task 3: Add `verify` CLI Command and Rewrite `authed-event` / `reseal`

**Files:**
- Create: `src/cli/verify_cmd.rs`
- Modify: `src/cli/authed_event.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Create `verify_cmd.rs`**

Create `src/cli/verify_cmd.rs`:

```rust
// src/cli/verify_cmd.rs
//
// CLI handler for `sahjhan verify`.
//
// ## Index
// - [cmd-verify]              cmd_verify()  — verify HMAC proof via daemon

use crate::cli::commands;
use crate::cli::daemon_cmd;
use std::collections::HashMap;

// [cmd-verify]
pub fn cmd_verify(config_dir: &str, event_type: &str, fields: &[String], proof: &str) -> i32 {
    let socket_path = match daemon_cmd::resolve_socket_path(config_dir) {
        Ok(p) => p,
        Err((code, msg)) => {
            eprintln!("error: {}", msg);
            return code;
        }
    };

    let mut field_map = HashMap::new();
    for f in fields {
        if let Some((k, v)) = f.split_once('=') {
            field_map.insert(k.to_string(), v.to_string());
        } else {
            eprintln!("error: invalid field format '{}', expected key=value", f);
            return commands::EXIT_USAGE_ERROR;
        }
    }

    let request = serde_json::json!({
        "op": "verify",
        "event_type": event_type,
        "fields": field_map,
        "proof": proof,
    });

    match daemon_cmd::connect_and_request(&socket_path, &request.to_string()) {
        Ok(response) => {
            let v: serde_json::Value = match serde_json::from_str(&response) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("error: invalid response from daemon: {}", e);
                    return commands::EXIT_CONFIG_ERROR;
                }
            };
            if v["ok"] == true {
                commands::EXIT_SUCCESS
            } else {
                eprintln!(
                    "error: {}",
                    v["message"].as_str().unwrap_or("invalid proof")
                );
                commands::EXIT_INTEGRITY_ERROR
            }
        }
        Err(msg) => {
            eprintln!("error: {}", msg);
            commands::EXIT_CONFIG_ERROR
        }
    }
}
```

- [ ] **Step 2: Rewrite `cmd_authed_event` to use daemon**

Replace the entire `cmd_authed_event` function in `src/cli/authed_event.rs`:

```rust
// [cmd-authed-event]
pub fn cmd_authed_event(
    config_dir: &str,
    event_type: &str,
    field_strs: &[String],
    proof: &str,
    targeting: &LedgerTargeting,
) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    // Verify event type IS restricted
    match config.events.get(event_type) {
        Some(event_config) => {
            if event_config.restricted != Some(true) {
                eprintln!(
                    "error: event type '{}' is not restricted. Use 'sahjhan event' instead.",
                    event_type
                );
                return EXIT_USAGE_ERROR;
            }
        }
        None => {}
    }

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting, &config_path) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let mut manifest = match load_manifest(&data_dir) {
        Ok(m) => m,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    // Parse fields
    let mut fields: HashMap<String, String> = HashMap::new();
    for f in field_strs {
        if let Some((key, value)) = f.split_once('=') {
            fields.insert(key.to_string(), value.to_string());
        } else {
            eprintln!("error: invalid field '{}': expected key=value", f);
            return EXIT_USAGE_ERROR;
        }
    }

    // Validate fields against events.toml definitions
    if let Some(event_config) = config.events.get(event_type) {
        if let Err((code, msg)) = validate_event_fields(event_config, &fields, event_type) {
            eprintln!("{}", msg);
            return code;
        }
    }

    // Verify proof via daemon
    let verify_code = super::verify_cmd::cmd_verify(config_dir, event_type, field_strs, proof);
    if verify_code != 0 {
        return verify_code;
    }

    // Proof verified — record the event
    let mut machine = StateMachine::new(&config, ledger);

    record_and_render(
        &config,
        &config_path,
        &mut machine,
        &mut manifest,
        &data_dir,
        event_type,
        fields,
        targeting,
    )
}
```

- [ ] **Step 3: Rewrite `cmd_reseal` to use daemon**

Replace the HMAC verification section (lines 214-242) of `cmd_reseal` in `src/cli/authed_event.rs`:

Replace:
```rust
    // Verify HMAC proof
    let key_path = resolve_session_key_path(&data_dir, targeting);
    let key = match std::fs::read(&key_path) { ... };
    let payload = "config_reseal";
    let mut mac = match HmacSha256::new_from_slice(&key) { ... };
    mac.update(payload.as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());
    if proof != expected { ... }
```

With:
```rust
    // Verify proof via daemon
    let verify_code = super::verify_cmd::cmd_verify(config_dir, "config_reseal", &[], proof);
    if verify_code != 0 {
        return verify_code;
    }
```

- [ ] **Step 4: Clean up imports in `authed_event.rs`**

Remove now-unused imports:
- Remove `use hmac::{Hmac, Mac};`
- Remove `use sha2::Sha256;`
- Remove `type HmacSha256 = Hmac<Sha256>;`
- Remove `use std::path::PathBuf;`
- Remove `resolve_session_key_path` function entirely
- Remove `build_canonical_payload` function entirely (the canonical source is `daemon::build_canonical_payload`)
- Remove `resolve_data_dir` and `resolve_ledger_from_targeting` from the import if no longer used by `cmd_reseal` (check: `cmd_reseal` still uses `resolve_data_dir` for the data_dir variable, keep it; `resolve_ledger_from_targeting` is still used in reseal, keep it)

- [ ] **Step 5: Register verify_cmd and add CLI command**

Add to `src/cli/mod.rs`:

```rust
pub mod verify_cmd;
```

In `src/main.rs`, add import:

```rust
use sahjhan::cli::verify_cmd;
```

Add to `Commands` enum:

```rust
    /// Verify an HMAC-SHA256 proof via daemon
    Verify {
        /// Event type
        #[arg(long = "event-type")]
        event_type: String,

        /// Field values (key=value)
        #[arg(long = "field", value_name = "KEY=VALUE")]
        fields: Vec<String>,

        /// HMAC-SHA256 proof to verify
        #[arg(long)]
        proof: String,
    },
```

Add dispatch arm:

```rust
        Commands::Verify {
            event_type,
            fields,
            proof,
        } => {
            let code = verify_cmd::cmd_verify(&cli.config_dir, &event_type, &fields, &proof);
            Box::new(LegacyResult::new("verify", code))
        }
```

- [ ] **Step 6: Verify it compiles**

Run: `cargo check`
Expected: compiles (tests will fail until Task 4 removes disk key expectations)

- [ ] **Step 7: Commit**

```bash
git add src/cli/verify_cmd.rs src/cli/authed_event.rs src/cli/mod.rs src/main.rs
git commit -m "feat: add verify command, rewrite authed-event and reseal to use daemon"
```

---

### Task 4: Remove Disk-Based Key Code

**Files:**
- Modify: `src/cli/init.rs`
- Modify: `src/cli/ledger.rs`
- Modify: `src/cli/guards.rs`
- Modify: `src/cli/config_cmd.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Remove session.key generation from `cmd_init`**

In `src/cli/init.rs`, remove the session key generation block (the block starting with `// Generate session key` that generates 32 random bytes and writes to `data_dir/session.key`). Delete the entire block including comments.

- [ ] **Step 2: Remove per-ledger session.key generation from `cmd_ledger_create`**

In `src/cli/ledger.rs`, remove the block starting with `// Generate per-ledger session key` that creates `data_dir/ledgers/<name>/session.key`. Delete the entire block including comments.

- [ ] **Step 3: Remove session.key auto-inclusion from `cmd_guards`**

In `src/cli/guards.rs`, remove the lines that auto-include `session.key` in `read_blocked`:

Remove:
```rust
    // Auto-include session key path (defense in depth)
    let session_key_path = format!("{}/session.key", config.paths.data_dir);
    if !read_blocked.contains(&session_key_path) {
        read_blocked.push(session_key_path);
    }
```

- [ ] **Step 4: Remove `cmd_session_key_path` and `Config` subcommand**

In `src/cli/config_cmd.rs`, remove the `cmd_session_key_path` function. If it's the only function, replace the file contents with:

```rust
// src/cli/config_cmd.rs
//
// Configuration query commands.
// (session-key-path removed — keys are daemon-only now)
```

In `src/main.rs`:
- Remove the `ConfigAction` enum entirely
- Remove `Config { action: ConfigAction }` from `Commands`
- Remove the `Commands::Config` dispatch arm
- Remove `use sahjhan::cli::config_cmd;`

- [ ] **Step 5: Verify it compiles**

Run: `cargo check`
Expected: compiles. Some tests will fail (they reference disk session keys).

- [ ] **Step 6: Commit**

```bash
git add src/cli/init.rs src/cli/ledger.rs src/cli/guards.rs src/cli/config_cmd.rs src/main.rs
git commit -m "feat: remove all disk-based session key code"
```

---

### Task 5: Update Tests

**Files:**
- Modify: `tests/auth_tests.rs`
- Modify: `tests/config_integrity_tests.rs`
- Modify: `tests/daemon_signing_tests.rs`
- Possibly modify: other test files that check for session.key

This is the largest task. Every test that touches session keys or HMAC proofs must be rewritten to use the daemon.

- [ ] **Step 1: Identify all failing tests**

Run: `cargo test 2>&1 | grep "FAILED\|error\[" | head -30`
Expected: list of failing tests and compilation errors in test files

- [ ] **Step 2: Fix `tests/auth_tests.rs`**

This file needs the most changes. The general pattern:
- Tests that checked `session.key` exists on disk → remove or replace with "daemon is required" test
- Tests that computed HMAC proofs from the disk key → start a daemon, use `sahjhan sign` to get proofs
- Tests that checked `session-key-path` command → remove

**Specific rewrites needed:**

`test_init_creates_session_key` → Remove this test (init no longer creates a key).

`test_ledger_create_generates_per_ledger_key` → Remove this test (ledger create no longer creates keys).

`test_config_session_key_path_global` → Remove this test (command no longer exists).

`test_config_session_key_path_per_ledger` → Remove this test (command no longer exists).

`test_guards_returns_json_with_auto_included_key` → Rewrite: verify guards output does NOT include session.key path.

`test_guards_without_config_section` → Rewrite: verify guards output is empty when no guards configured (no auto-included session key).

`test_authed_event_valid_proof` → Rewrite:
```rust
#[test]
#[ignore] // requires daemon
fn test_authed_event_valid_proof() {
    let dir = setup_auth_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Get proof from daemon
    let sign_output = Command::cargo_bin("sahjhan").unwrap()
        .args(["--config-dir", "enforcement", "sign",
               "--event-type", "quiz_answered",
               "--field", "score=5/5",
               "--field", "pass=true"])
        .current_dir(dir.path())
        .output().unwrap();
    assert!(sign_output.status.success());
    let proof = String::from_utf8_lossy(&sign_output.stdout).trim().to_string();

    // Use proof with authed-event
    Command::cargo_bin("sahjhan").unwrap()
        .args(["--config-dir", "enforcement", "authed-event", "quiz_answered",
               "--field", "score=5/5", "--field", "pass=true",
               "--proof", &proof])
        .current_dir(dir.path())
        .assert()
        .success();

    daemon.kill().ok();
    daemon.wait().ok();
}
```

`test_authed_event_invalid_proof` → Rewrite: start daemon, pass a wrong proof, assert failure.

`test_authed_event_rejects_unrestricted_type` → Keep mostly as-is (this test doesn't need the daemon — it checks that the CLI rejects non-restricted event types before even verifying the proof). But it will need a daemon running since `authed-event` now talks to the daemon. Actually — the "unrestricted" check happens before the daemon call, so this can stay without a daemon. The function returns early before hitting verify_cmd. Check the code flow: yes, the restricted check is before the verify call. This test can stay as-is.

Wait — `authed-event` now calls `verify_cmd::cmd_verify` which calls `daemon_cmd::resolve_socket_path`. If the daemon isn't running, `resolve_socket_path` will fail. But the restricted check happens before the verify call, so for unrestricted event types the function returns early without hitting the daemon. This test should still pass without a daemon. Verify after implementation.

- [ ] **Step 3: Fix `tests/config_integrity_tests.rs`**

The `test_cli_reseal_with_valid_proof_succeeds` test reads the disk session key to compute a proof. Rewrite:

```rust
#[test]
#[ignore] // requires daemon
fn test_cli_reseal_with_valid_proof_succeeds() {
    let dir = setup_config_integrity_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Modify a config file so reseal is needed
    // ... (keep existing config modification logic) ...

    // Get reseal proof from daemon
    let sign_output = Command::cargo_bin("sahjhan").unwrap()
        .args(["--config-dir", "enforcement", "sign",
               "--event-type", "config_reseal"])
        .current_dir(dir.path())
        .output().unwrap();
    assert!(sign_output.status.success());
    let proof = String::from_utf8_lossy(&sign_output.stdout).trim().to_string();

    // Reseal with the proof
    Command::cargo_bin("sahjhan").unwrap()
        .args(["--config-dir", "enforcement", "reseal", "--proof", &proof])
        .current_dir(dir.path())
        .assert()
        .success();

    daemon.kill().ok();
    daemon.wait().ok();
}
```

- [ ] **Step 4: Add sign → authed-event e2e test to `tests/daemon_signing_tests.rs`**

Add:

```rust
#[test]
#[ignore]
fn test_sign_then_authed_event_full_flow() {
    let dir = setup_signing_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Sign
    let sign_output = Command::cargo_bin("sahjhan").unwrap()
        .args(["--config-dir", "enforcement", "sign",
               "--event-type", "quiz_answered",
               "--field", "score=5", "--field", "pass=true"])
        .current_dir(dir.path())
        .output().unwrap();
    assert!(sign_output.status.success());
    let proof = String::from_utf8_lossy(&sign_output.stdout).trim().to_string();

    // Authed-event with that proof
    Command::cargo_bin("sahjhan").unwrap()
        .args(["--config-dir", "enforcement", "authed-event", "quiz_answered",
               "--field", "score=5", "--field", "pass=true",
               "--proof", &proof])
        .current_dir(dir.path())
        .assert()
        .success();

    // Verify event was recorded
    Command::cargo_bin("sahjhan").unwrap()
        .args(["--config-dir", "enforcement", "log", "tail", "1"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicates::str::contains("quiz_answered"));

    daemon.kill().ok();
    daemon.wait().ok();
}
```

Note: `setup_signing_dir` already creates events.toml with `quiz_answered` as restricted. The daemon test helpers (`start_daemon`, `wait_for_socket`) are already defined in this file.

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: all non-ignored tests pass

Run: `cargo test --test daemon_signing_tests -- --ignored --test-threads=1`
Expected: all ignored daemon tests pass including the new sign→authed-event test

Run: `cargo test --test auth_tests -- --ignored --test-threads=1`
Expected: rewritten auth tests pass

- [ ] **Step 6: Run clippy and fmt**

Run: `cargo clippy -- -D warnings && cargo fmt`

- [ ] **Step 7: Update CLAUDE.md**

Update the module lookup tables:
- Remove `resolve_session_key_path` from cli/ table
- Add `cmd_verify` to cli/ table
- Update `authed_event.rs` description to note daemon verification
- Remove `config session-key-path` from CLI commands list

- [ ] **Step 8: Commit**

```bash
git add -A
git commit -m "feat: daemon-only auth — remove disk keys, wire auth, update tests

authed-event and reseal now verify proofs via daemon. Session keys
exist only in daemon memory. Caller authentication wired into
handle_connection with trusted-callers manifest."
```
