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
    assert!(dir.path().join("output/.sahjhan/ledger.jsonl").exists());
    assert!(dir.path().join("output/.sahjhan/manifest.json").exists());
    // Registry should also be created by init
    assert!(dir.path().join("output/.sahjhan/ledgers.toml").exists());
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
        .stdout(predicate::str::contains("state:"))
        .stdout(predicate::str::contains("idle"));
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
        .stdout(predicate::str::contains("working"));
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
fn test_event_missing_required_field_rejected() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin"])
        .current_dir(dir.path())
        .assert()
        .success();

    // set_member_complete requires "set" and "member" fields — omit "member"
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "event",
            "set_member_complete",
            "--field",
            "set=check",
        ])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("missing field 'member'"));
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

    // status should show done
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .assert()
        .stdout(predicate::str::contains("done"));
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
        .stdout(predicate::str::contains("working"));
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
        .stdout(predicate::str::contains("ledger.jsonl"));
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
        .stdout(predicate::str::contains("rendered:"));
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
fn test_status_terse_format() {
    let dir = setup_initialized_dir();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.starts_with("state:"), "stdout was: {}", stdout);
    assert!(stdout.contains("next:"), "stdout was: {}", stdout);
    assert!(!stdout.contains("===="), "stdout was: {}", stdout);
    assert!(!stdout.contains("State:"), "stdout was: {}", stdout);
    assert!(!stdout.contains("Ledger:"), "stdout was: {}", stdout);
    assert!(!stdout.contains("Manifest:"), "stdout was: {}", stdout);
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
        .stdout(predicate::str::contains("done"));
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
        .stdout(predicate::str::contains("valid."));
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

// ===========================================================================
// Task 12 — ledger subcommand tests
// ===========================================================================

#[test]
fn test_ledger_list_empty() {
    // After init, the registry has a "default" entry created automatically.
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "ledger", "list"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("default"));
}

#[test]
fn test_ledger_create_and_list() {
    let dir = setup_initialized_dir();

    // Create a ledger
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "ledger",
            "create",
            "--name",
            "audit",
            "--path",
            "audit.jsonl",
            "--mode",
            "event-only",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("created: audit"));

    // List should show it
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "ledger", "list"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("audit"))
        .stdout(predicate::str::contains("event-only"));
}

#[test]
fn test_ledger_create_and_remove() {
    let dir = setup_initialized_dir();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "ledger",
            "create",
            "--name",
            "temp",
            "--path",
            "temp.jsonl",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "ledger",
            "remove",
            "--name",
            "temp",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("removed: temp"));

    // Verify the file still exists on disk (relative paths resolve against cwd)
    assert!(dir.path().join("temp.jsonl").exists());
}

#[test]
fn test_ledger_verify_by_name() {
    let dir = setup_initialized_dir();

    // Create a named ledger
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "ledger",
            "create",
            "--name",
            "verifiable",
            "--path",
            "verifiable.jsonl",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    // Verify by name
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "ledger",
            "verify",
            "--name",
            "verifiable",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("chain valid"));
}

#[test]
fn test_ledger_checkpoint() {
    let dir = setup_initialized_dir();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "ledger",
            "create",
            "--name",
            "cp-test",
            "--path",
            "cp-test.jsonl",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "ledger",
            "checkpoint",
            "--name",
            "cp-test",
            "--scope",
            "phase-1",
            "--snapshot",
            "midpoint",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("checkpoint: seq"));
}

#[test]
fn test_ledger_import_from_stdin() {
    let dir = setup_initialized_dir();

    // Import JSONL from stdin
    let input = r#"{"type":"note","fields":{"text":"hello"}}
{"type":"note","fields":{"text":"world"}}
"#;

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "ledger",
            "import",
            "--name",
            "imported",
            "--path",
            "imported.jsonl",
        ])
        .write_stdin(input)
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("imported: imported"));

    // Verify the imported ledger
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "ledger",
            "verify",
            "--name",
            "imported",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("3 entries")); // genesis + 2 events
}

