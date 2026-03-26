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
