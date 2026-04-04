// tests/auth_tests.rs
//
// Tests for restricted events, HMAC authentication (daemon-based),
// and the ledger_lacks_event gate.

use assert_cmd::Command;
use predicates::prelude::*;
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

// ---------------------------------------------------------------------------
// Daemon helpers
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Restricted event tests (no daemon needed)
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Daemon-based authed-event tests
// ---------------------------------------------------------------------------

#[test]
#[ignore]
fn test_authed_event_valid_proof() {
    let dir = setup_auth_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

    // Use `sahjhan sign` to get a valid proof from the daemon.
    let sign_output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "sign",
            "--event-type",
            "quiz_answered",
            "--field",
            "score=5/5",
            "--field",
            "pass=true",
        ])
        .current_dir(dir.path())
        .output()
        .expect("sign command failed");
    assert!(sign_output.status.success(), "sign should succeed");
    let proof = String::from_utf8_lossy(&sign_output.stdout)
        .trim()
        .to_string();
    assert!(!proof.is_empty(), "proof should not be empty");

    // Use the proof with authed-event.
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

    stop_daemon(&mut daemon);
}

#[test]
#[ignore]
fn test_authed_event_invalid_proof() {
    let dir = setup_auth_dir();
    let mut daemon = start_daemon(dir.path());
    wait_for_socket(dir.path());

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
        .stderr(
            predicate::str::contains("invalid proof")
                .or(predicate::str::contains("proof does not match")),
        );

    stop_daemon(&mut daemon);
}
