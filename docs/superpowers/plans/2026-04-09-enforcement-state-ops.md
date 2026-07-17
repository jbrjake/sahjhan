# Enforcement State Operations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `enforcement_read`, `enforcement_write`, and `enforcement_update` daemon operations that store enforcement state in daemon memory, inaccessible to agents via generic vault ops.

**Architecture:** Three new wire protocol operations stored under the reserved vault key `"_enforcement"`. Generic vault ops (`vault_store`, `vault_read`, `vault_delete`, `vault_list`) reject/filter names starting with `_`. The `enforcement_update` op does atomic read-modify-write with top-level key merge. Both write and update inject `last_refresh` UTC timestamp from the daemon clock.

**Tech Stack:** Rust, serde_json (JSON parse/merge), chrono (UTC timestamp), base64 (wire encoding)

---

### Task 1: Wire Protocol — New Request Variants

**Files:**
- Modify: `src/daemon/protocol.rs:14-38` (Request enum)
- Test: `tests/daemon_protocol_tests.rs`

- [ ] **Step 1: Write failing tests for parsing the three new request types**

Add to `tests/daemon_protocol_tests.rs`:

```rust
#[test]
fn test_parse_enforcement_read_request() {
    let json = r#"{"op": "enforcement_read"}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    assert!(matches!(req, Request::EnforcementRead));
}

#[test]
fn test_parse_enforcement_write_request() {
    let json = r#"{"op": "enforcement_write", "data": "eyJzdGF0ZSI6ICJhY3RpdmUifQ=="}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    match req {
        Request::EnforcementWrite { data } => {
            assert_eq!(data, "eyJzdGF0ZSI6ICJhY3RpdmUifQ==");
        }
        _ => panic!("Expected EnforcementWrite"),
    }
}

#[test]
fn test_parse_enforcement_update_request() {
    let json = r#"{"op": "enforcement_update", "patch": "eyJhY3RpdmUiOiB0cnVlfQ=="}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    match req {
        Request::EnforcementUpdate { patch } => {
            assert_eq!(patch, "eyJhY3RpdmUiOiB0cnVlfQ==");
        }
        _ => panic!("Expected EnforcementUpdate"),
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_parse_enforcement -- --nocapture`
Expected: FAIL — `EnforcementRead`, `EnforcementWrite`, `EnforcementUpdate` variants don't exist.

- [ ] **Step 3: Add the three Request variants to protocol.rs**

In `src/daemon/protocol.rs`, add to the `Request` enum after the `Verify` variant:

```rust
    #[serde(rename = "enforcement_read")]
    EnforcementRead,
    #[serde(rename = "enforcement_write")]
    EnforcementWrite { data: String },
    #[serde(rename = "enforcement_update")]
    EnforcementUpdate { patch: String },
```

Update the file's `## Index` comment to mention the new variants.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test test_parse_enforcement -- --nocapture`
Expected: PASS — all three parse correctly.

- [ ] **Step 5: Commit**

```bash
git add src/daemon/protocol.rs tests/daemon_protocol_tests.rs
git commit -m "feat(protocol): add enforcement_read/write/update Request variants (#27)"
```

---

### Task 2: Wire Protocol — enforcement_active in Status Response

**Files:**
- Modify: `src/daemon/protocol.rs:42-220` (Response struct + ok_status constructor)
- Test: `tests/daemon_protocol_tests.rs`

- [ ] **Step 1: Write failing tests for enforcement_active in status response**

Add to `tests/daemon_protocol_tests.rs`:

```rust
#[test]
fn test_serialize_ok_status_includes_enforcement_active() {
    let resp = Response::ok_status(12345, 3600, 2, 0, 0, true);
    let json = serde_json::to_string(&resp).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["enforcement_active"], true);
}

#[test]
fn test_serialize_ok_status_enforcement_inactive() {
    let resp = Response::ok_status(12345, 3600, 2, 0, 0, false);
    let json = serde_json::to_string(&resp).unwrap();
    let v: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["enforcement_active"], false);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_serialize_ok_status_includes_enforcement -- --nocapture`
