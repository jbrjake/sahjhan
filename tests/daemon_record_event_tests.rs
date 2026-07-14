//! E2E tests for the `record_event` socket op: authenticated ledger append
//! for a trusted peer (the ledger-write analog of `enforcement_write`).
//!
//! These require a live daemon, so they are `#[ignore]` by default and run
//! explicitly (same pattern as daemon_enforcement_tests.rs). Auth is skipped
//! because trusted-callers.toml has an empty `[callers]` table — the record
//! path itself is what's under test, not peer authentication (covered in
//! daemon_auth_tests.rs / daemon_signing_tests.rs).

use assert_cmd::Command;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use tempfile::tempdir;

fn setup_dir() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        r#"[protocol]
name = "test-record-event"
version = "1.0.0"
description = "record_event op test"

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

    // A restricted event with field patterns — the daemon must validate
    // fields against THIS consumer-declared schema, holding no domain
    // knowledge of `context_reset` itself.
    std::fs::write(
        config_dir.join("events.toml"),
        r#"[events.context_reset]
description = "Context boundary — recorded by primer hook after /clear"
restricted = true
fields = [
    { name = "run", type = "string", pattern = "^\\d+$" },
    { name = "trigger", type = "string", pattern = "^user_prompt_submit$" },
]
"#,
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

fn send_request(dir: &std::path::Path, request: &str) -> serde_json::Value {
    let stream = UnixStream::connect(dir.join("output/.sahjhan/daemon.sock")).expect("connect");
    let mut w = stream.try_clone().unwrap();
    writeln!(w, "{}", request).expect("write request");
    let reader = BufReader::new(&stream);
    let line = reader
        .lines()
        .next()
        .expect("should get a response")
        .expect("response should be readable");
    serde_json::from_str(&line).expect("response should be valid JSON")
}

/// Parse the ledger JSONL and return all entries of the given event type.
fn ledger_events(dir: &std::path::Path, event_type: &str) -> Vec<serde_json::Value> {
    let content = std::fs::read_to_string(dir.join("output/.sahjhan/ledger.jsonl")).unwrap();
    content
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|e| e["type"] == event_type)
        .collect()
}

#[test]
#[ignore]
fn test_record_event_appends_restricted_event_to_ledger() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    let req = r#"{"op": "record_event", "event_type": "context_reset", "fields": {"run": "42", "trigger": "user_prompt_submit"}}"#;
    let resp = send_request(dir.path(), req);
    assert_eq!(resp["ok"], true, "record_event failed: {:?}", resp);

    // The event must actually land in the hash-chained ledger — a read-back
    // assertion, not an invocation count. This is the property the holtz
    // returncode-swallow hid: the op reporting success without persisting.
    let events = ledger_events(dir.path(), "context_reset");
    assert_eq!(events.len(), 1, "expected one context_reset in the ledger");
    assert_eq!(events[0]["fields"]["run"], "42");
    assert_eq!(events[0]["fields"]["trigger"], "user_prompt_submit");

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_record_event_rejects_undeclared_event() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    let req = r#"{"op": "record_event", "event_type": "not_a_real_event", "fields": {}}"#;
    let resp = send_request(dir.path(), req);
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"], "unknown_event");
    assert!(ledger_events(dir.path(), "not_a_real_event").is_empty());

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_record_event_rejects_field_pattern_violation() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // `run` must match ^\d+$ — "abc" violates the consumer-declared pattern.
    let req = r#"{"op": "record_event", "event_type": "context_reset", "fields": {"run": "abc", "trigger": "user_prompt_submit"}}"#;
    let resp = send_request(dir.path(), req);
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"], "invalid_field");
    assert!(
        ledger_events(dir.path(), "context_reset").is_empty(),
        "a field-invalid event must not be persisted"
    );

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_record_event_rejects_missing_required_field() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // `trigger` is required but omitted.
    let req = r#"{"op": "record_event", "event_type": "context_reset", "fields": {"run": "42"}}"#;
    let resp = send_request(dir.path(), req);
    assert_eq!(resp["ok"], false);
    assert_eq!(resp["error"], "invalid_field");
    assert!(ledger_events(dir.path(), "context_reset").is_empty());

    stop_daemon(&mut daemon);
}
