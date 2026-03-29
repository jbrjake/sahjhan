use sahjhan::config::transitions::{GateConfig, TransitionConfig};
use sahjhan::config::ProtocolConfig;
use sahjhan::ledger::chain::Ledger;
use sahjhan::state::machine::StateMachine;
use std::collections::HashMap;
use std::path::Path;
use tempfile::tempdir;

#[test]
fn test_initial_state() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "minimal", "1.0.0").unwrap();
    let sm = StateMachine::new(&config, ledger);
    assert_eq!(sm.current_state(), "idle");
}

#[test]
fn test_valid_transition() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "minimal", "1.0.0").unwrap();
    let mut sm = StateMachine::new(&config, ledger);
    let result = sm.transition("begin", &[]);
    assert!(result.is_ok());
    assert_eq!(sm.current_state(), "working");
}

#[test]
fn test_invalid_transition_from_wrong_state() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "minimal", "1.0.0").unwrap();
    let mut sm = StateMachine::new(&config, ledger);
    // Can't complete from idle — that transition is from "working"
    let result = sm.transition("complete", &[]);
    assert!(result.is_err());
}

#[test]
fn test_gate_blocks_transition() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "minimal", "1.0.0").unwrap();
    let mut sm = StateMachine::new(&config, ledger);
    sm.transition("begin", &[]).unwrap();
    // Try to complete without recording set completions
    let result = sm.transition("complete", &[]);
    assert!(result.is_err()); // set_covered gate should fail
}

#[test]
fn test_set_completion_enables_transition() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "minimal", "1.0.0").unwrap();
    let mut sm = StateMachine::new(&config, ledger);
    sm.transition("begin", &[]).unwrap();

    // Record set completions via the ledger
    let mut fields = HashMap::new();
    fields.insert("set".to_string(), "check".to_string());
    fields.insert("member".to_string(), "tests".to_string());
    sm.record_event("set_member_complete", fields).unwrap();

    let mut fields = HashMap::new();
    fields.insert("set".to_string(), "check".to_string());
    fields.insert("member".to_string(), "lint".to_string());
    sm.record_event("set_member_complete", fields).unwrap();

    // Now the set_covered gate should pass
    let result = sm.transition("complete", &[]);
    assert!(result.is_ok());
    assert_eq!(sm.current_state(), "done");
}

#[test]
fn test_set_status() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "minimal", "1.0.0").unwrap();
    let mut sm = StateMachine::new(&config, ledger);

    let status = sm.set_status("check");
    assert_eq!(status.total, 2);
    assert_eq!(status.completed, 0);

    let mut fields = HashMap::new();
    fields.insert("set".to_string(), "check".to_string());
    fields.insert("member".to_string(), "tests".to_string());
    sm.record_event("set_member_complete", fields).unwrap();

    let status = sm.set_status("check");
    assert_eq!(status.completed, 1);
    assert!(status.members[0].done); // "tests" is first in order
    assert!(!status.members[1].done); // "lint" is not done
}

// ---------------------------------------------------------------------------
// Branching transition tests
// ---------------------------------------------------------------------------

/// Helper: build a file_exists GateConfig pointing at the given path.
fn file_exists_gate(path: &str) -> GateConfig {
    let mut params = HashMap::new();
    params.insert("path".to_string(), toml::Value::String(path.to_string()));
    GateConfig {
        gate_type: "file_exists".to_string(),
        intent: None,
        gates: vec![],
        params,
    }
}

#[test]
fn test_branching_fallback_transition() {
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();

    // Two transitions sharing from="idle", command="go":
    //   1. idle→working with file_exists gate for nonexistent file (will fail)
    //   2. idle→done with no gates (fallback)
    config.transitions = vec![
        TransitionConfig {
            from: "idle".to_string(),
            to: "working".to_string(),
            command: "go".to_string(),
            args: vec![],
            gates: vec![file_exists_gate("/nonexistent_file_that_does_not_exist")],
        },
        TransitionConfig {
            from: "idle".to_string(),
            to: "done".to_string(),
            command: "go".to_string(),
            args: vec![],
            gates: vec![],
        },
    ];

    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "minimal", "1.0.0").unwrap();
    let mut sm = StateMachine::new(&config, ledger);

    let result = sm.transition("go", &[]);
    assert!(
        result.is_ok(),
        "fallback candidate should succeed: {:?}",
        result
    );
    assert_eq!(sm.current_state(), "done");
}

#[test]
fn test_branching_first_candidate_wins() {
    let dir = tempdir().unwrap();

    // Create the file so the first candidate's gate passes.
    let gate_file = dir.path().join("exists.txt");
    std::fs::write(&gate_file, "present").unwrap();

    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.transitions = vec![
        TransitionConfig {
            from: "idle".to_string(),
            to: "working".to_string(),
            command: "go".to_string(),
            args: vec![],
            gates: vec![file_exists_gate(gate_file.to_str().unwrap())],
        },
        TransitionConfig {
            from: "idle".to_string(),
            to: "done".to_string(),
            command: "go".to_string(),
            args: vec![],
            gates: vec![],
        },
    ];

    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "minimal", "1.0.0").unwrap();
    let mut sm = StateMachine::new(&config, ledger);

    let result = sm.transition("go", &[]);
    assert!(result.is_ok(), "first candidate should win: {:?}", result);
    assert_eq!(sm.current_state(), "working");
}

#[test]
fn test_branching_all_candidates_blocked() {
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.transitions = vec![
        TransitionConfig {
            from: "idle".to_string(),
            to: "working".to_string(),
            command: "go".to_string(),
            args: vec![],
            gates: vec![file_exists_gate("/nonexistent_a")],
        },
        TransitionConfig {
            from: "idle".to_string(),
            to: "done".to_string(),
            command: "go".to_string(),
            args: vec![],
            gates: vec![file_exists_gate("/nonexistent_b")],
        },
    ];

    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "minimal", "1.0.0").unwrap();
    let mut sm = StateMachine::new(&config, ledger);

    let result = sm.transition("go", &[]);
    assert!(result.is_err(), "all candidates blocked should error");
    let err_msg = format!("{}", result.unwrap_err());
    assert!(
        err_msg.contains("all transition candidates"),
        "expected AllCandidatesBlocked error, got: {}",
        err_msg
    );
}