Expected: FAIL — `ok_status` doesn't accept 6 args yet.

- [ ] **Step 3: Add enforcement_active field to Response and update ok_status**

In `src/daemon/protocol.rs`, add to the `Response` struct after `idle_timeout`:

```rust
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enforcement_active: Option<bool>,
```

Update **every** `Response` constructor to include `enforcement_active: None` in its struct literal, except `ok_status` which gets the new parameter:

```rust
    pub fn ok_status(
        pid: u32,
        uptime_seconds: u64,
        vault_entries: usize,
        idle_seconds: u64,
        idle_timeout: u64,
        enforcement_active: bool,
    ) -> Self {
        Self {
            // ... existing fields ...
            enforcement_active: Some(enforcement_active),
            // ... rest ...
        }
    }
```

- [ ] **Step 4: Fix all existing ok_status call sites**

There are two call sites that pass 5 args to `ok_status`:

1. `src/daemon/mod.rs` in `handle_request` — the `Request::Status` arm. This needs the vault to check for `_enforcement`. Change to:

```rust
        Request::Status => {
            let pid = std::process::id();
            let uptime = start_time.elapsed().as_secs();
            let idle_secs = last_activity.elapsed().as_secs();
            let (vault_entries, enforcement_active) = match vault.lock() {
                Ok(v) => (v.list().len(), v.read("_enforcement").is_some()),
                Err(_) => (0, false),
            };
            Response::ok_status(pid, uptime, vault_entries, idle_secs, idle_timeout, enforcement_active)
        }
```

2. `tests/daemon_protocol_tests.rs` — existing tests `test_serialize_ok_status_response` and `test_serialize_ok_status_includes_idle_fields` both call `Response::ok_status(12345, 3600, 2, ...)`. Add `false` as the 6th arg to each.

- [ ] **Step 5: Run full test suite to verify everything compiles and passes**

Run: `cargo test`
Expected: PASS — all existing tests pass, new tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/daemon/protocol.rs src/daemon/mod.rs tests/daemon_protocol_tests.rs
git commit -m "feat(protocol): add enforcement_active to status response (#27)"
```

---

### Task 3: Protected Vault Namespace

**Files:**
- Modify: `src/daemon/mod.rs:382-448` (handle_request vault arms)
- Test: `tests/daemon_protocol_tests.rs` (unit) and `tests/daemon_vault_tests.rs`

- [ ] **Step 1: Write failing tests for reserved namespace rejection**

Add to `tests/daemon_protocol_tests.rs`:

```rust
#[test]
fn test_parse_vault_store_reserved_name() {
    // Verify the request parses — the rejection happens in the handler, not the parser
    let json = r#"{"op": "vault_store", "name": "_enforcement", "data": "aGVsbG8="}"#;
    let req: Request = serde_json::from_str(json).unwrap();
    assert!(matches!(req, Request::VaultStore { .. }));
}
```

Add a handler-level test. Since `handle_request` is a private function, we test this via the E2E tests in Task 7. For now, add a vault-level comment test to `tests/daemon_vault_tests.rs` documenting the contract:

```rust
#[test]
fn test_vault_stores_reserved_names_at_data_level() {
    // The vault itself has no namespace protection — it stores anything.
    // The _enforcement namespace guard lives in handle_request, not the vault.
    let mut vault = Vault::new();
    vault.store("_enforcement".to_string(), b"data".to_vec());
    assert_eq!(vault.read("_enforcement").unwrap(), b"data");
}
```

- [ ] **Step 2: Run tests to verify they pass (these are documenting existing behavior)**

Run: `cargo test test_vault_stores_reserved_names -- --nocapture`
Expected: PASS.

- [ ] **Step 3: Add reserved namespace guards to handle_request**

In `src/daemon/mod.rs`, modify the three vault arms in `handle_request`:

For `VaultStore`:
```rust
        Request::VaultStore { name, data } => {
            if name.starts_with('_') {
                return Response::err("reserved", "vault names starting with '_' are reserved");
            }
            // ... existing decode + store logic unchanged ...
        }
