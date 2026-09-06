//! E2E tests for the `record_event` socket op: authenticated ledger append
//! for a trusted peer (the ledger-write analog of `enforcement_write`).
//!
//! These require a live daemon, so they are `#[ignore]` by default and run
//! explicitly (same pattern as daemon_enforcement_tests.rs). Auth is skipped
//! because no trusted-callers.toml exists (caller auth unconfigured) — the
//! record path itself is what's under test, not peer authentication (covered
//! in daemon_auth_tests.rs / daemon_signing_tests.rs).

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

# Agent-writable, and carrying a stamped field the engine owns (#50): the
# daemon must record *which peer* it authenticated, not what the peer claims.
[events.finding_deferred]
description = "A finding set aside"
fields = [
    { name = "id", type = "string" },
    { name = "reason", type = "string" },
    { name = "recorded_by", type = "string", stamped = true },
]
"#,
    )
    .unwrap();

    // No trusted-callers.toml — caller auth unconfigured; the daemon accepts
    // test connections (a present manifest is enforced; an empty one denies).

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

// ---------------------------------------------------------------------------
// Provenance stamping on the record path (sahjhan #50)
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_record_event_stamps_daemon_unverified_when_auth_is_unconfigured() {
    // No trusted-callers.toml, so the daemon serves every peer and can name
    // none of them. It says exactly that rather than implying it verified
    // something — a gate filtering on this value is filtering on "anyone".
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    let req = r#"{"op": "record_event", "event_type": "finding_deferred", "fields": {"id": "BH-001", "reason": "theoretical"}}"#;
    let resp = send_request(dir.path(), req);
    assert_eq!(resp["ok"], true, "record_event failed: {:?}", resp);

    let events = ledger_events(dir.path(), "finding_deferred");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0]["fields"]["recorded_by"], "daemon:unverified");

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_record_event_refuses_a_peer_supplied_stamp() {
    let dir = setup_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    let req = r#"{"op": "record_event", "event_type": "finding_deferred", "fields": {"id": "BH-001", "reason": "theoretical", "recorded_by": "hook:hooks/courier.py"}}"#;
    let resp = send_request(dir.path(), req);
    assert_eq!(resp["ok"], false, "{:?}", resp);
    assert_eq!(resp["error"], "invalid_field");
    assert!(
        ledger_events(dir.path(), "finding_deferred").is_empty(),
        "a forged provenance must not be persisted, stamped over, or otherwise \
         quietly accepted"
    );

    stop_daemon(&mut daemon);
}

/// The issue's arrangement: a courier script that speaks the socket itself,
/// listed and hashed in trusted-callers.toml, recording the event whose
/// provenance a gate keys on.
fn setup_dir_with_courier() -> tempfile::TempDir {
    let dir = setup_dir();
    let config_dir = dir.path().join("enforcement");
    let hooks_dir = config_dir.join("hooks");
    std::fs::create_dir_all(&hooks_dir).unwrap();

    let courier = r#"#!/usr/bin/env python3
import json, socket, sys
s = socket.socket(socket.AF_UNIX)
s.connect(sys.argv[1])
req = {"op": "record_event", "event_type": "finding_deferred",
       "fields": {"id": "BH-001", "reason": "theoretical"}}
s.sendall((json.dumps(req) + "\n").encode())
resp = s.makefile().readline()
print(resp, end="")
sys.exit(0 if json.loads(resp).get("ok") else 1)
"#;
    std::fs::write(hooks_dir.join("theoretical_courier.py"), courier).unwrap();

    use sha2::{Digest, Sha256};
    let hash = format!(
        "sha256:{}",
        hex::encode(Sha256::digest(
            std::fs::read(hooks_dir.join("theoretical_courier.py")).unwrap()
        ))
    );
    std::fs::write(
        config_dir.join("trusted-callers.toml"),
        format!(
            "[callers]\n\"hooks/theoretical_courier.py\" = \"{}\"\n",
            hash
        ),
    )
    .unwrap();

    // trusted-callers.toml is sealed config, and it did not exist when `init`
    // ran — re-init so the genesis seal covers the file as written.
    std::fs::remove_dir_all(dir.path().join("output")).unwrap();
    std::fs::create_dir_all(dir.path().join("output")).unwrap();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "init"])
        .current_dir(dir.path())
        .assert()
        .success();

    dir
}

#[test]
#[ignore]
fn test_record_event_stamps_the_authenticated_script_path() {
    // The half of #50 that no existing mechanism reached: the courier's
    // deferral and an agent-typed one no longer write a byte-identical row.
    let dir = setup_dir_with_courier();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    let output = std::process::Command::new("python3")
        .arg(dir.path().join("enforcement/hooks/theoretical_courier.py"))
        .arg(dir.path().join("output/.sahjhan/daemon.sock"))
        .output()
        .expect("courier should run");
    let stdout = String::from_utf8_lossy(&output.stdout).to_string();

    stop_daemon(&mut daemon);

    assert!(
        output.status.success(),
        "the courier is listed and hashed, so it authenticates: {}",
        stdout
    );
    let events = ledger_events(dir.path(), "finding_deferred");
    assert_eq!(events.len(), 1, "{}", stdout);
    assert_eq!(
        events[0]["fields"]["recorded_by"], "hook:hooks/theoretical_courier.py",
        "the identity the daemon verified is the identity it records"
    );
}
