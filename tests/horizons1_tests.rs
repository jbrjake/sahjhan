// tests/horizons1_tests.rs
//
// Integration tests for the HORIZONS-1 mission control protocol.

use assert_cmd::Command;
use serde_json::Value;
use tempfile::tempdir;

/// Set up a temp directory with horizons1 config and initialize.
fn setup_horizons1() -> tempfile::TempDir {
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
            format!("examples/horizons1/{}", file),
            config_dir.join(file),
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

#[test]
fn test_horizons1_init_and_status() {
    let dir = setup_horizons1();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["data"]["state"], "pre_launch");
    let sets = v["data"]["sets"].as_array().unwrap();
    assert_eq!(sets.len(), 1);
    assert_eq!(sets[0]["name"], "subsystems");
    assert_eq!(sets[0]["total"], 5);
    assert_eq!(sets[0]["completed"], 0);
}

#[test]
fn test_horizons1_transition_through_phases() {
    let dir = setup_horizons1();

    // pre_launch → assembly_complete
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "transition",
            "complete_assembly",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["data"]["state"], "assembly_complete");

    // assembly_complete → testing
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin_testing"])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["data"]["state"], "testing");
}

#[test]
fn test_horizons1_gate_blocks_launch_without_subsystems() {
    let dir = setup_horizons1();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "transition",
            "complete_assembly",
        ])
        .current_dir(dir.path())
        .assert()
        .success();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin_testing"])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "--json",
            "gate",
            "check",
            "clear_for_launch",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["data"]["result"], "blocked");
    let candidates = v["data"]["candidates"].as_array().unwrap();
    assert_eq!(candidates[0]["all_passed"], false);
}

#[test]
fn test_horizons1_subsystem_completion_unblocks_launch() {
    let dir = setup_horizons1();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "transition",
            "complete_assembly",
        ])
        .current_dir(dir.path())
        .assert()
        .success();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin_testing"])
        .current_dir(dir.path())
        .assert()
        .success();

    for subsystem in &["eps", "adcs", "telecom", "propulsion", "payload"] {
        Command::cargo_bin("sahjhan")
            .unwrap()
            .args([
                "--config-dir",
                "enforcement",
                "set",
                "complete",
                "subsystems",
                subsystem,
            ])
            .current_dir(dir.path())
            .assert()
            .success();
    }

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "--json",
            "gate",
            "check",
            "clear_for_launch",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["data"]["result"], "ready");
}

#[test]
fn test_horizons1_anomaly_from_any_state() {
    let dir = setup_horizons1();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "transition",
            "declare_anomaly",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["data"]["state"], "anomaly");
}

#[test]
fn test_horizons1_log_json_after_transitions() {
    let dir = setup_horizons1();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "transition",
            "complete_assembly",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "log", "dump"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = v["data"]["entries"].as_array().unwrap();
    assert!(entries.len() >= 2);
    assert_eq!(entries.last().unwrap()["event_type"], "state_transition");
}

#[test]
fn test_horizons1_set_status_json() {
    let dir = setup_horizons1();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "transition",
            "complete_assembly",
        ])
        .current_dir(dir.path())
        .assert()
        .success();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin_testing"])
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
            "subsystems",
            "eps",
        ])
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
            "subsystems",
            "adcs",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "--json",
            "set",
            "status",
            "subsystems",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["data"]["name"], "subsystems");
    assert_eq!(v["data"]["completed"], 2);
    assert_eq!(v["data"]["total"], 5);
    let members = v["data"]["members"].as_array().unwrap();
    assert_eq!(members.len(), 5);
    assert_eq!(members[0]["name"], "eps");
    assert_eq!(members[0]["done"], true);
    assert_eq!(members[1]["name"], "adcs");
    assert_eq!(members[1]["done"], true);
    assert_eq!(members[2]["done"], false);
}
