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
        std::fs::copy(format!("examples/minimal/{}", file), config_dir.join(file)).unwrap();
    }
    // Copy templates directory
    let templates_dir = config_dir.join("templates");
    std::fs::create_dir_all(&templates_dir).unwrap();
    for file in &["status.md.tera", "history.md.tera"] {
        std::fs::copy(
            format!("examples/minimal/templates/{}", file),
            templates_dir.join(file),
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
        .stdout(predicate::str::contains("genesis"));
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
        .stdout(predicate::str::contains("genesis"));
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
fn test_render_produces_status_file() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "render"])
        .current_dir(dir.path())
        .assert()
        .success();
    assert!(dir.path().join("output/STATUS.md").exists());
}

#[test]
fn test_render_produces_history_file() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "render"])
        .current_dir(dir.path())
        .assert()
        .success();
    assert!(dir.path().join("output/HISTORY.md").exists());
}

#[test]
fn test_render_status_contains_state() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "render"])
        .current_dir(dir.path())
        .assert()
        .success();
    let content = std::fs::read_to_string(dir.path().join("output/STATUS.md")).unwrap();
    assert!(content.contains("idle") || content.contains("Idle"));
}

#[test]
fn test_render_status_contains_protocol() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "render"])
        .current_dir(dir.path())
        .assert()
        .success();
    let content = std::fs::read_to_string(dir.path().join("output/STATUS.md")).unwrap();
    assert!(content.contains("minimal"));
    assert!(content.contains("1.0.0"));
}

#[test]
fn test_render_history_contains_events() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "render"])
        .current_dir(dir.path())
        .assert()
        .success();
    let content = std::fs::read_to_string(dir.path().join("output/HISTORY.md")).unwrap();
    assert!(content.contains("Event History"));
    assert!(content.contains("genesis"));
}

#[test]
fn test_render_reports_files() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "render"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Rendered: STATUS.md"))
        .stdout(predicate::str::contains("Rendered: HISTORY.md"));
}

#[test]
fn test_transition_triggers_render() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin"])
        .current_dir(dir.path())
        .assert()
        .success();
    // on_transition render should have produced STATUS.md
    assert!(dir.path().join("output/STATUS.md").exists());
    let content = std::fs::read_to_string(dir.path().join("output/STATUS.md")).unwrap();
    assert!(content.contains("working") || content.contains("Working"));
}

#[test]
fn test_set_complete_triggers_event_render() {
    let dir = setup_initialized_dir();
    // Transition to working state first
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Record a set_member_complete — should trigger on_event render for HISTORY.md
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

    assert!(dir.path().join("output/HISTORY.md").exists());
    let content = std::fs::read_to_string(dir.path().join("output/HISTORY.md")).unwrap();
    assert!(content.contains("set_member_complete"));
}

#[test]
fn test_render_tracked_in_manifest() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "render"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Manifest should now track the rendered files
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "manifest", "list"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("STATUS.md"))
        .stdout(predicate::str::contains("render"));
}

#[test]
fn test_hook_generate() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "hook", "generate"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("write_guard.py"))
        .stdout(predicate::str::contains("bash_guard.py"))
        .stdout(predicate::str::contains("_sahjhan_bootstrap.py"))
        .stdout(predicate::str::contains("hooks.json"));
}

#[test]
fn test_hook_generate_with_output_dir() {
    let dir = setup_initialized_dir();
    let hooks_dir = dir.path().join(".hooks");
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "hook",
            "generate",
            "--output-dir",
            hooks_dir.to_str().unwrap(),
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Generated 3 hook scripts"));

    // Verify files were created
    assert!(hooks_dir.join("write_guard.py").exists());
    assert!(hooks_dir.join("bash_guard.py").exists());
    assert!(hooks_dir.join("_sahjhan_bootstrap.py").exists());
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

// ---------------------------------------------------------------------------
// render --dump-context (issue #4)
// ---------------------------------------------------------------------------

#[test]
fn test_render_dump_context_outputs_json() {
    let dir = setup_initialized_dir();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "render", "--dump-context"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value =
        serde_json::from_slice(&output).expect("--dump-context output should be valid JSON");

    // Verify all documented context variables are present
    assert!(json.get("protocol").is_some(), "missing 'protocol'");
    assert!(json.get("state").is_some(), "missing 'state'");
    assert!(json.get("events").is_some(), "missing 'events'");
    assert!(json.get("sets").is_some(), "missing 'sets'");
    assert!(json.get("ledger_len").is_some(), "missing 'ledger_len'");
    assert!(json.get("violations").is_some(), "missing 'violations'");

    // Verify protocol sub-fields
    let protocol = json.get("protocol").unwrap();
    assert!(protocol.get("name").is_some(), "missing 'protocol.name'");
    assert!(
        protocol.get("version").is_some(),
        "missing 'protocol.version'"
    );

    // Verify state sub-fields
    let state = json.get("state").unwrap();
    assert!(state.get("name").is_some(), "missing 'state.name'");
    assert!(state.get("label").is_some(), "missing 'state.label'");
}

#[test]
fn test_render_dump_context_reflects_state_changes() {
    let dir = setup_initialized_dir();

    // Transition to working state
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin"])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "render", "--dump-context"])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value = serde_json::from_slice(&output).unwrap();

    // State should reflect the transition
    assert_eq!(json["state"]["name"].as_str().unwrap(), "working");
    assert_eq!(json["state"]["label"].as_str().unwrap(), "Working");

    // Events should include the genesis + transition
    let events = json["events"].as_array().unwrap();
    assert!(events.len() >= 2);

    // ledger_len should match
    assert_eq!(json["ledger_len"].as_u64().unwrap(), events.len() as u64);
}

// ---------------------------------------------------------------------------
// validate command tests
// ---------------------------------------------------------------------------

/// Helper: set up a config directory (without initializing a run) from the
/// minimal example.  Returns the tempdir so it stays alive.
fn setup_config_only_dir() -> tempfile::TempDir {
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
    // Copy templates directory
    let templates_dir = config_dir.join("templates");
    std::fs::create_dir_all(&templates_dir).unwrap();
    for file in &["status.md.tera", "history.md.tera"] {
        std::fs::copy(
            format!("examples/minimal/templates/{}", file),
            templates_dir.join(file),
        )
        .unwrap();
    }
    dir
}

#[test]
fn test_validate_clean_config() {
    let dir = setup_config_only_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "validate"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Config valid"));
}

#[test]
fn test_validate_catches_bad_gate_type() {
    let dir = setup_config_only_dir();
    let config_dir = dir.path().join("enforcement");

    // Overwrite transitions.toml with an invalid gate type
    std::fs::write(
        config_dir.join("transitions.toml"),
        r#"
[[transitions]]
from = "idle"
to = "working"
command = "begin"
gates = [
    { type = "nonexistent_gate", foo = "bar" },
]
"#,
    )
    .unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "validate"])
        .current_dir(dir.path())
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "unknown gate type 'nonexistent_gate'",
        ));
}

