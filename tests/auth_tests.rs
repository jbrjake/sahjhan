// tests/auth_tests.rs
//
// Tests for restricted events, HMAC authentication, session keys,
// guards, the ledger_lacks_event gate, and config session-key-path.

use assert_cmd::Command;
use hmac::{Hmac, Mac};
use predicates::prelude::*;
use sha2::Sha256;
use tempfile::tempdir;

/// Create a temp directory with config that includes restricted events, then run `init`.
fn setup_auth_dir() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        r#"
[protocol]
name = "test-auth"
version = "1.0.0"
description = "Auth test protocol"

[paths]
managed = ["output"]
data_dir = "output/.sahjhan"
render_dir = "output"

[guards]
read_blocked = ["enforcement/quiz-bank.json"]
"#,
    )
    .unwrap();

    std::fs::write(
        config_dir.join("states.toml"),
        r#"
[states.idle]
label = "Idle"
initial = true
"#,
    )
    .unwrap();

    std::fs::write(
        config_dir.join("transitions.toml"),
        "[[transitions]]\nfrom = \"idle\"\nto = \"idle\"\ncommand = \"noop\"\ngates = []\n",
    )
    .unwrap();

    std::fs::write(
        config_dir.join("events.toml"),
        r#"
[events.quiz_answered]
description = "Quiz result"
restricted = true
fields = [
    { name = "score", type = "string" },
    { name = "pass", type = "string" },
]

[events.finding]
description = "A finding"
fields = [
    { name = "detail", type = "string" },
]
"#,
    )
    .unwrap();

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
fn test_init_creates_session_key() {
    let dir = setup_auth_dir();
    let key_path = dir.path().join("output/.sahjhan/session.key");
    assert!(key_path.exists(), "session.key should exist after init");
    let key_bytes = std::fs::read(&key_path).unwrap();
    assert_eq!(key_bytes.len(), 32, "session key should be 32 bytes");
}

#[test]
fn test_ledger_create_generates_per_ledger_key() {
    let dir = setup_auth_dir();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "ledger",
            "create",
            "--name",
            "test-ledger",
            "--path",
            "output/.sahjhan/test-ledger.jsonl",
            "--mode",
            "event-only",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    let key_path = dir
        .path()
        .join("output/.sahjhan/ledgers/test-ledger/session.key");
    assert!(
        key_path.exists(),
        "per-ledger session.key should exist after ledger create"
    );
    let key_bytes = std::fs::read(&key_path).unwrap();
    assert_eq!(key_bytes.len(), 32, "per-ledger session key should be 32 bytes");

    // Per-ledger key should differ from global key
    let global_key = std::fs::read(dir.path().join("output/.sahjhan/session.key")).unwrap();
    assert_ne!(key_bytes, global_key.as_slice(), "per-ledger key should differ from global");
}

#[test]
fn test_event_rejects_restricted_type() {
    let dir = setup_auth_dir();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "event",
            "quiz_answered",
            "--field",
            "score=5/5",
            "--field",
            "pass=true",
        ])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("restricted"))
        .stderr(predicate::str::contains("authed-event"));
}

#[test]
fn test_event_allows_unrestricted_type() {
    let dir = setup_auth_dir();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "event",
            "finding",
            "--field",
            "detail=something",
        ])
        .current_dir(dir.path())
        .assert()
        .success();
}

#[test]
fn test_config_session_key_path_global() {
    let dir = setup_auth_dir();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "config", "session-key-path"])
        .current_dir(dir.path())
        .assert()
        .success();

    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.trim().ends_with("session.key"),
        "should output path ending in session.key, got: {}",
        stdout.trim()
    );
    assert!(
        stdout.trim().starts_with('/'),
        "should be absolute path, got: {}",
        stdout.trim()
    );
}

#[test]
fn test_config_session_key_path_per_ledger() {
    let dir = setup_auth_dir();

    // Create a named ledger so it gets a per-ledger key
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "ledger",
            "create",
            "--name",
            "myledger",
            "--path",
            "output/.sahjhan/myledger.jsonl",
            "--mode",
            "event-only",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "--ledger",
            "myledger",
            "config",
            "session-key-path",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.trim().contains("ledgers/myledger/session.key"),
        "should output per-ledger key path, got: {}",
        stdout.trim()
    );
}

#[test]
fn test_guards_returns_json_with_auto_included_key() {
    let dir = setup_auth_dir();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "guards"])
        .current_dir(dir.path())
        .assert()
        .success();

    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).expect("should be valid JSON");

    let read_blocked = parsed["read_blocked"]
        .as_array()
        .expect("read_blocked should be an array");

    let paths: Vec<&str> = read_blocked.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(
        paths.contains(&"enforcement/quiz-bank.json"),
        "should contain configured path, got: {:?}",
        paths
    );
    assert!(
        paths.iter().any(|p| p.contains("session.key")),
        "should auto-include session.key, got: {:?}",
        paths
    );
}

#[test]
fn test_guards_without_config_section() {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        r#"
[protocol]
name = "test"
version = "1.0.0"
description = "test"

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

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "guards"])
        .current_dir(dir.path())
        .assert()
        .success();

    let stdout = String::from_utf8(output.get_output().stdout.clone()).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(stdout.trim()).expect("should be valid JSON");

    let read_blocked = parsed["read_blocked"]
        .as_array()
        .expect("read_blocked should be an array");

    let paths: Vec<&str> = read_blocked.iter().map(|v| v.as_str().unwrap()).collect();
    assert!(
        paths.iter().any(|p| p.contains("session.key")),
        "should auto-include session.key even without [guards] section, got: {:?}",
        paths
    );
}

fn compute_proof(key_path: &std::path::Path, event_type: &str, fields: &[(&str, &str)]) -> String {
    let key = std::fs::read(key_path).unwrap();
    let mut sorted_fields: Vec<(&str, &str)> = fields.to_vec();
    sorted_fields.sort_by_key(|(k, _)| *k);

    let mut payload = event_type.to_string();
    for (k, v) in &sorted_fields {
        payload.push('\0');
        payload.push_str(&format!("{}={}", k, v));
    }

    let mut mac = Hmac::<Sha256>::new_from_slice(&key).unwrap();
    mac.update(payload.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

#[test]
fn test_authed_event_valid_proof() {
    let dir = setup_auth_dir();
    let key_path = dir.path().join("output/.sahjhan/session.key");

    let proof = compute_proof(
        &key_path,
        "quiz_answered",
        &[("score", "5/5"), ("pass", "true")],
    );

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "authed-event",
            "quiz_answered",
            "--field",
            "score=5/5",
            "--field",
            "pass=true",
            "--proof",
            &proof,
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("recorded: quiz_answered"));
}

#[test]
fn test_authed_event_invalid_proof() {
    let dir = setup_auth_dir();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "authed-event",
            "quiz_answered",
            "--field",
            "score=5/5",
            "--field",
            "pass=true",
            "--proof",
            "deadbeef",
        ])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid proof"));
}

#[test]
fn test_authed_event_rejects_unrestricted_type() {
    let dir = setup_auth_dir();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "authed-event",
            "finding",
            "--field",
            "detail=something",
            "--proof",
            "deadbeef",
        ])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("not restricted"));
}
