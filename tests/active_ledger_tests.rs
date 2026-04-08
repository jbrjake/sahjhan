// tests/active_ledger_tests.rs
//
// Tests for the active-ledger marker feature (#25).

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
        std::fs::copy(format!("examples/minimal/{}", file), config_dir.join(file)).unwrap();
    }
    let templates_dir = config_dir.join("templates");
    std::fs::create_dir_all(&templates_dir).unwrap();
    for file in &["status.md.tera", "history.md.tera"] {
        std::fs::copy(
            format!("examples/minimal/templates/{}", file),
            templates_dir.join(file),
        )
        .unwrap();
    }
    std::fs::create_dir_all(dir.path().join("output")).unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "init"])
        .current_dir(dir.path())
        .assert()
        .success();

    dir
}

/// Create a second named ledger via direct creation.
fn create_named_ledger(dir: &std::path::Path, name: &str) {
    let data_dir = dir.join("output/.sahjhan");
    let ledger_path = data_dir.join(format!("{}.jsonl", name));
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "ledger",
            "create",
            "--name",
            name,
            "--path",
            ledger_path.to_str().unwrap(),
        ])
        .current_dir(dir)
        .assert()
        .success();
}

// ---------------------------------------------------------------------------
// ledger activate
// ---------------------------------------------------------------------------

#[test]
fn test_activate_valid_ledger() {
    let dir = setup_initialized_dir();
    create_named_ledger(dir.path(), "run-1");

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "ledger", "activate", "run-1"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Activated ledger: run-1"));

    // Verify the marker file was written
    let marker = dir.path().join("output/.sahjhan/active-ledger");
    assert!(marker.exists());
    let content = std::fs::read_to_string(&marker).unwrap();
    assert_eq!(content.trim(), "run-1");
}

#[test]
fn test_activate_unregistered_ledger_fails() {
    let dir = setup_initialized_dir();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "ledger",
            "activate",
            "nonexistent",
        ])
        .current_dir(dir.path())
        .assert()
        .failure();

    // Marker should NOT have been written
    let marker = dir.path().join("output/.sahjhan/active-ledger");
    assert!(!marker.exists());
}

#[test]
fn test_activate_json_output() {
    let dir = setup_initialized_dir();
    create_named_ledger(dir.path(), "run-1");

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "--json",
            "ledger",
            "activate",
            "run-1",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["data"]["activated"], "run-1");
}

// ---------------------------------------------------------------------------
// ledger deactivate
// ---------------------------------------------------------------------------

#[test]
fn test_deactivate_removes_marker() {
    let dir = setup_initialized_dir();
    create_named_ledger(dir.path(), "run-1");

    // Activate first
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "ledger", "activate", "run-1"])
        .current_dir(dir.path())
        .assert()
        .success();

    let marker = dir.path().join("output/.sahjhan/active-ledger");
    assert!(marker.exists());

    // Now deactivate
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "ledger", "deactivate"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Deactivated active ledger"));

    assert!(!marker.exists());
}

#[test]
fn test_deactivate_noop_when_no_marker() {
    let dir = setup_initialized_dir();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "ledger", "deactivate"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No active ledger to deactivate"));
}

#[test]
fn test_deactivate_json_output() {
    let dir = setup_initialized_dir();
    create_named_ledger(dir.path(), "run-1");

    // Activate then deactivate
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "ledger", "activate", "run-1"])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "--json",
            "ledger",
            "deactivate",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["data"]["deactivated"], true);

    // Deactivate again — should report false
    let output2 = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "--json",
            "ledger",
            "deactivate",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let stdout2 = String::from_utf8(output2.stdout).unwrap();
    let v2: serde_json::Value = serde_json::from_str(&stdout2).unwrap();
    assert_eq!(v2["data"]["deactivated"], false);
}

// ---------------------------------------------------------------------------
// ledger create --activate
// ---------------------------------------------------------------------------

#[test]
fn test_create_with_activate_flag() {
    let dir = setup_initialized_dir();
    let data_dir = dir.path().join("output/.sahjhan");
    let ledger_path = data_dir.join("run-5.jsonl");

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "ledger",
            "create",
            "--name",
            "run-5",
            "--path",
            ledger_path.to_str().unwrap(),
            "--activate",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("created: run-5"))
        .stdout(predicate::str::contains("Activated ledger: run-5"));

    // Verify the marker file was written
    let marker = data_dir.join("active-ledger");
    assert!(marker.exists());
    let content = std::fs::read_to_string(&marker).unwrap();
    assert_eq!(content.trim(), "run-5");
}

// ---------------------------------------------------------------------------
// Resolution priority
// ---------------------------------------------------------------------------