#[test]
fn test_validate_catches_missing_gate_param() {
    let dir = setup_config_only_dir();
    let config_dir = dir.path().join("enforcement");

    // file_exists gate without required "path" parameter
    std::fs::write(
        config_dir.join("transitions.toml"),
        r#"
[[transitions]]
from = "idle"
to = "working"
command = "begin"
gates = [
    { type = "file_exists" },
]
"#,
    )
    .unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "validate"])
        .current_dir(dir.path())
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "missing required parameter 'path'",
        ));
}

#[test]
fn test_validate_catches_missing_template() {
    let dir = setup_config_only_dir();
    let config_dir = dir.path().join("enforcement");

    // Overwrite renders.toml to point to a nonexistent template
    std::fs::write(
        config_dir.join("renders.toml"),
        r#"
[[renders]]
target = "STATUS.md"
template = "templates/nonexistent.tera"
trigger = "on_transition"
"#,
    )
    .unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "validate"])
        .current_dir(dir.path())
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "template 'templates/nonexistent.tera' does not exist",
        ));
}

#[test]
fn test_validate_catches_bad_alias_target() {
    let dir = setup_config_only_dir();
    let config_dir = dir.path().join("enforcement");

    // Overwrite protocol.toml with an alias pointing to a non-existent transition
    std::fs::write(
        config_dir.join("protocol.toml"),
        r#"
[protocol]
name = "minimal"
version = "1.0.0"
description = "Minimal example protocol"

[paths]
managed = ["output"]
data_dir = "output/.sahjhan"
render_dir = "output"

[sets.check]
description = "Verification checks"
values = ["tests", "lint"]

[aliases]
"start" = "transition begin"
"bogus" = "transition does_not_exist"
"#,
    )
    .unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "validate"])
        .current_dir(dir.path())
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "alias 'bogus' targets transition 'does_not_exist' which is not defined",
        ));
}

#[test]
fn test_validate_catches_bad_render_event_type() {
    let dir = setup_config_only_dir();
    let config_dir = dir.path().join("enforcement");

    // Overwrite renders.toml with a reference to a non-existent event type
    std::fs::write(
        config_dir.join("renders.toml"),
        r#"
[[renders]]
target = "STATUS.md"
template = "templates/status.md.tera"
trigger = "on_transition"

[[renders]]
target = "HISTORY.md"
template = "templates/history.md.tera"
trigger = "on_event"
event_types = ["totally_fake_event"]
"#,
    )
    .unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "validate"])
        .current_dir(dir.path())
        .assert()
        .code(3)
        .stderr(predicate::str::contains(
            "undefined event type 'totally_fake_event'",
        ));
}

#[test]
fn test_validate_warns_terminal_state_outgoing() {
    let dir = setup_config_only_dir();
    let config_dir = dir.path().join("enforcement");

    // Add a transition from the terminal state "done"
    std::fs::write(
        config_dir.join("transitions.toml"),
        r#"
[[transitions]]
from = "idle"
to = "working"
command = "begin"
gates = []

[[transitions]]
from = "working"
to = "done"
command = "complete"
gates = [
    { type = "set_covered", set = "check", event = "set_member_complete", field = "member" },
]

[[transitions]]
from = "done"
to = "idle"
command = "restart"
gates = []
"#,
    )
    .unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "validate"])
        .current_dir(dir.path())
        .assert()
        .success() // warnings don't cause failure
        .stderr(predicate::str::contains(
            "terminal state 'done' has outgoing transition",
        ));
}

#[test]
fn test_validate_warns_unreachable_state() {
    let dir = setup_config_only_dir();
    let config_dir = dir.path().join("enforcement");

    // Add an orphan state that nothing transitions to
    std::fs::write(
        config_dir.join("states.toml"),
        r#"
[states.idle]
label = "Idle"
initial = true

[states.working]
label = "Working"

[states.done]
label = "Done"
terminal = true

[states.orphan]
label = "Orphan"
"#,
    )
    .unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "validate"])
        .current_dir(dir.path())
        .assert()
        .success() // warnings don't cause failure
        .stderr(predicate::str::contains("state 'orphan' is unreachable"));
}
