// tests/gate_tests.rs
//
// Integration tests for the gate evaluator — one test per gate type,
// plus additional tests for timeout enforcement, snapshot resolution,
// violation resolution, and field validation.

use sahjhan::config::{GateConfig, ProtocolConfig, StateParam, TransitionConfig};
use sahjhan::gates::evaluator::{evaluate_gate, evaluate_gates, GateContext};
use sahjhan::ledger::chain::Ledger;
use sahjhan::state::machine::StateMachine;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_gate(gate_type: &str, params: Vec<(&str, toml::Value)>) -> GateConfig {
    GateConfig {
        gate_type: gate_type.to_string(),
        intent: None,
        gates: vec![],
        params: params
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// file_exists
// ---------------------------------------------------------------------------

#[test]
fn test_file_exists_pass() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let test_file = dir.path().join("existing.txt");
    std::fs::write(&test_file, "content").unwrap();

    let gate = make_gate(
        "file_exists",
        vec![(
            "path",
            toml::Value::String(test_file.to_str().unwrap().to_string()),
        )],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(
        result.passed,
        "expected pass but reason: {:?}",
        result.reason
    );
}

#[test]
fn test_file_exists_fail() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate(
        "file_exists",
        vec![(
            "path",
            toml::Value::String("/nonexistent/path/xyz123".to_string()),
        )],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(!result.passed);
}

// ---------------------------------------------------------------------------
// files_exist
// ---------------------------------------------------------------------------

#[test]
fn test_files_exist_pass() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    std::fs::write(dir.path().join("a.txt"), "").unwrap();
    std::fs::write(dir.path().join("b.txt"), "").unwrap();

    let gate = make_gate(
        "files_exist",
        vec![(
            "paths",
            toml::Value::Array(vec![
                toml::Value::String(dir.path().join("a.txt").to_str().unwrap().to_string()),
                toml::Value::String(dir.path().join("b.txt").to_str().unwrap().to_string()),
            ]),
        )],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    assert!(evaluate_gate(&gate, &ctx).passed);
}

#[test]
fn test_files_exist_fail_missing_one() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    std::fs::write(dir.path().join("a.txt"), "").unwrap();
    // b.txt intentionally missing

    let gate = make_gate(
        "files_exist",
        vec![(
            "paths",
            toml::Value::Array(vec![
                toml::Value::String(dir.path().join("a.txt").to_str().unwrap().to_string()),
                toml::Value::String(dir.path().join("b.txt").to_str().unwrap().to_string()),
            ]),
        )],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    assert!(!evaluate_gate(&gate, &ctx).passed);
}

// ---------------------------------------------------------------------------
// command_succeeds
// ---------------------------------------------------------------------------

#[test]
fn test_command_succeeds_pass() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate(
        "command_succeeds",
        vec![("cmd", toml::Value::String("true".to_string()))],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    assert!(evaluate_gate(&gate, &ctx).passed);
}

#[test]
fn test_command_succeeds_fail() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate(
        "command_succeeds",
        vec![("cmd", toml::Value::String("false".to_string()))],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    assert!(!evaluate_gate(&gate, &ctx).passed);
}

// ---------------------------------------------------------------------------
// command_succeeds — timeout enforcement (Issue #1)
// ---------------------------------------------------------------------------

#[test]
fn test_command_succeeds_timeout() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Command that sleeps for 30 seconds, but timeout is 1 second.
    let gate = make_gate(
        "command_succeeds",
        vec![
            ("cmd", toml::Value::String("sleep 30".to_string())),
            ("timeout", toml::Value::Integer(1)),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };

    let start = std::time::Instant::now();
    let result = evaluate_gate(&gate, &ctx);
    let elapsed = start.elapsed();

    assert!(!result.passed, "timed-out command should not pass");
    assert!(
        result.reason.as_ref().unwrap().contains("timed out"),
        "reason should mention timeout: {:?}",
        result.reason
    );
    // Should complete in roughly 1-3 seconds, not 30.
    assert!(
        elapsed.as_secs() < 5,
        "timeout should have been enforced, but took {:?}",
        elapsed
    );
}

// ---------------------------------------------------------------------------
// command_output
// ---------------------------------------------------------------------------

#[test]
fn test_command_output_match() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate(
        "command_output",
        vec![
            ("cmd", toml::Value::String("echo hello".to_string())),
            ("expect", toml::Value::String("hello".to_string())),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    assert!(evaluate_gate(&gate, &ctx).passed);
}

#[test]
fn test_command_output_mismatch() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate(
        "command_output",
        vec![
            ("cmd", toml::Value::String("echo hello".to_string())),
            ("expect", toml::Value::String("world".to_string())),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    assert!(!evaluate_gate(&gate, &ctx).passed);
}

// ---------------------------------------------------------------------------
// command_output — timeout enforcement (Issue #6)
// ---------------------------------------------------------------------------

#[test]
fn test_command_output_timeout() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate(
        "command_output",
        vec![
            (
                "cmd",
                toml::Value::String("sleep 30 && echo done".to_string()),
            ),
            ("expect", toml::Value::String("done".to_string())),
            ("timeout", toml::Value::Integer(1)),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };

    let start = std::time::Instant::now();
    let result = evaluate_gate(&gate, &ctx);
    let elapsed = start.elapsed();

    assert!(!result.passed, "timed-out command should not pass");
    assert!(
        result.reason.as_ref().unwrap().contains("timed out"),
        "reason should mention timeout: {:?}",
        result.reason
    );
    assert!(
        elapsed.as_secs() < 5,
        "timeout should have been enforced, but took {:?}",
        elapsed
    );
}

// ---------------------------------------------------------------------------
// ledger_has_event
// ---------------------------------------------------------------------------

#[test]
fn test_ledger_has_event_pass() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();
    ledger.append("my_event", BTreeMap::new()).unwrap();
    ledger.append("my_event", BTreeMap::new()).unwrap();

    let gate = make_gate(
        "ledger_has_event",
        vec![
            ("event", toml::Value::String("my_event".to_string())),
            ("min_count", toml::Value::Integer(2)),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    assert!(evaluate_gate(&gate, &ctx).passed);
}

#[test]
fn test_ledger_has_event_fail_count() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();
    ledger.append("my_event", BTreeMap::new()).unwrap();

    let gate = make_gate(
        "ledger_has_event",
        vec![
            ("event", toml::Value::String("my_event".to_string())),
            ("min_count", toml::Value::Integer(2)),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    assert!(!evaluate_gate(&gate, &ctx).passed);
}

#[test]
fn test_ledger_has_event_with_filter_pass() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut payload = BTreeMap::new();
    payload.insert("status".to_string(), "ok".to_string());
    ledger.append("my_event", payload).unwrap();

    let mut filter = toml::value::Table::new();
    filter.insert("status".to_string(), toml::Value::String("ok".to_string()));

    let gate = make_gate(
        "ledger_has_event",
        vec![
            ("event", toml::Value::String("my_event".to_string())),
            ("min_count", toml::Value::Integer(1)),
            ("filter", toml::Value::Table(filter)),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    assert!(evaluate_gate(&gate, &ctx).passed);
}

#[test]
fn test_ledger_has_event_with_filter_fail() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut payload = BTreeMap::new();
    payload.insert("status".to_string(), "error".to_string());
    ledger.append("my_event", payload).unwrap();

    let mut filter = toml::value::Table::new();
    filter.insert("status".to_string(), toml::Value::String("ok".to_string()));

    let gate = make_gate(
        "ledger_has_event",
        vec![
            ("event", toml::Value::String("my_event".to_string())),
            ("min_count", toml::Value::Integer(1)),
            ("filter", toml::Value::Table(filter)),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    assert!(!evaluate_gate(&gate, &ctx).passed);
}

// ---------------------------------------------------------------------------
// ledger_has_event_since
// ---------------------------------------------------------------------------

#[test]
fn test_ledger_has_event_since_pass() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Record a transition, then the event we want to detect.
    let mut trans_fields = BTreeMap::new();
    trans_fields.insert("from".to_string(), "idle".to_string());
    trans_fields.insert("to".to_string(), "working".to_string());
    trans_fields.insert("command".to_string(), "begin".to_string());
    ledger.append("state_transition", trans_fields).unwrap();
    ledger.append("check_done", BTreeMap::new()).unwrap();

    let gate = make_gate(
        "ledger_has_event_since",
        vec![
            ("event", toml::Value::String("check_done".to_string())),
            ("since", toml::Value::String("last_transition".to_string())),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "working",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    assert!(evaluate_gate(&gate, &ctx).passed);
}

#[test]
fn test_ledger_has_event_since_fail() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Record the event BEFORE the transition — should not count.
    ledger.append("check_done", BTreeMap::new()).unwrap();

    let mut trans_fields = BTreeMap::new();
    trans_fields.insert("from".to_string(), "idle".to_string());
    trans_fields.insert("to".to_string(), "working".to_string());
    trans_fields.insert("command".to_string(), "begin".to_string());
    ledger.append("state_transition", trans_fields).unwrap();

    let gate = make_gate(
        "ledger_has_event_since",
        vec![
            ("event", toml::Value::String("check_done".to_string())),
            ("since", toml::Value::String("last_transition".to_string())),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "working",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    assert!(!evaluate_gate(&gate, &ctx).passed);
}

// ---------------------------------------------------------------------------
// set_covered
// ---------------------------------------------------------------------------

#[test]
fn test_set_covered_pass() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    for member in &["tests", "lint"] {
        let mut fields = BTreeMap::new();
        fields.insert("set".to_string(), "check".to_string());
        fields.insert("member".to_string(), member.to_string());
        ledger.append("set_member_complete", fields).unwrap();
    }

    let gate = make_gate(
        "set_covered",
        vec![("set", toml::Value::String("check".to_string()))],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    assert!(evaluate_gate(&gate, &ctx).passed);
}

#[test]
fn test_set_covered_fail_partial() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Only record "tests", not "lint".
    let mut fields = BTreeMap::new();
    fields.insert("set".to_string(), "check".to_string());
    fields.insert("member".to_string(), "tests".to_string());
    ledger.append("set_member_complete", fields).unwrap();

    let gate = make_gate(
        "set_covered",
        vec![("set", toml::Value::String("check".to_string()))],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(!result.passed);
    assert!(result.reason.unwrap().contains("lint"));
}

// ---------------------------------------------------------------------------
// min_elapsed
// ---------------------------------------------------------------------------

#[test]
fn test_min_elapsed_pass_no_event() {
    // No event of the given type has occurred — elapsed is "infinite", should pass.
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate(
        "min_elapsed",
        vec![
            ("event", toml::Value::String("my_event".to_string())),
            ("seconds", toml::Value::Integer(3600)),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    assert!(evaluate_gate(&gate, &ctx).passed);
}

#[test]
fn test_min_elapsed_fail_just_happened() {
    // We record an event right now — 3600s have NOT elapsed.
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();
    ledger.append("my_event", BTreeMap::new()).unwrap();

    let gate = make_gate(
        "min_elapsed",
        vec![
            ("event", toml::Value::String("my_event".to_string())),
            ("seconds", toml::Value::Integer(3600)),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    assert!(!evaluate_gate(&gate, &ctx).passed);
}

// ---------------------------------------------------------------------------
// no_violations (Issue #3: Resolved violations are accounted for)
// ---------------------------------------------------------------------------

#[test]
fn test_no_violations_clean() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate("no_violations", vec![]);
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    assert!(evaluate_gate(&gate, &ctx).passed);
}

#[test]
fn test_no_violations_with_violation() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();
    ledger
        .append("protocol_violation", BTreeMap::new())
        .unwrap();

    let gate = make_gate("no_violations", vec![]);
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    assert!(!evaluate_gate(&gate, &ctx).passed);
}

#[test]
fn test_no_violations_with_resolved_violation() {
    // A violation that has been resolved should not block.
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    ledger
        .append("protocol_violation", BTreeMap::new())
        .unwrap();
    ledger
        .append("violation_resolved", BTreeMap::new())
        .unwrap();

    let gate = make_gate("no_violations", vec![]);
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(
        result.passed,
        "resolved violation should not block: {:?}",
        result.reason
    );
}

#[test]
fn test_no_violations_partial_resolution() {
    // Two violations, one resolved — still one unresolved.
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    ledger
        .append("protocol_violation", BTreeMap::new())
        .unwrap();
    ledger
        .append("protocol_violation", BTreeMap::new())
        .unwrap();
    ledger
        .append("violation_resolved", BTreeMap::new())
        .unwrap();

    let gate = make_gate("no_violations", vec![]);
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(!result.passed, "should still have 1 unresolved violation");
    assert!(result.reason.as_ref().unwrap().contains("1 unresolved"));
}

// ---------------------------------------------------------------------------
// field_not_empty
// ---------------------------------------------------------------------------

#[test]
fn test_field_not_empty_pass() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut fields = HashMap::new();
    fields.insert("summary".to_string(), "not empty".to_string());

    let gate = make_gate(
        "field_not_empty",
        vec![("field", toml::Value::String("summary".to_string()))],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: Some(&fields),
    };
    assert!(evaluate_gate(&gate, &ctx).passed);
}

#[test]
fn test_field_not_empty_fail_empty_value() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut fields = HashMap::new();
    fields.insert("summary".to_string(), "".to_string());

    let gate = make_gate(
        "field_not_empty",
        vec![("field", toml::Value::String("summary".to_string()))],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: Some(&fields),
    };
    assert!(!evaluate_gate(&gate, &ctx).passed);
}

#[test]
fn test_field_not_empty_fail_missing_field() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let fields = HashMap::new();

    let gate = make_gate(
        "field_not_empty",
        vec![("field", toml::Value::String("summary".to_string()))],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: Some(&fields),
    };
    assert!(!evaluate_gate(&gate, &ctx).passed);
}

// ---------------------------------------------------------------------------
// snapshot_compare
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_compare_gt_pass() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Command outputs JSON; we extract "count" and compare > 5.
    let gate = make_gate(
        "snapshot_compare",
        vec![
            (
                "cmd",
                toml::Value::String(r#"echo '{"count": 10}'"#.to_string()),
            ),
            ("extract", toml::Value::String("count".to_string())),
            ("compare", toml::Value::String("gt".to_string())),
            ("reference", toml::Value::String("5".to_string())),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    assert!(evaluate_gate(&gate, &ctx).passed);
}

#[test]
fn test_snapshot_compare_eq_pass() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate(
        "snapshot_compare",
        vec![
            (
                "cmd",
                toml::Value::String(r#"echo '{"value": 42}'"#.to_string()),
            ),
            ("extract", toml::Value::String("value".to_string())),
            ("compare", toml::Value::String("eq".to_string())),
            ("reference", toml::Value::String("42".to_string())),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    assert!(evaluate_gate(&gate, &ctx).passed);
}

#[test]
fn test_snapshot_compare_fail() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate(
        "snapshot_compare",
        vec![
            (
                "cmd",
                toml::Value::String(r#"echo '{"count": 3}'"#.to_string()),
            ),
            ("extract", toml::Value::String("count".to_string())),
            ("compare", toml::Value::String("gt".to_string())),
            ("reference", toml::Value::String("5".to_string())),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    assert!(!evaluate_gate(&gate, &ctx).passed);
}

// ---------------------------------------------------------------------------
// snapshot_compare — resolve "snapshot:key" from ledger (Issue #2)
// ---------------------------------------------------------------------------

#[test]
fn test_snapshot_compare_with_ledger_reference() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Store a snapshot in the ledger with key "baseline" and value "5".
    let mut snapshot_fields = BTreeMap::new();
    snapshot_fields.insert("key".to_string(), "baseline".to_string());
    snapshot_fields.insert("value".to_string(), "5".to_string());
    ledger.append("snapshot", snapshot_fields).unwrap();

    // Command outputs count=10, compare > snapshot:baseline (which resolves to 5).
    let gate = make_gate(
        "snapshot_compare",
        vec![
            (
                "cmd",
                toml::Value::String(r#"echo '{"count": 10}'"#.to_string()),
            ),
            ("extract", toml::Value::String("count".to_string())),
            ("compare", toml::Value::String("gt".to_string())),
            (
                "reference",
                toml::Value::String("snapshot:baseline".to_string()),
            ),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(
        result.passed,
        "should resolve snapshot:baseline to 5 and pass: {:?}",
        result.reason
    );
}

#[test]
fn test_snapshot_compare_missing_snapshot_fails() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Reference a snapshot that does not exist.
    let gate = make_gate(
        "snapshot_compare",
        vec![
            (
                "cmd",
                toml::Value::String(r#"echo '{"count": 10}'"#.to_string()),
            ),
            ("extract", toml::Value::String("count".to_string())),
            ("compare", toml::Value::String("gt".to_string())),
            (
                "reference",
                toml::Value::String("snapshot:nonexistent".to_string()),
            ),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(!result.passed, "missing snapshot should fail");
    assert!(
        result
            .reason
            .as_ref()
            .unwrap()
            .contains("no snapshot found"),
        "reason should explain missing snapshot: {:?}",
        result.reason
    );
}

#[test]
fn test_snapshot_compare_uses_most_recent_snapshot() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Two snapshots with the same key — should use the more recent one.
    let mut snap1 = BTreeMap::new();
    snap1.insert("key".to_string(), "baseline".to_string());
    snap1.insert("value".to_string(), "100".to_string());
    ledger.append("snapshot", snap1).unwrap();

    let mut snap2 = BTreeMap::new();
    snap2.insert("key".to_string(), "baseline".to_string());
    snap2.insert("value".to_string(), "5".to_string());
    ledger.append("snapshot", snap2).unwrap();

    // count=10 > snapshot:baseline. If it uses the first snapshot (100), it would fail.
    // If it uses the most recent (5), it should pass.
    let gate = make_gate(
        "snapshot_compare",
        vec![
            (
                "cmd",
                toml::Value::String(r#"echo '{"count": 10}'"#.to_string()),
            ),
            ("extract", toml::Value::String("count".to_string())),
            ("compare", toml::Value::String("gt".to_string())),
            (
                "reference",
                toml::Value::String("snapshot:baseline".to_string()),
            ),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(
        result.passed,
        "should use most recent snapshot (value=5): {:?}",
        result.reason
    );
}

// ---------------------------------------------------------------------------
// field validation before template interpolation (Issue #4)
// ---------------------------------------------------------------------------

#[test]
fn test_field_validation_rejects_invalid_pattern() {
    let dir = tempdir().unwrap();

    // Build a config that has a field pattern for "member".
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();

    // Add an event definition with a pattern-validated field.
    use sahjhan::config::{EventConfig, EventFieldConfig};
    config.events.insert(
        "test_event".to_string(),
        EventConfig {
            description: "test".to_string(),
            restricted: None,
            fields: vec![EventFieldConfig {
                name: "member".to_string(),
                field_type: "string".to_string(),
                pattern: Some(r"^[a-zA-Z0-9_-]+$".to_string()),
                values: None,
                optional: false,
            }],
        },
    );

    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // state_params contains "member" with a value that violates the pattern.
    let mut state_params = HashMap::new();
    state_params.insert("member".to_string(), "'; rm -rf /".to_string());

    let gate = make_gate(
        "command_succeeds",
        vec![("cmd", toml::Value::String("echo {{member}}".to_string()))],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params,
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(!result.passed, "invalid field value should be rejected");
    assert!(
        result
            .reason
            .as_ref()
            .unwrap()
            .contains("does not match pattern"),
        "reason should explain pattern mismatch: {:?}",
        result.reason
    );
}

#[test]
fn test_field_validation_accepts_valid_pattern() {
    let dir = tempdir().unwrap();

    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();

    use sahjhan::config::{EventConfig, EventFieldConfig};
    config.events.insert(
        "test_event".to_string(),
        EventConfig {
            description: "test".to_string(),
            restricted: None,
            fields: vec![EventFieldConfig {
                name: "member".to_string(),
                field_type: "string".to_string(),
                pattern: Some(r"^[a-zA-Z0-9_-]+$".to_string()),
                values: None,
                optional: false,
            }],
        },
    );

    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut state_params = HashMap::new();
    state_params.insert("member".to_string(), "valid-value_123".to_string());

    let gate = make_gate(
        "command_succeeds",
        vec![("cmd", toml::Value::String("echo {{member}}".to_string()))],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params,
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(
        result.passed,
        "valid field value should pass: {:?}",
        result.reason
    );
}

// ---------------------------------------------------------------------------
// evaluate_gates (batch)
// ---------------------------------------------------------------------------

#[test]
fn test_evaluate_gates_all_pass() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gates = vec![
        make_gate(
            "command_succeeds",
            vec![("cmd", toml::Value::String("true".to_string()))],
        ),
        make_gate("no_violations", vec![]),
    ];
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let results = evaluate_gates(&gates, &ctx);
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|r| r.passed));
}

#[test]
fn test_evaluate_gates_continues_after_failure() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gates = vec![
        make_gate(
            "command_succeeds",
            vec![("cmd", toml::Value::String("false".to_string()))],
        ),
        make_gate(
            "command_succeeds",
            vec![("cmd", toml::Value::String("true".to_string()))],
        ),
    ];
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let results = evaluate_gates(&gates, &ctx);
    assert_eq!(results.len(), 2);
    assert!(!results[0].passed);
    assert!(results[1].passed);
}

// ---------------------------------------------------------------------------
// query gate
// ---------------------------------------------------------------------------

#[test]
fn test_query_gate_pass() {
    // Ledger with a few events — count(*) < 10 should be true.
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    for _ in 0..3 {
        ledger.append("some_event", BTreeMap::new()).unwrap();
    }

    let gate = make_gate(
        "query",
        vec![
            (
                "sql",
                toml::Value::String("SELECT count(*) < 10 as result FROM events".to_string()),
            ),
            ("expect", toml::Value::String("true".to_string())),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(
        result.passed,
        "expected pass but reason: {:?}",
        result.reason
    );
}

#[test]
fn test_query_gate_fail() {
    // Ledger with many events — count(*) < 2 should be false.
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    for _ in 0..5 {
        ledger.append("some_event", BTreeMap::new()).unwrap();
    }

    let gate = make_gate(
        "query",
        vec![
            (
                "sql",
                toml::Value::String("SELECT count(*) < 2 as result FROM events".to_string()),
            ),
            ("expect", toml::Value::String("true".to_string())),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(
        !result.passed,
        "expected fail (5 events, count < 2 = false)"
    );
    assert!(
        result.reason.as_ref().unwrap().contains("expected 'true'"),
        "reason should mention expected value: {:?}",
        result.reason
    );
}

#[test]
fn test_query_gate_missing_sql() {
    // Gate with no sql param — should return a failed result with an error message.
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate("query", vec![]);
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(!result.passed, "missing sql param should fail");
    assert!(
        result.reason.as_ref().unwrap().contains("sql"),
        "reason should mention 'sql': {:?}",
        result.reason
    );
}

// ---------------------------------------------------------------------------
// transition args as template variables (Issue #6)
// ---------------------------------------------------------------------------

#[test]
fn test_transition_args_interpolated_in_gate_command() {
    let dir = tempdir().unwrap();

    // Build a config with a command_succeeds gate that uses {{item_id}}.
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();

    // Add a transition from idle->working with a gate that checks {{item_id}}.
    config.transitions = vec![TransitionConfig {
        from: "idle".to_string(),
        to: "working".to_string(),
        command: "begin".to_string(),
        args: vec![],
        gates: vec![make_gate(
            "command_succeeds",
            vec![(
                "cmd",
                toml::Value::String("test {{item_id}} = 'BH-019'".to_string()),
            )],
        )],
    }];

    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut machine = StateMachine::new(&config, ledger);

    // Pass item_id=BH-019 as a transition arg.
    let result = machine.transition("begin", &["item_id=BH-019".to_string()]);
    assert!(
        result.is_ok(),
        "transition should succeed with interpolated arg: {:?}",
        result.err()
    );
}

#[test]
fn test_transition_args_override_state_params() {
    let dir = tempdir().unwrap();

    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();

    // Add a state param that maps "targets" to set "check"
    config.states.get_mut("working").unwrap().params = Some(vec![StateParam {
        name: "targets".to_string(),
        set: "check".to_string(),
        source: None,
    }]);

    config.transitions = vec![TransitionConfig {
        from: "idle".to_string(),
        to: "working".to_string(),
        command: "begin".to_string(),
        args: vec![],
        gates: vec![make_gate(
            "command_succeeds",
            vec![(
                "cmd",
                toml::Value::String("test {{targets}} = 'override_val'".to_string()),
            )],
        )],
    }];

    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut machine = StateMachine::new(&config, ledger);

    // CLI arg should override the state_param value
    let result = machine.transition("begin", &["targets=override_val".to_string()]);
    assert!(
        result.is_ok(),
        "CLI arg should override state param: {:?}",
        result.err()
    );
}

// ---------------------------------------------------------------------------
// StateParam source: "current" — derives first incomplete set member
// ---------------------------------------------------------------------------

#[test]
fn test_state_param_source_current() {
    let dir = tempdir().unwrap();
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();

    // Set up: "check" set has ["tests", "lint"].
    // Mark "tests" as complete in the ledger.
    // State param with source = "current" should resolve to "lint" (first incomplete).
    config.states.get_mut("working").unwrap().params = Some(vec![StateParam {
        name: "current_item".to_string(),
        set: "check".to_string(),
        source: Some("current".to_string()),
    }]);

    // Gate: test that {{current_item}} equals 'lint' (the first incomplete member)
    config.transitions = vec![TransitionConfig {
        from: "idle".to_string(),
        to: "working".to_string(),
        command: "begin".to_string(),
        args: vec![],
        gates: vec![make_gate(
            "command_succeeds",
            vec![(
                "cmd",
                toml::Value::String("test {{current_item}} = 'lint'".to_string()),
            )],
        )],
    }];

    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Mark "tests" as complete
    let mut fields = BTreeMap::new();
    fields.insert("set".to_string(), "check".to_string());
    fields.insert("member".to_string(), "tests".to_string());
    ledger.append("set_member_complete", fields).unwrap();

    let mut machine = StateMachine::new(&config, ledger);
    let result = machine.transition("begin", &[]);
    assert!(
        result.is_ok(),
        "source=current should resolve to 'lint': {:?}",
        result.err()
    );
}

// ---------------------------------------------------------------------------
// StateParam source: "last_completed" — derives last completed set member
// ---------------------------------------------------------------------------

#[test]
fn test_state_param_source_last_completed() {
    let dir = tempdir().unwrap();
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();

    // Mark "tests" then "lint" as complete. last_completed should be "lint".
    config.states.get_mut("working").unwrap().params = Some(vec![StateParam {
        name: "completed_item".to_string(),
        set: "check".to_string(),
        source: Some("last_completed".to_string()),
    }]);

    config.transitions = vec![TransitionConfig {
        from: "idle".to_string(),
        to: "working".to_string(),
        command: "begin".to_string(),
        args: vec![],
        gates: vec![make_gate(
            "command_succeeds",
            vec![(
                "cmd",
                toml::Value::String("test {{completed_item}} = 'lint'".to_string()),
            )],
        )],
    }];

    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Complete "tests" first, then "lint"
    let mut fields = BTreeMap::new();
    fields.insert("set".to_string(), "check".to_string());
    fields.insert("member".to_string(), "tests".to_string());
    ledger.append("set_member_complete", fields).unwrap();

    let mut fields = BTreeMap::new();
    fields.insert("set".to_string(), "check".to_string());
    fields.insert("member".to_string(), "lint".to_string());
    ledger.append("set_member_complete", fields).unwrap();

    let mut machine = StateMachine::new(&config, ledger);
    let result = machine.transition("begin", &[]);
    assert!(
        result.is_ok(),
        "source=last_completed should resolve to 'lint': {:?}",
        result.err()
    );
}

// ---------------------------------------------------------------------------
// StateParam source: default (None) — backwards-compatible comma-joined values
// ---------------------------------------------------------------------------

#[test]
fn test_state_param_source_default_unchanged() {
    let dir = tempdir().unwrap();
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();

    // No source field — should produce comma-joined set values ("tests,lint").
    config.states.get_mut("working").unwrap().params = Some(vec![StateParam {
        name: "all_items".to_string(),
        set: "check".to_string(),
        source: None,
    }]);

    config.transitions = vec![TransitionConfig {
        from: "idle".to_string(),
        to: "working".to_string(),
        command: "begin".to_string(),
        args: vec![],
        gates: vec![make_gate(
            "command_succeeds",
            vec![(
                "cmd",
                toml::Value::String("test {{all_items}} = 'tests,lint'".to_string()),
            )],
        )],
    }];

    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut machine = StateMachine::new(&config, ledger);
    let result = machine.transition("begin", &[]);
    assert!(
        result.is_ok(),
        "default source should produce comma-joined values: {:?}",
        result.err()
    );
}

// ---------------------------------------------------------------------------
// query gate — template interpolation
// ---------------------------------------------------------------------------

#[test]
fn test_query_gate_interpolates_template_vars() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Append events of a specific type
    ledger.append("tagged_event", BTreeMap::new()).unwrap();
    ledger.append("tagged_event", BTreeMap::new()).unwrap();

    // Query using {{target_type}} template var — should be interpolated
    let gate = make_gate(
        "query",
        vec![
            (
                "sql",
                toml::Value::String(
                    "SELECT count(*) >= 2 as result FROM events WHERE type = '{{target_type}}'"
                        .to_string(),
                ),
            ),
            ("expect", toml::Value::String("true".to_string())),
        ],
    );

    let mut state_params = HashMap::new();
    state_params.insert("target_type".to_string(), "tagged_event".to_string());

    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params,
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };

    let result = evaluate_gate(&gate, &ctx);
    assert!(
        result.passed,
        "query with interpolated template var should pass: {:?}",
        result.reason
    );
}

// ---------------------------------------------------------------------------
// Config validation: StateParam source
// ---------------------------------------------------------------------------

#[test]
fn test_validate_rejects_invalid_source() {
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();

    config.states.get_mut("working").unwrap().params = Some(vec![StateParam {
        name: "item".to_string(),
        set: "check".to_string(),
        source: Some("bogus".to_string()),
    }]);

    let errors = config.validate();
    assert!(
        errors
            .iter()
            .any(|e| e.contains("source") && e.contains("bogus")),
        "should reject invalid source value, got: {:?}",
        errors
    );
}

// ---------------------------------------------------------------------------
// Positional args mapped to declared transition args (Issue #9)
// ---------------------------------------------------------------------------

#[test]
fn test_positional_arg_mapped_to_declared_transition_arg() {
    let dir = tempdir().unwrap();
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();

    // Transition declares args = ["item_id"] — first positional arg maps to item_id.
    config.transitions = vec![TransitionConfig {
        from: "idle".to_string(),
        to: "working".to_string(),
        command: "begin".to_string(),
        args: vec!["item_id".to_string()],
        gates: vec![make_gate(
            "command_succeeds",
            vec![(
                "cmd",
                toml::Value::String("test {{item_id}} = 'BH-029'".to_string()),
            )],
        )],
    }];

    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut machine = StateMachine::new(&config, ledger);

    // Pass BH-029 as a positional arg (no '=' sign).
    let result = machine.transition("begin", &["BH-029".to_string()]);
    assert!(
        result.is_ok(),
        "positional arg should be mapped to declared arg name: {:?}",
        result.err()
    );
}

#[test]
fn test_positional_arg_mixed_with_key_value() {
    let dir = tempdir().unwrap();
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();

    // Transition declares args = ["item_id"].
    // Gate checks both {{item_id}} (positional) and {{severity}} (key=value).
    config.transitions = vec![TransitionConfig {
        from: "idle".to_string(),
        to: "working".to_string(),
        command: "begin".to_string(),
        args: vec!["item_id".to_string()],
        gates: vec![make_gate(
            "command_succeeds",
            vec![(
                "cmd",
                toml::Value::String(
                    "test {{item_id}} = 'BH-029' && test {{severity}} = 'high'".to_string(),
                ),
            )],
        )],
    }];

    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut machine = StateMachine::new(&config, ledger);

    // Mix positional and key=value args.
    let result = machine.transition(
        "begin",
        &["BH-029".to_string(), "severity=high".to_string()],
    );
    assert!(
        result.is_ok(),
        "mixed positional and key=value args should work: {:?}",
        result.err()
    );
}

#[test]
fn test_positional_arg_overrides_state_param() {
    let dir = tempdir().unwrap();
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();

    // State param with source="current" would resolve to "tests" (first incomplete).
    // But positional arg should override it with "lint".
    config.states.get_mut("working").unwrap().params = Some(vec![StateParam {
        name: "current_item".to_string(),
        set: "check".to_string(),
        source: Some("current".to_string()),
    }]);

    config.transitions = vec![TransitionConfig {
        from: "idle".to_string(),
        to: "working".to_string(),
        command: "begin".to_string(),
        args: vec!["current_item".to_string()],
        gates: vec![make_gate(
            "command_succeeds",
            vec![(
                "cmd",
                toml::Value::String("test {{current_item}} = 'lint'".to_string()),
            )],
        )],
    }];

    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut machine = StateMachine::new(&config, ledger);

    // Positional arg "lint" should override state_param "tests"
    let result = machine.transition("begin", &["lint".to_string()]);
    assert!(
        result.is_ok(),
        "positional arg should override state_param: {:?}",
        result.err()
    );
}

#[test]
fn test_excess_positional_args_ignored() {
    let dir = tempdir().unwrap();
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();

    // Only one declared arg — extra positional args should be silently ignored.
    config.transitions = vec![TransitionConfig {
        from: "idle".to_string(),
        to: "working".to_string(),
        command: "begin".to_string(),
        args: vec!["item_id".to_string()],
        gates: vec![make_gate(
            "command_succeeds",
            vec![(
                "cmd",
                toml::Value::String("test {{item_id}} = 'BH-029'".to_string()),
            )],
        )],
    }];

    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut machine = StateMachine::new(&config, ledger);

    // Two positional args, but only one declared — second is ignored.
    let result = machine.transition("begin", &["BH-029".to_string(), "extra".to_string()]);
    assert!(
        result.is_ok(),
        "excess positional args should be ignored: {:?}",
        result.err()
    );
}

#[test]
fn test_no_declared_args_positional_ignored() {
    let dir = tempdir().unwrap();
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();

    // No declared args — positional args should be silently ignored (backward compat).
    config.transitions = vec![TransitionConfig {
        from: "idle".to_string(),
        to: "working".to_string(),
        command: "begin".to_string(),
        args: vec![],
        gates: vec![],
    }];

    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut machine = StateMachine::new(&config, ledger);

    // Positional arg with no declared args — should not break anything.
    let result = machine.transition("begin", &["BH-029".to_string()]);
    assert!(
        result.is_ok(),
        "positional args with no declared args should be ignored: {:?}",
        result.err()
    );
}

// ---------------------------------------------------------------------------
// intent field on GateConfig and GateResult
// ---------------------------------------------------------------------------

#[test]
fn test_gate_result_has_intent_from_config() {
    // A GateConfig with an explicit intent should propagate it to GateResult.intent.
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let test_file = dir.path().join("existing.txt");
    std::fs::write(&test_file, "content").unwrap();

    let gate = GateConfig {
        gate_type: "file_exists".to_string(),
        intent: Some("spec must have real content".to_string()),
        gates: vec![],
        params: vec![(
            "path".to_string(),
            toml::Value::String(test_file.to_str().unwrap().to_string()),
        )]
        .into_iter()
        .collect(),
    };
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(result.passed, "gate should pass: {:?}", result.reason);
    assert_eq!(
        result.intent.as_deref(),
        Some("spec must have real content"),
        "intent should be taken from GateConfig"
    );
}

#[test]
fn test_gate_result_has_default_intent_when_missing() {
    // A GateConfig with intent=None should have a default intent filled in.
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let test_file = dir.path().join("existing.txt");
    std::fs::write(&test_file, "content").unwrap();

    let gate = GateConfig {
        gate_type: "file_exists".to_string(),
        intent: None,
        gates: vec![],
        params: vec![(
            "path".to_string(),
            toml::Value::String(test_file.to_str().unwrap().to_string()),
        )]
        .into_iter()
        .collect(),
    };
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(result.passed, "gate should pass: {:?}", result.reason);
    assert_eq!(
        result.intent.as_deref(),
        Some("required files must exist before proceeding"),
        "intent should be the default for file_exists"
    );
}

#[test]
fn test_validate_accepts_valid_sources() {
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();

    config.states.get_mut("working").unwrap().params = Some(vec![
        StateParam {
            name: "a".to_string(),
            set: "check".to_string(),
            source: Some("values".to_string()),
        },
        StateParam {
            name: "b".to_string(),
            set: "check".to_string(),
            source: Some("current".to_string()),
        },
        StateParam {
            name: "c".to_string(),
            set: "check".to_string(),
            source: Some("last_completed".to_string()),
        },
        StateParam {
            name: "d".to_string(),
            set: "check".to_string(),
            source: None,
        },
    ]);

    let errors = config.validate();
    let source_errors: Vec<_> = errors.iter().filter(|e| e.contains("source")).collect();
    assert!(
        source_errors.is_empty(),
        "valid sources should not produce errors: {:?}",
        source_errors
    );
}

// ---------------------------------------------------------------------------
// ledger_lacks_event
// ---------------------------------------------------------------------------

#[test]
fn test_ledger_lacks_event_pass_when_no_events() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate(
        "ledger_lacks_event",
        vec![("event", toml::Value::String("finding".to_string()))],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(
        result.passed,
        "expected pass (no findings) but reason: {:?}",
        result.reason
    );
}

#[test]
fn test_ledger_lacks_event_fail_when_event_exists() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut payload = BTreeMap::new();
    payload.insert("detail".to_string(), "something bad".to_string());
    ledger.append("finding", payload).unwrap();

    let gate = make_gate(
        "ledger_lacks_event",
        vec![("event", toml::Value::String("finding".to_string()))],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(!result.passed, "expected gate to fail");
    let reason = result.reason.expect("expected a reason string");
    assert!(
        reason.contains('1'),
        "expected reason to contain '1', got: {}",
        reason
    );
}

#[test]
fn test_ledger_lacks_event_with_filter() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Append a recon finding — should NOT match the audit filter.
    let mut recon_payload = BTreeMap::new();
    recon_payload.insert("detail".to_string(), "recon finding".to_string());
    recon_payload.insert("phase".to_string(), "recon".to_string());
    ledger.append("finding", recon_payload).unwrap();

    let mut filter = toml::value::Table::new();
    filter.insert(
        "phase".to_string(),
        toml::Value::String("audit".to_string()),
    );

    let gate = make_gate(
        "ledger_lacks_event",
        vec![
            ("event", toml::Value::String("finding".to_string())),
            ("filter", toml::Value::Table(filter.clone())),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(
        result.passed,
        "recon finding should not match audit filter; reason: {:?}",
        result.reason
    );

    // Now append an audit finding — gate should fail.
    let mut audit_payload = BTreeMap::new();
    audit_payload.insert("detail".to_string(), "audit finding".to_string());
    audit_payload.insert("phase".to_string(), "audit".to_string());
    ledger.append("finding", audit_payload).unwrap();

    let gate2 = make_gate(
        "ledger_lacks_event",
        vec![
            ("event", toml::Value::String("finding".to_string())),
            ("filter", toml::Value::Table(filter)),
        ],
    );
    let ctx2 = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result2 = evaluate_gate(&gate2, &ctx2);
    assert!(!result2.passed, "audit finding should cause gate to fail");
}

// ---------------------------------------------------------------------------
// any_of (composite)
// ---------------------------------------------------------------------------

#[test]
fn test_any_of_passes_when_one_child_passes() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Create one file that exists
    let existing = dir.path().join("exists.txt");
    std::fs::write(&existing, "content").unwrap();

    let gate = GateConfig {
        gate_type: "any_of".to_string(),
        intent: None,
        gates: vec![
            make_gate(
                "file_exists",
                vec![("path", toml::Value::String("/nonexistent/xyz".to_string()))],
            ),
            make_gate(
                "file_exists",
                vec![("path", toml::Value::String(existing.to_str().unwrap().to_string()))],
            ),
        ],
        params: HashMap::new(),
    };
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(
        result.passed,
        "any_of should pass when one child passes, reason: {:?}",
        result.reason
    );
}

#[test]
fn test_any_of_fails_when_no_child_passes() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = GateConfig {
        gate_type: "any_of".to_string(),
        intent: None,
        gates: vec![
            make_gate(
                "file_exists",
                vec![("path", toml::Value::String("/nonexistent/abc".to_string()))],
            ),
            make_gate(
                "file_exists",
                vec![("path", toml::Value::String("/nonexistent/def".to_string()))],
            ),
        ],
        params: HashMap::new(),
    };
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(
        !result.passed,
        "any_of should fail when no child passes"
    );
}

// ---------------------------------------------------------------------------
// all_of (composite)
// ---------------------------------------------------------------------------

#[test]
fn test_all_of_passes_when_all_children_pass() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let file_a = dir.path().join("a.txt");
    let file_b = dir.path().join("b.txt");
    std::fs::write(&file_a, "a").unwrap();
    std::fs::write(&file_b, "b").unwrap();

    let gate = GateConfig {
        gate_type: "all_of".to_string(),
        intent: None,
        gates: vec![
            make_gate(
                "file_exists",
                vec![("path", toml::Value::String(file_a.to_str().unwrap().to_string()))],
            ),
            make_gate(
                "file_exists",
                vec![("path", toml::Value::String(file_b.to_str().unwrap().to_string()))],
            ),
        ],
        params: HashMap::new(),
    };
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(
        result.passed,
        "all_of should pass when all children pass, reason: {:?}",
        result.reason
    );
}

#[test]
fn test_all_of_fails_when_one_child_fails() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let file_a = dir.path().join("a.txt");
    std::fs::write(&file_a, "a").unwrap();

    let gate = GateConfig {
        gate_type: "all_of".to_string(),
        intent: None,
        gates: vec![
            make_gate(
                "file_exists",
                vec![("path", toml::Value::String(file_a.to_str().unwrap().to_string()))],
            ),
            make_gate(
                "file_exists",
                vec![("path", toml::Value::String("/nonexistent/xyz".to_string()))],
            ),
        ],
        params: HashMap::new(),
    };
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(
        !result.passed,
        "all_of should fail when one child fails"
    );
}

// ---------------------------------------------------------------------------
// not (composite)
// ---------------------------------------------------------------------------

#[test]
fn test_not_inverts_passing_child() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let existing = dir.path().join("exists.txt");
    std::fs::write(&existing, "content").unwrap();

    let gate = GateConfig {
        gate_type: "not".to_string(),
        intent: None,
        gates: vec![make_gate(
            "file_exists",
            vec![("path", toml::Value::String(existing.to_str().unwrap().to_string()))],
        )],
        params: HashMap::new(),
    };
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(
        !result.passed,
        "not should invert a passing child to fail"
    );
}

#[test]
fn test_not_inverts_failing_child() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = GateConfig {
        gate_type: "not".to_string(),
        intent: None,
        gates: vec![make_gate(
            "file_exists",
            vec![("path", toml::Value::String("/nonexistent/xyz".to_string()))],
        )],
        params: HashMap::new(),
    };
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(
        result.passed,
        "not should invert a failing child to pass, reason: {:?}",
        result.reason
    );
}

// ---------------------------------------------------------------------------
// k_of_n (composite)
// ---------------------------------------------------------------------------

#[test]
fn test_k_of_n_passes_at_threshold() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let file_a = dir.path().join("a.txt");
    let file_b = dir.path().join("b.txt");
    std::fs::write(&file_a, "a").unwrap();
    std::fs::write(&file_b, "b").unwrap();

    let gate = GateConfig {
        gate_type: "k_of_n".to_string(),
        intent: None,
        gates: vec![
            make_gate(
                "file_exists",
                vec![("path", toml::Value::String(file_a.to_str().unwrap().to_string()))],
            ),
            make_gate(
                "file_exists",
                vec![("path", toml::Value::String(file_b.to_str().unwrap().to_string()))],
            ),
            make_gate(
                "file_exists",
                vec![("path", toml::Value::String("/nonexistent/xyz".to_string()))],
            ),
        ],
        params: vec![("k".to_string(), toml::Value::Integer(2))]
            .into_iter()
            .collect(),
    };
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(
        result.passed,
        "k_of_n should pass when passed_count >= k, reason: {:?}",
        result.reason
    );
}

#[test]
fn test_k_of_n_fails_below_threshold() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let file_a = dir.path().join("a.txt");
    std::fs::write(&file_a, "a").unwrap();

    let gate = GateConfig {
        gate_type: "k_of_n".to_string(),
        intent: None,
        gates: vec![
            make_gate(
                "file_exists",
                vec![("path", toml::Value::String(file_a.to_str().unwrap().to_string()))],
            ),
            make_gate(
                "file_exists",
                vec![("path", toml::Value::String("/nonexistent/abc".to_string()))],
            ),
            make_gate(
                "file_exists",
                vec![("path", toml::Value::String("/nonexistent/def".to_string()))],
            ),
        ],
        params: vec![("k".to_string(), toml::Value::Integer(2))]
            .into_iter()
            .collect(),
    };
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(
        !result.passed,
        "k_of_n should fail when passed_count < k"
    );
}