#[test]
fn test_explicit_ledger_flag_beats_active_marker() {
    let dir = setup_initialized_dir();
    create_named_ledger(dir.path(), "run-1");

    // Activate run-1
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "ledger", "activate", "run-1"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Record an event to the default ledger explicitly
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "--ledger",
            "default",
            "event",
            "note",
            "--field",
            "message=explicit-target",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    // Verify status with --ledger default shows explicit source
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "--json",
            "--ledger",
            "default",
            "status",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["data"]["ledger_source"], "explicit --ledger flag");
    assert_eq!(v["data"]["ledger_name"], "default");
}

#[test]
fn test_active_marker_used_when_no_explicit_flag() {
    let dir = setup_initialized_dir();
    create_named_ledger(dir.path(), "run-1");

    // Activate run-1
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "ledger", "activate", "run-1"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Status without --ledger should show active-ledger marker
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["data"]["ledger_source"], "active-ledger marker");
    assert_eq!(v["data"]["ledger_name"], "run-1");
}

#[test]
fn test_no_marker_falls_back_to_default() {
    let dir = setup_initialized_dir();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8(output.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["data"]["ledger_source"], "no active-ledger marker");
    assert_eq!(v["data"]["ledger_name"], "default");
}

// ---------------------------------------------------------------------------
// Stale marker
// ---------------------------------------------------------------------------

#[test]
fn test_stale_marker_warns_and_falls_back() {
    let dir = setup_initialized_dir();

    // Write a marker pointing to a non-existent ledger
    let marker = dir.path().join("output/.sahjhan/active-ledger");
    std::fs::write(&marker, "deleted-ledger\n").unwrap();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    // Should succeed (fell back to default)
    assert!(output.status.success());

    // Should have warned on stderr
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("active-ledger 'deleted-ledger' is not registered"),
        "stderr was: {}",
        stderr
    );

    // Status text should show default
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Ledger: default"), "stdout was: {}", stdout);
}

// ---------------------------------------------------------------------------
// reset clears marker
// ---------------------------------------------------------------------------

#[test]
fn test_reset_removes_active_ledger_marker() {
    let dir = setup_initialized_dir();
    create_named_ledger(dir.path(), "run-1");

    // Activate
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "ledger", "activate", "run-1"])
        .current_dir(dir.path())
        .assert()
        .success();

    let marker = dir.path().join("output/.sahjhan/active-ledger");
    assert!(marker.exists());

    // Get the reset token
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "reset", "--confirm"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Extract token from "reset requires --token XXXXXX"
    let token = stdout.trim().split_whitespace().last().unwrap().to_string();

    // Run actual reset
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "reset",
            "--confirm",
            "--token",
            &token,
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    // Marker should be gone
    assert!(
        !marker.exists(),
        "active-ledger marker should be removed by reset"
    );
}

// ---------------------------------------------------------------------------
// Status display
// ---------------------------------------------------------------------------

#[test]
fn test_status_text_shows_ledger_source() {
    let dir = setup_initialized_dir();

    // Without marker
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Ledger: default (no active-ledger marker)"),
        "stdout was: {}",
        stdout
    );

    // With marker
    create_named_ledger(dir.path(), "run-1");
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "ledger", "activate", "run-1"])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Ledger: run-1 (active-ledger marker)"),
        "stdout was: {}",
        stdout
    );
}

// ---------------------------------------------------------------------------
// Events land in active ledger
// ---------------------------------------------------------------------------

#[test]
fn test_events_land_in_active_ledger() {
    let dir = setup_initialized_dir();
    create_named_ledger(dir.path(), "run-1");

    // Activate run-1
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "ledger", "activate", "run-1"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Record an event (no --ledger flag — should use active marker)
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "event",
            "note",
            "--field",
            "message=active-test",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    // Verify event is in run-1 by querying that ledger
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "--ledger",
            "run-1",
            "log",
            "tail",
            "1",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("note") && stdout.contains("active-test"),
        "Event should be in run-1 ledger. stdout: {}",
        stdout
    );
}

// ---------------------------------------------------------------------------
// Activate overwrites prior marker
// ---------------------------------------------------------------------------

#[test]
fn test_activate_overwrites_prior_marker() {
    let dir = setup_initialized_dir();
    create_named_ledger(dir.path(), "run-1");
    create_named_ledger(dir.path(), "run-2");

    // Activate run-1
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "ledger", "activate", "run-1"])
        .current_dir(dir.path())
        .assert()
        .success();

    let marker = dir.path().join("output/.sahjhan/active-ledger");
    assert_eq!(std::fs::read_to_string(&marker).unwrap().trim(), "run-1");

    // Activate run-2 — overwrites
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "ledger", "activate", "run-2"])
        .current_dir(dir.path())
        .assert()
        .success();

    assert_eq!(std::fs::read_to_string(&marker).unwrap().trim(), "run-2");
}