#[test]
fn test_ledger_remove_nonexistent() {
    let dir = setup_initialized_dir();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "ledger",
            "remove",
            "--name",
            "ghost",
        ])
        .current_dir(dir.path())
        .assert()
        .code(3); // EXIT_CONFIG_ERROR
}

// ===========================================================================
// Task 13 — query subcommand tests
// ===========================================================================

#[test]
fn test_query_select_all() {
    let dir = setup_initialized_dir();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "query",
            "SELECT count(*) as count FROM events",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("count"))
        .stdout(predicate::str::contains("1")); // genesis only
}

#[test]
fn test_query_with_type_filter() {
    let dir = setup_initialized_dir();

    // Add a transition first
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Query for state_transition events
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "query",
            "--type",
            "state_transition",
            "--count",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("1"));
}

#[test]
fn test_query_json_output() {
    let dir = setup_initialized_dir();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "query",
            "--json",
            "SELECT type, seq FROM events ORDER BY seq LIMIT 1",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let json: serde_json::Value =
        serde_json::from_slice(&output).expect("should produce valid JSON");
    assert!(json.is_array());
    let arr = json.as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["type"].as_str().unwrap(), "genesis");
}

#[test]
fn test_query_csv_output() {
    let dir = setup_initialized_dir();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "query",
            "--format",
            "csv",
            "SELECT type, seq FROM events ORDER BY seq LIMIT 1",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("seq,type"))
        .stdout(predicate::str::contains("0,genesis"));
}

#[test]
fn test_query_jsonl_output() {
    let dir = setup_initialized_dir();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "query",
            "--format",
            "jsonl",
            "SELECT type FROM events ORDER BY seq LIMIT 1",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(r#""type":"genesis""#));
}

#[test]
fn test_query_convenience_no_sql() {
    let dir = setup_initialized_dir();

    // No SQL, just --type
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "query",
            "--type",
            "genesis",
            "--format",
            "json",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("genesis"));
}

// ===========================================================================
// Task 14 — --ledger / --ledger-path targeting tests
// ===========================================================================

#[test]
fn test_ledger_path_targeting_log_verify() {
    let dir = setup_initialized_dir();

    // Verify the default ledger via --ledger-path
    let ledger_file = dir
        .path()
        .join("output/.sahjhan/ledger.jsonl")
        .to_str()
        .unwrap()
        .to_string();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "--ledger-path",
            &ledger_file,
            "log",
            "verify",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("chain valid"));
}

#[test]
fn test_named_ledger_targeting_log_dump() {
    let dir = setup_initialized_dir();

    // Create a named ledger
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "ledger",
            "create",
            "--name",
            "second",
            "--path",
            "second.jsonl",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    // Use --ledger to target the named ledger for log dump
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "--ledger",
            "second",
            "log",
            "dump",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("genesis"));
}

#[test]
fn test_event_only_blocks_transition() {
    let dir = setup_initialized_dir();

    // Create an event-only ledger
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "ledger",
            "create",
            "--name",
            "eo",
            "--path",
            "eo.jsonl",
            "--mode",
            "event-only",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    // Try to transition on the event-only ledger — should fail with code 3
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "--ledger",
            "eo",
            "transition",
            "begin",
        ])
        .current_dir(dir.path())
        .assert()
        .code(3)
        .stderr(predicate::str::contains("event-only ledger"));
}

#[test]
fn test_event_only_blocks_gate_check() {
    let dir = setup_initialized_dir();

    // Create an event-only ledger
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "ledger",
            "create",
            "--name",
            "eo2",
            "--path",
            "eo2.jsonl",
            "--mode",
            "event-only",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    // Try to check gates on event-only ledger — should fail
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "--ledger",
            "eo2",
            "gate",
            "check",
            "begin",
        ])
        .current_dir(dir.path())
        .assert()
        .code(3)
        .stderr(predicate::str::contains("event-only ledger"));
}

