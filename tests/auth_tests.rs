// tests/auth_tests.rs
//
// Tests for restricted events, HMAC authentication, session keys,
// guards, and the ledger_lacks_event gate.

use assert_cmd::Command;
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
