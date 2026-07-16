use sahjhan::config::events::{EventConfig, EventFieldConfig};
use sahjhan::config::transitions::{EmitConfig, GateConfig, TransitionConfig};
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
            emits: Vec::new(),
            from: "idle".to_string(),
            to: "working".to_string(),
            command: "go".to_string(),
            args: vec![],
            gates: vec![file_exists_gate("/nonexistent_file_that_does_not_exist")],
        },
        TransitionConfig {
            emits: Vec::new(),
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
            emits: Vec::new(),
            from: "idle".to_string(),
            to: "working".to_string(),
            command: "go".to_string(),
            args: vec![],
            gates: vec![file_exists_gate(gate_file.to_str().unwrap())],
        },
        TransitionConfig {
            emits: Vec::new(),
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
            emits: Vec::new(),
            from: "idle".to_string(),
            to: "working".to_string(),
            command: "go".to_string(),
            args: vec![],
            gates: vec![file_exists_gate("/nonexistent_a")],
        },
        TransitionConfig {
            emits: Vec::new(),
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

#[test]
fn test_transition_returns_outcome_with_states() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "minimal", "1.0.0").unwrap();
    let mut sm = StateMachine::new(&config, ledger);
    let outcome = sm.transition("begin", &[]).unwrap();
    assert_eq!(outcome.from, "idle");
    assert_eq!(outcome.to, "working");
    assert!(
        outcome.attestations.is_empty(),
        "no command gates, no attestations"
    );
    assert!(
        outcome.emitted_events.is_empty(),
        "no emits declared, none emitted"
    );
}

// ---------------------------------------------------------------------------
// Transition-emitted events (EmitConfig)
// ---------------------------------------------------------------------------

/// Helper: build an EmitConfig from field/command (name, value) pairs.
fn emit_cfg(event: &str, fields: &[(&str, &str)], commands: &[(&str, &str)]) -> EmitConfig {
    EmitConfig {
        event: event.to_string(),
        commands: commands
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        fields: fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    }
}

/// A successful transition appends its declared emit event, resolving fields
/// from the positional arg, a derivation command, and a literal — and it lands
/// on the ledger AFTER the state_transition. This is the mechanism that lets
/// `fix_commit` record `finding_resolved` in one atomic command.
#[test]
fn test_transition_emits_event_with_resolved_fields() {
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.transitions = vec![TransitionConfig {
        from: "idle".to_string(),
        to: "working".to_string(),
        command: "begin".to_string(),
        args: vec!["item_id".to_string()],
        gates: vec![],
        emits: vec![emit_cfg(
            "finding_resolved",
            &[
                ("id", "{{item_id}}"),
                ("commit_hash", "{{commit_hash}}"),
                ("phase", "fix_loop"),
            ],
            &[("commit_hash", "printf abc1234")],
        )],
    }];

    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "minimal", "1.0.0").unwrap();
    let mut sm = StateMachine::new(&config, ledger);

    let outcome = sm.transition("begin", &["BH-001".to_string()]).unwrap();
    assert_eq!(outcome.emitted_events, vec!["finding_resolved".to_string()]);
    assert_eq!(sm.current_state(), "working");

    let resolved = sm.ledger().events_of_type("finding_resolved");
    assert_eq!(resolved.len(), 1, "exactly one finding_resolved emitted");
    let e = resolved[0];
    assert_eq!(e.fields.get("id").unwrap(), "BH-001", "arg -> id");
    assert_eq!(
        e.fields.get("commit_hash").unwrap(),
        "abc1234",
        "command stdout -> commit_hash"
    );
    assert_eq!(
        e.fields.get("phase").unwrap(),
        "fix_loop",
        "literal passes through"
    );

    let st_seq = sm
        .ledger()
        .events_of_type("state_transition")
        .last()
        .unwrap()
        .seq;
    assert!(
        e.seq > st_seq,
        "emitted event must land after the state_transition"
    );
}

/// An emit whose template references an unavailable variable blocks the whole
/// transition — nothing is appended and the state does not advance.
#[test]
fn test_emit_with_unresolved_var_blocks_transition_atomically() {
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.transitions = vec![TransitionConfig {
        from: "idle".to_string(),
        to: "working".to_string(),
        command: "begin".to_string(),
        args: vec![], // no item_id declared or passed
        gates: vec![],
        emits: vec![emit_cfg("finding_resolved", &[("id", "{{item_id}}")], &[])],
    }];

    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "minimal", "1.0.0").unwrap();
    let mut sm = StateMachine::new(&config, ledger);

    let result = sm.transition("begin", &[]);
    assert!(result.is_err(), "unresolved emit var must block");
    let err = format!("{}", result.unwrap_err());
    assert!(
        err.contains("unresolved"),
        "reason should name the cause: {err}"
    );

    assert_eq!(sm.current_state(), "idle", "state must not advance");
    assert!(
        sm.ledger().events_of_type("state_transition").is_empty(),
        "no state_transition appended on a blocked emit"
    );
    assert!(sm.ledger().events_of_type("finding_resolved").is_empty());
}

/// An emit whose derivation command fails blocks the transition atomically.
#[test]
fn test_emit_with_failing_command_blocks_transition_atomically() {
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.transitions = vec![TransitionConfig {
        from: "idle".to_string(),
        to: "working".to_string(),
        command: "begin".to_string(),
        args: vec![],
        gates: vec![],
        emits: vec![emit_cfg("finding_resolved", &[], &[("h", "exit 7")])],
    }];

    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "minimal", "1.0.0").unwrap();
    let mut sm = StateMachine::new(&config, ledger);

    let result = sm.transition("begin", &[]);
    assert!(result.is_err(), "failing emit command must block");
    assert_eq!(sm.current_state(), "idle");
    assert!(sm.ledger().events_of_type("state_transition").is_empty());
}

/// Config validation rejects a transition that emits an unknown event type.
#[test]
fn test_validate_rejects_unknown_emit_event() {
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.transitions[0].emits = vec![emit_cfg("does_not_exist", &[], &[])];
    let errors = config.validate();
    assert!(
        errors
            .iter()
            .any(|e| e.contains("emits unknown event 'does_not_exist'")),
        "validate() must flag unknown emit event, got: {errors:?}"
    );
}

/// Config validation rejects a transition that emits a restricted event — an
/// emit must not bypass the HMAC proof that restricted events require.
#[test]
fn test_validate_rejects_restricted_emit_event() {
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.events.insert(
        "secret".to_string(),
        EventConfig {
            description: "restricted".to_string(),
            restricted: Some(true),
            fields: Vec::<EventFieldConfig>::new(),
        },
    );
    config.transitions[0].emits = vec![emit_cfg("secret", &[], &[])];
    let errors = config.validate();
    assert!(
        errors
            .iter()
            .any(|e| e.contains("emits restricted event 'secret'")),
        "validate() must reject emitting a restricted event, got: {errors:?}"
    );
}