#[test]
fn test_event_only_status_metadata() {
    let dir = setup_initialized_dir();

    // Create an event-only ledger
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "ledger",
            "create",
            "--name",
            "eo3",
            "--path",
            "eo3.jsonl",
            "--mode",
            "event-only",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    // Status on event-only should show metadata
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--ledger", "eo3", "status"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("event-only"));
}

#[test]
fn test_query_on_named_ledger() {
    let dir = setup_initialized_dir();

    // Create a named ledger
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "ledger",
            "create",
            "--name",
            "queryable",
            "--path",
            "queryable.jsonl",
        ])
        .current_dir(dir.path())
        .assert()
        .success();

    // Query the named ledger using the global --ledger flag
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "--ledger",
            "queryable",
            "query",
            "SELECT count(*) as count FROM events",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("1")); // genesis only
}

#[test]
fn test_gate_check_with_args() {
    let dir = setup_initialized_dir();

    // Overwrite transitions.toml with a gate that uses {{item_id}}
    std::fs::write(
        dir.path().join("enforcement/transitions.toml"),
        r#"
[[transitions]]
from = "idle"
to = "working"
command = "begin"
gates = [
    { type = "command_succeeds", cmd = "test {{item_id}} = 'BH-019'" },
]

[[transitions]]
from = "working"
to = "done"
command = "complete"
gates = [
    { type = "set_covered", set = "check", event = "set_member_complete", field = "member" },
]
"#,
    )
    .unwrap();

    // Gate check with args should show the gate passing
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "gate",
            "check",
            "begin",
            "--",
            "item_id=BH-019",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("✓"));
}

#[test]
fn test_render_with_named_ledger_falls_back_to_default() {
    let dir = setup_initialized_dir();

    // Overwrite renders.toml to reference a named ledger that doesn't exist
    std::fs::write(
        dir.path().join("enforcement/renders.toml"),
        r#"
[[renders]]
target = "STATUS.md"
template = "templates/status.md.tera"
trigger = "on_transition"
ledger = "run"
"#,
    )
    .unwrap();

    // Render should succeed by falling back to the default ledger
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "render"])
        .current_dir(dir.path())
        .assert()
        .success();

    // STATUS.md should exist and have content from the default ledger
    assert!(dir.path().join("output/STATUS.md").exists());
    let content = std::fs::read_to_string(dir.path().join("output/STATUS.md")).unwrap();
    assert!(content.contains("idle") || content.contains("Idle"));
}

#[test]
fn test_transition_with_template_args() {
    let dir = setup_initialized_dir();

    // Overwrite transitions.toml with a gate using {{item_id}}
    std::fs::write(
        dir.path().join("enforcement/transitions.toml"),
        r#"
[[transitions]]
from = "idle"
to = "working"
command = "begin"
gates = [
    { type = "command_succeeds", cmd = "test {{item_id}} = 'BH-019'" },
]

[[transitions]]
from = "working"
to = "done"
command = "complete"
gates = [
    { type = "set_covered", set = "check", event = "set_member_complete", field = "member" },
]
"#,
    )
    .unwrap();

    // Transition with item_id arg should succeed
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "transition",
            "begin",
            "item_id=BH-019",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\u{2192}"));
}

#[test]
fn test_transition_terse_output() {
    let dir = setup_initialized_dir();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("\u{2192}"), "expected arrow in: {}", stdout);
    assert!(stdout.contains("idle"), "expected old state in: {}", stdout);
    assert!(
        stdout.contains("working"),
        "expected new state in: {}",
        stdout
    );
    assert!(
        !stdout.contains("Transition:"),
        "should not have old prefix in: {}",
        stdout
    );
    assert!(
        !stdout.contains("Recorded"),
        "should not have old suffix in: {}",
        stdout
    );
}

