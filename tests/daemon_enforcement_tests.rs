//! Tests for enforcement state operations (#27).

use assert_cmd::Command;
use base64::Engine;
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
        "[states.idle]\nlabel = \"Idle\"\ninitial = true\n\n[states.working]\nlabel = \"Working\"\n",
    )
    .unwrap();

    std::fs::write(
        config_dir.join("transitions.toml"),
        "[[transitions]]\nfrom = \"idle\"\nto = \"idle\"\ncommand = \"noop\"\ngates = []\n\n\
         [[transitions]]\nfrom = \"idle\"\nto = \"working\"\ncommand = \"go\"\ngates = []\n",
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

    let data =
        base64::engine::general_purpose::STANDARD.encode(r#"{"state": "active", "score": 42}"#);
    let write_req = format!(r#"{{"op": "enforcement_write", "data": "{}"}}"#, data);
    let write_resp = send_request(dir.path(), &write_req);
    assert_eq!(write_resp["ok"], true);

    let read_resp = send_request(dir.path(), r#"{"op": "enforcement_read"}"#);
    assert_eq!(read_resp["ok"], true);
    let read_data = read_resp["data"].as_str().unwrap();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(read_data)
        .unwrap();
    let obj: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    // The stored "state" is overridden at read time by the ledger-derived
    // state (holtz #57): a fresh ledger has no state_transition, so the
    // config's initial state wins over the stale stored value.
    assert_eq!(obj["state"], "idle");
    assert_eq!(obj["score"], 42);
    assert!(
        obj["last_refresh"].is_string(),
        "last_refresh should be present"
    );

    stop_daemon(&mut daemon);
}

// ---------------------------------------------------------------------------
// Ledger-state overlay on enforcement_read (holtz #57)
// ---------------------------------------------------------------------------

/// Write a stale enforcement blob, then send the request and return the
/// decoded blob from the response.
fn read_enforcement_blob(dir: &std::path::Path) -> serde_json::Value {
    let resp = send_request(dir, r#"{"op": "enforcement_read"}"#);
    assert_eq!(resp["ok"], true, "enforcement_read failed: {:?}", resp);
    let data = resp["data"].as_str().unwrap();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(data)
        .unwrap();
    serde_json::from_slice(&decoded).unwrap()
}

fn write_enforcement_blob(dir: &std::path::Path, json: &str) {
    let data = base64::engine::general_purpose::STANDARD.encode(json);
    let req = format!(r#"{{"op": "enforcement_write", "data": "{}"}}"#, data);
    let resp = send_request(dir, &req);
    assert_eq!(resp["ok"], true, "enforcement_write failed: {:?}", resp);
}

#[test]
#[ignore]
fn test_enforcement_read_overlays_state_from_ledger_transition() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Stale blob: consumer's hook last saw "merge_ready"
    write_enforcement_blob(dir.path(), r#"{"state": "merge_ready", "stall": 3}"#);

    // The ledger advances without any enforcement_write (the #57 scenario)
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "go"])
        .current_dir(dir.path())
        .assert()
        .success();

    let obj = read_enforcement_blob(dir.path());
    assert_eq!(
        obj["state"], "working",
        "read must serve ledger-derived state, not the stale stored value"
    );
    // Other consumer-owned fields pass through untouched
    assert_eq!(obj["stall"], 3);

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_enforcement_read_serves_stored_state_when_ledger_invalid() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    write_enforcement_blob(dir.path(), r#"{"state": "merge_ready"}"#);

    // Corrupt the ledger: chain verification must fail, so the overlay
    // is skipped and the stored blob is served unchanged (fail-soft).
    let ledger_path = dir.path().join("output/.sahjhan/ledger.jsonl");
    let mut contents = std::fs::read_to_string(&ledger_path).unwrap();
    contents.push_str("{\"garbage\": true}\n");
    std::fs::write(&ledger_path, contents).unwrap();

    let obj = read_enforcement_blob(dir.path());
    assert_eq!(
        obj["state"], "merge_ready",
        "corrupt ledger must not produce a derived state"
    );

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_enforcement_read_overlay_follows_active_ledger() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    write_enforcement_blob(dir.path(), r#"{"state": "merge_ready"}"#);

    // Create and activate a second ledger, then advance state on it.
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "ledger",
            "create",
            "--name",
            "run-1",
            "--path",
            "output/.sahjhan/run-1.jsonl",
            "--activate",
        ])
        .current_dir(dir.path())
        .assert()
        .success();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "go"])
        .current_dir(dir.path())
        .assert()
        .success();

    let obj = read_enforcement_blob(dir.path());
    assert_eq!(
        obj["state"], "working",
        "overlay must derive state from the ACTIVE ledger"
    );

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_enforcement_update_merges_top_level() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    let initial = base64::engine::general_purpose::STANDARD
        .encode(r#"{"state": "active", "score": 42, "items": [1, 2]}"#);
    let write_req = format!(r#"{{"op": "enforcement_write", "data": "{}"}}"#, initial);
    let write_resp = send_request(dir.path(), &write_req);
    assert_eq!(write_resp["ok"], true);

    let patch = base64::engine::general_purpose::STANDARD
        .encode(r#"{"score": 99, "new_field": "hello", "items": [3, 4, 5]}"#);
    let update_req = format!(r#"{{"op": "enforcement_update", "patch": "{}"}}"#, patch);
    let update_resp = send_request(dir.path(), &update_req);
    assert_eq!(update_resp["ok"], true);

    let merged_data = update_resp["data"].as_str().unwrap();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(merged_data)
        .unwrap();
    let obj: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    assert_eq!(obj["state"], "active");
    assert_eq!(obj["score"], 99);
    assert_eq!(obj["new_field"], "hello");
    assert_eq!(obj["items"], serde_json::json!([3, 4, 5]));
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

    let initial = base64::engine::general_purpose::STANDARD.encode(r#"{"state": "active"}"#);
    let write_req = format!(r#"{{"op": "enforcement_write", "data": "{}"}}"#, initial);
    send_request(dir.path(), &write_req);

    std::thread::sleep(std::time::Duration::from_millis(50));

    let patch = base64::engine::general_purpose::STANDARD.encode(r#"{"x": 1}"#);
    let update_req = format!(r#"{{"op": "enforcement_update", "patch": "{}"}}"#, patch);
    let resp = send_request(dir.path(), &update_req);
    assert_eq!(resp["ok"], true);

    let data = resp["data"].as_str().unwrap();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(data)
        .unwrap();
    let obj: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    let ts = obj["last_refresh"]
        .as_str()
        .expect("last_refresh should be a string");
    assert!(
        ts.contains('T') && ts.contains(':'),
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

    let data = base64::engine::general_purpose::STANDARD.encode(b"hello");
    let store_req = format!(
        r#"{{"op": "vault_store", "name": "user-key", "data": "{}"}}"#,
        data
    );
    let store_resp = send_request(dir.path(), &store_req);
    assert_eq!(store_resp["ok"], true);

    let enf_data = base64::engine::general_purpose::STANDARD.encode(r#"{"active": true}"#);
    let enf_req = format!(r#"{{"op": "enforcement_write", "data": "{}"}}"#, enf_data);
    let enf_resp = send_request(dir.path(), &enf_req);
    assert_eq!(enf_resp["ok"], true);

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

    let read_resp = send_request(dir.path(), r#"{"op": "enforcement_read"}"#);
    assert_eq!(read_resp["ok"], true);
    let data = read_resp["data"].as_str().unwrap();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(data)
        .unwrap();
    let obj: serde_json::Value = serde_json::from_slice(&decoded).unwrap();
    // "state" is overridden at read time by the ledger-derived state
    // (holtz #57); the fixture ledger has no transitions, so the initial
    // state wins over the stored "auditing".
    assert_eq!(obj["state"], "idle");
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

    let status1 = send_request(dir.path(), r#"{"op": "status"}"#);
    assert_eq!(status1["enforcement_active"], false);

    let data = base64::engine::general_purpose::STANDARD.encode(r#"{"active": true}"#);
    send_request(
        dir.path(),
        &format!(r#"{{"op": "enforcement_write", "data": "{}"}}"#, data),
    );

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