```

For `VaultRead`:
```rust
        Request::VaultRead { name } => {
            if name.starts_with('_') {
                return Response::err("reserved", "vault names starting with '_' are reserved");
            }
            // ... existing logic unchanged ...
        }
```

For `VaultDelete`:
```rust
        Request::VaultDelete { name } => {
            if name.starts_with('_') {
                return Response::err("reserved", "vault names starting with '_' are reserved");
            }
            // ... existing logic unchanged ...
        }
```

For `VaultList`, filter out `_`-prefixed entries:
```rust
        Request::VaultList => match vault.lock() {
            Ok(v) => {
                let names: Vec<String> = v
                    .list()
                    .into_iter()
                    .filter(|s| !s.starts_with('_'))
                    .map(|s| s.to_string())
                    .collect();
                Response::ok_names(names)
            }
            Err(e) => Response::err("internal_error", &format!("vault lock poisoned: {}", e)),
        },
```

- [ ] **Step 4: Run all tests to verify nothing broke**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/daemon/mod.rs tests/daemon_protocol_tests.rs tests/daemon_vault_tests.rs
git commit -m "feat(daemon): protect _-prefixed vault namespace from generic ops (#27)"
```

---

### Task 4: Enforcement State Handlers

**Files:**
- Modify: `src/daemon/mod.rs:374-448` (handle_request — add 3 new match arms)

- [ ] **Step 1: Add enforcement_read handler**

In `handle_request`, add a match arm after the `Verify` arm:

```rust
        Request::EnforcementRead => match vault.lock() {
            Ok(v) => match v.read("_enforcement") {
                Some(bytes) => {
                    let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                    Response::ok_data(&encoded)
                }
                None => Response::err("not_found", "no enforcement state"),
            },
            Err(e) => Response::err("internal_error", &format!("vault lock poisoned: {}", e)),
        },
```

- [ ] **Step 2: Add enforcement_write handler**

```rust
        Request::EnforcementWrite { data } => {
            let bytes = match base64::engine::general_purpose::STANDARD.decode(&data) {
                Ok(b) => b,
                Err(e) => {
                    return Response::err("decode_error", &format!("invalid base64: {}", e));
                }
            };
            // Parse as JSON to validate well-formedness
            let mut obj: serde_json::Map<String, serde_json::Value> = match serde_json::from_slice(&bytes) {
                Ok(serde_json::Value::Object(m)) => m,
                Ok(_) => {
                    return Response::err("invalid_data", "enforcement state must be a JSON object");
                }
                Err(e) => {
                    return Response::err("invalid_data", &format!("invalid JSON: {}", e));
                }
            };
            // Inject last_refresh from daemon clock
            obj.insert(
                "last_refresh".to_string(),
                serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
            );
            let serialized = serde_json::to_vec(&obj).expect("re-serialization cannot fail");
            match vault.lock() {
                Ok(mut v) => {
                    v.store("_enforcement".to_string(), serialized);
                    Response::ok_empty()
                }
                Err(e) => Response::err("internal_error", &format!("vault lock poisoned: {}", e)),
            }
        }
```

- [ ] **Step 3: Add enforcement_update handler**

