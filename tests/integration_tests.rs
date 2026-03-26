// tests/integration_tests.rs
//
// End-to-end integration tests for the sahjhan CLI.

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

/// Create a temp directory with the minimal example config and run `init`.
fn setup_initialized_dir() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();
    for file in &[
        "protocol.toml",
        "states.toml",
        "transitions.toml",
        "events.toml",
        "renders.toml",
    ] {
        std::fs::copy(
            format!("examples/minimal/{}", file),
            config_dir.join(file),
        )
        .unwrap();
    }
    // Also create the output directory
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
fn test_init_creates_ledger() {
    let dir = setup_initialized_dir();
    assert!(dir.path().join("output/.sahjhan/ledger.bin").exists());
    assert!(dir.path().join("output/.sahjhan/manifest.json").exists());
}

#[test]
fn test_status_shows_current_state() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("State:"))
        .stdout(predicate::str::contains("Idle"));
}

#[test]
fn test_transition_advances_state() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin"])
        .current_dir(dir.path())
        .assert()
        .success();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .assert()
        .stdout(predicate::str::contains("Working"));
}

#[test]
fn test_log_verify_clean() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "log", "verify"])
        .current_dir(dir.path())
        .assert()
        .success();
}

#[test]
fn test_manifest_verify_clean() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "manifest", "verify"])
        .current_dir(dir.path())
        .assert()
        .success();
}

#[test]
fn test_event_recording() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin"])
        .current_dir(dir.path())
        .assert()
        .success();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "event",
            "set_member_complete",
            "--field",
            "set=check",
            "--field",
            "member=tests",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    // Check set status shows partial completion
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "set", "status", "check"])
        .current_dir(dir.path())
        .assert()
        .stdout(predicate::str::contains("tests"))
        .stdout(predicate::str::contains("1/2"));
}

#[test]
fn test_full_workflow() {
    let dir = setup_initialized_dir();

    // begin
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin"])
        .current_dir(dir.path())
        .assert()
        .success();

    // complete both set members
    for member in &["tests", "lint"] {
        Command::cargo_bin("sahjhan")
            .unwrap()
            .args([
                "--config-dir",
                "enforcement",
                "set",
                "complete",
                "check",
                member,
            ])
            .current_dir(dir.path())
            .assert()
            .success();
    }

    // complete transition (gate should now pass)
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "complete"])
        .current_dir(dir.path())
        .assert()
        .success();

    // status should show Done
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .assert()
        .stdout(predicate::str::contains("Done"));
}

#[test]
fn test_gate_check_dry_run() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Check gates for "complete" — should show failures
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "gate", "check", "complete"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\u{2717}")); // ✗
}

#[test]
fn test_alias_resolution() {
    let dir = setup_initialized_dir();
    // "start" is aliased to "transition begin"
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "start"])
        .current_dir(dir.path())
        .assert()
        .success();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .assert()
        .stdout(predicate::str::contains("Working"));
}

#[test]
fn test_log_dump() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "log", "dump"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("protocol_init"));
}

#[test]
fn test_log_tail() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "log", "tail", "1"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("protocol_init"));
}

#[test]
fn test_init_prevents_double_init() {
    let dir = setup_initialized_dir();
    // Second init should fail
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "init"])
        .current_dir(dir.path())
        .assert()
        .code(4); // EXIT_USAGE_ERROR
}

#[test]
fn test_transition_blocked_by_gate() {
    let dir = setup_initialized_dir();

    // begin first
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Try to complete without finishing set members — should fail
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "complete"])
        .current_dir(dir.path())
        .assert()
        .code(1); // EXIT_GATE_FAILED
}

#[test]
fn test_manifest_list() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "manifest", "list"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("ledger.bin"));
}

#[test]
fn test_set_status() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "set", "status", "check"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("0/2"))
        .stdout(predicate::str::contains("tests"))
        .stdout(predicate::str::contains("lint"));
}

#[test]
fn test_set_complete_unknown_set() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "set",
            "complete",
            "nonexistent",
            "foo",
        ])
        .current_dir(dir.path())
        .assert()
        .code(4); // EXIT_USAGE_ERROR
}

#[test]
fn test_set_complete_unknown_member() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "set",
            "complete",
            "check",
            "nonexistent",
        ])
        .current_dir(dir.path())
        .assert()
        .code(4); // EXIT_USAGE_ERROR
}

#[test]
fn test_invalid_transition_from_state() {
    let dir = setup_initialized_dir();
    // Try to "complete" from idle state — no such transition
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "complete"])
        .current_dir(dir.path())
        .assert()
        .code(4); // EXIT_USAGE_ERROR
}

#[test]
fn test_render_stub() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "render"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("not yet implemented"));
}

#[test]
fn test_hook_generate_stub() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "hook", "generate"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("not yet implemented"));
}

#[test]
fn test_log_verify_after_operations() {
    let dir = setup_initialized_dir();

    // Do some operations
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin"])
        .current_dir(dir.path())
        .assert()
        .success();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "set",
            "complete",
            "check",
            "tests",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    // Verify should still pass
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "log", "verify"])
        .current_dir(dir.path())
        .assert()
        .success();
}

#[test]
fn test_status_shows_set_progress() {
    let dir = setup_initialized_dir();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin"])
        .current_dir(dir.path())
        .assert()
        .success();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "set",
            "complete",
            "check",
            "tests",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("1/2"));
}

#[test]
fn test_finish_alias() {
    let dir = setup_initialized_dir();

    // begin
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "start"])
        .current_dir(dir.path())
        .assert()
        .success();

    // complete set members
    for member in &["tests", "lint"] {
        Command::cargo_bin("sahjhan")
            .unwrap()
            .args([
                "--config-dir",
                "enforcement",
                "set",
                "complete",
                "check",
                member,
            ])
            .current_dir(dir.path())
            .assert()
            .success();
    }

    // "finish" alias -> "transition complete"
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "finish"])
        .current_dir(dir.path())
        .assert()
        .success();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .assert()
        .stdout(predicate::str::contains("Done"));
}
