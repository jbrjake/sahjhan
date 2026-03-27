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
            fields: vec![EventFieldConfig {
                name: "member".to_string(),
                field_type: "string".to_string(),
                pattern: Some(r"^[a-zA-Z0-9_-]+$".to_string()),
                values: None,
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
            fields: vec![EventFieldConfig {
                name: "member".to_string(),
                field_type: "string".to_string(),
                pattern: Some(r"^[a-zA-Z0-9_-]+$".to_string()),
                values: None,
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
    }]);

    config.transitions = vec![TransitionConfig {
        from: "idle".to_string(),
        to: "working".to_string(),
        command: "begin".to_string(),
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