```rust
        Request::EnforcementUpdate { patch } => {
            let patch_bytes = match base64::engine::general_purpose::STANDARD.decode(&patch) {
                Ok(b) => b,
                Err(e) => {
                    return Response::err("decode_error", &format!("invalid base64: {}", e));
                }
            };
            let patch_obj: serde_json::Map<String, serde_json::Value> = match serde_json::from_slice(&patch_bytes) {
                Ok(serde_json::Value::Object(m)) => m,
                Ok(_) => {
                    return Response::err("invalid_data", "patch must be a JSON object");
                }
                Err(e) => {
                    return Response::err("invalid_data", &format!("invalid JSON: {}", e));
                }
            };
            match vault.lock() {
                Ok(mut v) => {
                    let current = match v.read("_enforcement") {
                        Some(bytes) => bytes.to_vec(),
                        None => {
                            return Response::err("not_found", "no enforcement state to update");
                        }
                    };
                    let mut state: serde_json::Map<String, serde_json::Value> =
                        match serde_json::from_slice(&current) {
                            Ok(serde_json::Value::Object(m)) => m,
                            _ => {
                                return Response::err(
                                    "internal_error",
                                    "stored enforcement state is not a valid JSON object",
                                );
                            }
                        };
                    state.extend(patch_obj);
                    state.insert(
                        "last_refresh".to_string(),
                        serde_json::Value::String(chrono::Utc::now().to_rfc3339()),
                    );
                    let serialized =
                        serde_json::to_vec(&state).expect("re-serialization cannot fail");
                    let encoded = base64::engine::general_purpose::STANDARD.encode(&serialized);
                    v.store("_enforcement".to_string(), serialized);
                    Response::ok_data(&encoded)
                }
                Err(e) => Response::err("internal_error", &format!("vault lock poisoned: {}", e)),
            }
        }
```

- [ ] **Step 4: Run all tests to verify compilation and no regressions**

Run: `cargo test`
Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/daemon/mod.rs
git commit -m "feat(daemon): implement enforcement_read/write/update handlers (#27)"
```

---

### Task 5: Unit Tests — Enforcement Operations

**Files:**
- Create: `tests/daemon_enforcement_tests.rs`

These tests exercise the enforcement operations via raw socket against a live daemon, following the same pattern as `daemon_signing_tests.rs`.

- [ ] **Step 1: Create test file with helpers and write-then-read round-trip test**

Create `tests/daemon_enforcement_tests.rs`:

```rust
//! Tests for enforcement state operations (#27).

use assert_cmd::Command;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Helpers (same pattern as daemon_signing_tests.rs)
// ---------------------------------------------------------------------------

fn setup_dir() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        r#"[protocol]
name = "test-enforcement"
version = "1.0.0"
description = "Enforcement ops test"

[paths]
managed = ["output"]
data_dir = "output/.sahjhan"
render_dir = "output"
"#,
    )
    .unwrap();

    std::fs::write(
        config_dir.join("states.toml"),
        "[states.idle]\nlabel = \"Idle\"\ninitial = true\n",
    )
    .unwrap();

    std::fs::write(
        config_dir.join("transitions.toml"),
        "[[transitions]]\nfrom = \"idle\"\nto = \"idle\"\ncommand = \"noop\"\ngates = []\n",
    )
    .unwrap();

    std::fs::write(config_dir.join("trusted-callers.toml"), "[callers]\n").unwrap();

    std::fs::create_dir_all(dir.path().join("output")).unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "init"])
        .current_dir(dir.path())
        .assert()
        .success();

    dir
}