#[test]
fn test_transition_without_required_arg_fails() {
    let dir = setup_initialized_dir();

    // Same gate requiring {{item_id}}
    std::fs::write(
        dir.path().join("enforcement/transitions.toml"),
        r#"
[[transitions]]
from = "idle"
to = "working"
command = "begin"
gates = [
    { type = "command_succeeds", cmd = "test {{item_id}} = 'BH-019'" },
]

[[transitions]]
from = "working"
to = "done"
command = "complete"
gates = [
    { type = "set_covered", set = "check", event = "set_member_complete", field = "member" },
]
"#,
    )
    .unwrap();

    // Transition without the arg — gate should fail because {{item_id}} is literal
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin"])
        .current_dir(dir.path())
        .assert()
        .code(1); // EXIT_GATE_FAILED
}

/// Create a temp directory with a config that has an optional field, then run `init`.
fn setup_optional_field_dir() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        r#"
[protocol]
name = "test-optional"
version = "1.0.0"
description = "Optional field test"

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
[events.finding_resolved]
description = "A finding was resolved"
fields = [
    { name = "id", type = "string", pattern = "^F-\\d{3}$" },
    { name = "commit_hash", type = "string" },
    { name = "evidence_path", type = "string", optional = true, pattern = "^evidence/" },
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
fn test_optional_field_provided_and_validated() {
    let dir = setup_optional_field_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "event",
            "finding_resolved",
            "--field",
            "id=F-001",
            "--field",
            "commit_hash=abc1234",
            "--field",
            "evidence_path=evidence/justification.md",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("recorded: finding_resolved"));
}

#[test]
fn test_optional_field_omitted_accepted() {
    let dir = setup_optional_field_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "event",
            "finding_resolved",
            "--field",
            "id=F-002",
            "--field",
            "commit_hash=def5678",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("recorded: finding_resolved"));
}

#[test]
fn test_optional_field_bad_pattern_rejected() {
    let dir = setup_optional_field_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "event",
            "finding_resolved",
            "--field",
            "id=F-003",
            "--field",
            "commit_hash=abc1234",
            "--field",
            "evidence_path=wrong/path.md",
        ])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("doesn't match pattern"));
}

#[test]
fn test_required_field_still_required_with_optional_present() {
    let dir = setup_optional_field_dir();
    // Omit required "id" field — should fail even though optional field exists
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "event",
            "finding_resolved",
            "--field",
            "commit_hash=abc1234",
        ])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("missing field 'id'"));
}

// ===========================================================================
// Task 5 — branching transition tests
// ===========================================================================

/// Create a temp directory with a branching protocol (two candidates for "go") and run `init`.
fn setup_branching_dir() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        r#"
[protocol]
name = "branch-test"
version = "1.0.0"
description = "Branching test protocol"

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

[states.happy]
label = "Happy path"

[states.fallback]
label = "Fallback path"
terminal = true
"#,
    )
    .unwrap();

    std::fs::write(
        config_dir.join("transitions.toml"),
        r#"
[[transitions]]
from = "idle"
to = "happy"
command = "go"
gates = [
    { type = "file_exists", path = "output/required.txt" },
]

[[transitions]]
from = "idle"
to = "fallback"
command = "go"
gates = []
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
fn test_cli_branching_takes_fallback() {
    let dir = setup_branching_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "go"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("fallback"));
}

#[test]
fn test_cli_branching_takes_first_when_gates_pass() {
    let dir = setup_branching_dir();
    std::fs::write(dir.path().join("output/required.txt"), "present").unwrap();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "go"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("happy"));
}

#[test]
fn test_cli_mermaid_raw() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "mermaid"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("stateDiagram-v2"))
        .stdout(predicate::str::contains("[*] --> idle"));
}

#[test]
fn test_cli_mermaid_rendered() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "mermaid", "--rendered"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("[idle]"))
        .stdout(predicate::str::contains("(initial)"));
}