fn start_daemon(dir: &std::path::Path) -> std::process::Child {
    std::process::Command::new(env!("CARGO_BIN_EXE_sahjhan"))
        .args(["--config-dir", "enforcement", "daemon", "start"])
        .current_dir(dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to start daemon")
}

fn wait_for_socket(dir: &std::path::Path) {
    let socket_path = dir.join("output/.sahjhan/daemon.sock");
    for _ in 0..50 {
        if socket_path.exists() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("Daemon socket did not appear at {:?}", socket_path);
}

fn stop_daemon(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

fn socket_path(dir: &std::path::Path) -> std::path::PathBuf {
    dir.join("output/.sahjhan/daemon.sock")
}

/// Send a JSON request to the daemon and return the parsed response.
fn send_request(dir: &std::path::Path, request: &str) -> serde_json::Value {
    let mut stream = UnixStream::connect(socket_path(dir)).expect("connect to daemon");
    writeln!(stream, "{}", request).expect("write request");
    let reader = BufReader::new(&stream);
    let line = reader
        .lines()
        .next()
        .expect("should get a response")
        .expect("response should be readable");
    serde_json::from_str(&line).expect("response should be valid JSON")
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_enforcement_read_not_found() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    let resp = send_request(dir.path(), r#"{"op": "enforcement_read"}"#);
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"], "not_found");
    assert_eq!(resp["message"], "no enforcement state");

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_enforcement_write_then_read() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Write enforcement state: {"state": "active", "score": 42}
    // base64 of that JSON: eyJzdGF0ZSI6ICJhY3RpdmUiLCAic2NvcmUiOiA0Mn0=
    let data = base64::engine::general_purpose::STANDARD
        .encode(r#"{"state": "active", "score": 42}"#);
    let write_req = format!(r#"{{"op": "enforcement_write", "data": "{}"}}"#, data);
    let write_resp = send_request(dir.path(), &write_req);
    assert_eq!(write_resp["ok"], true);

    // Read it back
    let read_resp = send_request(dir.path(), r#"{"op": "enforcement_read"}"#);
    assert_eq!(read_resp["ok"], true);
    let read_data = read_resp["data"].as_str().unwrap();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(read_data)
        .unwrap();
    let obj: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    assert_eq!(obj["state"], "active");
    assert_eq!(obj["score"], 42);
    // last_refresh should have been injected
    assert!(
        obj["last_refresh"].is_string(),
        "last_refresh should be present"
    );

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_enforcement_update_merges_top_level() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Write initial state
    let initial = base64::engine::general_purpose::STANDARD
        .encode(r#"{"state": "active", "score": 42, "items": [1, 2]}"#);
    let write_req = format!(r#"{{"op": "enforcement_write", "data": "{}"}}"#, initial);
    let write_resp = send_request(dir.path(), &write_req);
    assert_eq!(write_resp["ok"], true);

    // Update: change score, add new field, replace items array
    let patch = base64::engine::general_purpose::STANDARD
        .encode(r#"{"score": 99, "new_field": "hello", "items": [3, 4, 5]}"#);
    let update_req = format!(r#"{{"op": "enforcement_update", "patch": "{}"}}"#, patch);
    let update_resp = send_request(dir.path(), &update_req);
    assert_eq!(update_resp["ok"], true);

    // The response should contain the merged state
    let merged_data = update_resp["data"].as_str().unwrap();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(merged_data)
        .unwrap();
    let obj: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    assert_eq!(obj["state"], "active"); // unchanged
    assert_eq!(obj["score"], 99); // updated
    assert_eq!(obj["new_field"], "hello"); // added
    assert_eq!(obj["items"], serde_json::json!([3, 4, 5])); // replaced, not merged
    assert!(obj["last_refresh"].is_string());

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_enforcement_update_on_missing_state_returns_not_found() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    let patch = base64::engine::general_purpose::STANDARD.encode(r#"{"x": 1}"#);
    let req = format!(r#"{{"op": "enforcement_update", "patch": "{}"}}"#, patch);
    let resp = send_request(dir.path(), &req);
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"], "not_found");
    assert_eq!(resp["message"], "no enforcement state to update");

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_enforcement_update_sets_last_refresh() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Write initial state
    let initial =
        base64::engine::general_purpose::STANDARD.encode(r#"{"state": "active"}"#);
    let write_req = format!(r#"{{"op": "enforcement_write", "data": "{}"}}"#, initial);
    send_request(dir.path(), &write_req);

    // Small delay to ensure timestamps differ
    std::thread::sleep(std::time::Duration::from_millis(50));

    // Update
    let patch = base64::engine::general_purpose::STANDARD.encode(r#"{"x": 1}"#);
    let update_req = format!(r#"{{"op": "enforcement_update", "patch": "{}"}}"#, patch);
    let resp = send_request(dir.path(), &update_req);
    assert_eq!(resp["ok"], true);

    let data = resp["data"].as_str().unwrap();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(data)
        .unwrap();
    let obj: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    let ts = obj["last_refresh"].as_str().expect("last_refresh should be a string");
    // Should be a valid ISO8601/RFC3339 timestamp
    assert!(
        ts.contains("T") && ts.contains(":"),
        "last_refresh should be an ISO8601 timestamp, got: {}",
        ts
    );

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_vault_store_rejects_reserved_name() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    let data = base64::engine::general_purpose::STANDARD.encode(b"sneaky");
    let req = format!(
        r#"{{"op": "vault_store", "name": "_enforcement", "data": "{}"}}"#,
        data
    );
    let resp = send_request(dir.path(), &req);
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"], "reserved");

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_vault_read_rejects_reserved_name() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    let resp = send_request(
        dir.path(),
        r#"{"op": "vault_read", "name": "_enforcement"}"#,
    );
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"], "reserved");

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_vault_delete_rejects_reserved_name() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    let resp = send_request(
        dir.path(),
        r#"{"op": "vault_delete", "name": "_enforcement"}"#,
    );
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"], "reserved");

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_vault_list_hides_enforcement_entry() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Store a normal vault entry
    let data = base64::engine::general_purpose::STANDARD.encode(b"hello");
    let store_req = format!(
        r#"{{"op": "vault_store", "name": "user-key", "data": "{}"}}"#,
        data
    );
    let store_resp = send_request(dir.path(), &store_req);
    assert_eq!(store_resp["ok"], true);

    // Write enforcement state (creates _enforcement entry)
    let enf_data =
        base64::engine::general_purpose::STANDARD.encode(r#"{"active": true}"#);
    let enf_req = format!(r#"{{"op": "enforcement_write", "data": "{}"}}"#, enf_data);
    let enf_resp = send_request(dir.path(), &enf_req);
    assert_eq!(enf_resp["ok"], true);

    // vault_list should show user-key but NOT _enforcement
    let list_resp = send_request(dir.path(), r#"{"op": "vault_list"}"#);
    assert_eq!(list_resp["ok"], true);
    let names: Vec<String> = list_resp["names"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    assert!(names.contains(&"user-key".to_string()));
    assert!(
        !names.iter().any(|n| n.starts_with('_')),
        "vault_list should not expose _-prefixed entries, got: {:?}",
        names
    );

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_enforcement_write_then_read_full_round_trip() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Write → Read → Update → Read — full lifecycle
    let initial = base64::engine::general_purpose::STANDARD
        .encode(r#"{"state": "auditing", "items_remaining": 5}"#);
    send_request(
        dir.path(),
        &format!(r#"{{"op": "enforcement_write", "data": "{}"}}"#, initial),
    );

    let patch = base64::engine::general_purpose::STANDARD
        .encode(r#"{"items_remaining": 3, "last_item": "auth.rs"}"#);
    let update_resp = send_request(
        dir.path(),
        &format!(r#"{{"op": "enforcement_update", "patch": "{}"}}"#, patch),
    );
    assert_eq!(update_resp["ok"], true);

    // Final read should reflect merged state
    let read_resp = send_request(dir.path(), r#"{"op": "enforcement_read"}"#);
    assert_eq!(read_resp["ok"], true);
    let data = read_resp["data"].as_str().unwrap();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(data)
        .unwrap();
    let obj: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    assert_eq!(obj["state"], "auditing");
    assert_eq!(obj["items_remaining"], 3);
    assert_eq!(obj["last_item"], "auth.rs");
    assert!(obj["last_refresh"].is_string());

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_enforcement_status_shows_active() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Before write — enforcement_active should be false
    let status1 = send_request(dir.path(), r#"{"op": "status"}"#);
    assert_eq!(status1["enforcement_active"], false);

    // Write enforcement state
    let data =
        base64::engine::general_purpose::STANDARD.encode(r#"{"active": true}"#);
    send_request(
        dir.path(),
        &format!(r#"{{"op": "enforcement_write", "data": "{}"}}"#, data),
    );

    // After write — enforcement_active should be true
    let status2 = send_request(dir.path(), r#"{"op": "status"}"#);
    assert_eq!(status2["enforcement_active"], true);

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_enforcement_write_rejects_non_object() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Send a JSON array instead of object
    let data = base64::engine::general_purpose::STANDARD.encode(r#"[1, 2, 3]"#);
    let req = format!(r#"{{"op": "enforcement_write", "data": "{}"}}"#, data);
    let resp = send_request(dir.path(), &req);
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"], "invalid_data");

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_enforcement_write_rejects_invalid_base64() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    let req = r#"{"op": "enforcement_write", "data": "not-valid-base64!!!"}"#;
    let resp = send_request(dir.path(), req);
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"], "decode_error");

    stop_daemon(&mut daemon);
}
```

Note: this file uses `base64::engine::general_purpose::STANDARD` so add `use base64::Engine;` at the top of the file.

- [ ] **Step 2: Run the tests**

Run: `cargo test --test daemon_enforcement_tests -- --ignored --nocapture`
Expected: PASS — all tests should pass since the handlers were implemented in Task 4.

- [ ] **Step 3: Commit**

```bash
git add tests/daemon_enforcement_tests.rs
git commit -m "test: add enforcement state operation tests (#27)"
```

---

### Task 6: Documentation Updates

**Files:**
- Modify: `CLAUDE.md` (Module Lookup Tables — daemon sections)
- Modify: `src/daemon/mod.rs` (Index comment)

- [ ] **Step 1: Update src/daemon/mod.rs Index comment**

The `## Index` at the top of `mod.rs` should include the new operations. Update lines 7-14 to add:
- `handle_request` entry should note enforcement ops
- Add note about `_enforcement` reserved namespace

- [ ] **Step 2: Update src/daemon/protocol.rs Index comment**

Update the `## Index` at lines 6-8 to mention the new `EnforcementRead/Write/Update` variants.

- [ ] **Step 3: Update CLAUDE.md daemon tables**

In the `daemon/` section of the Module Lookup Tables, add rows:

Under `daemon/protocol.rs`:
| Enforcement read | `daemon/protocol.rs` | `Request::EnforcementRead` | Wire type for enforcement_read op |
| Enforcement write | `daemon/protocol.rs` | `Request::EnforcementWrite` | Wire type for enforcement_write op (base64 JSON) |
| Enforcement update | `daemon/protocol.rs` | `Request::EnforcementUpdate` | Wire type for enforcement_update op (base64 JSON patch) |

Under `daemon/mod.rs`:
| Enforcement handlers | `daemon/mod.rs` | `handle_request` | enforcement_read/write/update: opaque JSON state in vault under `_enforcement` |
| Reserved vault namespace | `daemon/mod.rs` | `handle_request` | `_`-prefixed names rejected by generic vault ops, filtered from vault_list |

Under the Test Files table:
| `tests/daemon_enforcement_tests.rs` | Enforcement state ops: write/read round-trip, update merge, not_found, reserved namespace, vault_list filtering, status enforcement_active, validation |

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md src/daemon/mod.rs src/daemon/protocol.rs
git commit -m "docs: update indexes and lookup tables for enforcement ops (#27)"
```

---

### Task 7: Version Bump

**Files:**
- Modify: `Cargo.toml:3` (version field)

- [ ] **Step 1: Bump version**

Change `version = "0.12.0"` to `version = "0.13.0"` in `Cargo.toml`.

- [ ] **Step 2: Update Cargo.lock**

Run: `cargo build`

- [ ] **Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to 0.13.0"
```

---

### Task 8: Final Verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 2: Run ignored (E2E) tests**

Run: `cargo test -- --ignored`
Expected: All tests pass (including new enforcement tests).

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings.

- [ ] **Step 4: Run fmt check**

Run: `cargo fmt -- --check`
Expected: No formatting issues.
