// tests/gate_tests.rs
//
// Integration tests for the gate evaluator — one test per gate type.

use sahjhan::gates::evaluator::{evaluate_gate, GateContext};
use sahjhan::config::{GateConfig, ProtocolConfig};
use sahjhan::ledger::chain::Ledger;
use tempfile::tempdir;
use std::collections::HashMap;
use std::path::Path;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_gate(gate_type: &str, params: Vec<(&str, toml::Value)>) -> GateConfig {
    GateConfig {
        gate_type: gate_type.to_string(),
        params: params.into_iter().map(|(k, v)| (k.to_string(), v)).collect(),
    }
}

// ---------------------------------------------------------------------------
// file_exists
// ---------------------------------------------------------------------------

#[test]
fn test_file_exists_pass() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let test_file = dir.path().join("existing.txt");
    std::fs::write(&test_file, "content").unwrap();

    let gate = make_gate("file_exists", vec![
        ("path", toml::Value::String(test_file.to_str().unwrap().to_string())),
    ]);
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(result.passed, "expected pass but reason: {:?}", result.reason);
}

#[test]
fn test_file_exists_fail() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate("file_exists", vec![
        ("path", toml::Value::String("/nonexistent/path/xyz123".to_string())),
    ]);
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
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    std::fs::write(dir.path().join("a.txt"), "").unwrap();
    std::fs::write(dir.path().join("b.txt"), "").unwrap();

    let gate = make_gate("files_exist", vec![
        ("paths", toml::Value::Array(vec![
            toml::Value::String(dir.path().join("a.txt").to_str().unwrap().to_string()),
            toml::Value::String(dir.path().join("b.txt").to_str().unwrap().to_string()),
        ])),
    ]);
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
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    std::fs::write(dir.path().join("a.txt"), "").unwrap();
    // b.txt intentionally missing

    let gate = make_gate("files_exist", vec![
        ("paths", toml::Value::Array(vec![
            toml::Value::String(dir.path().join("a.txt").to_str().unwrap().to_string()),
            toml::Value::String(dir.path().join("b.txt").to_str().unwrap().to_string()),
        ])),
    ]);
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
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate("command_succeeds", vec![
        ("cmd", toml::Value::String("true".to_string())),
    ]);
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
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate("command_succeeds", vec![
        ("cmd", toml::Value::String("false".to_string())),
    ]);
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
// command_output
// ---------------------------------------------------------------------------

#[test]
fn test_command_output_match() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate("command_output", vec![
        ("cmd", toml::Value::String("echo hello".to_string())),
        ("expect", toml::Value::String("hello".to_string())),
    ]);
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
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate("command_output", vec![
        ("cmd", toml::Value::String("echo hello".to_string())),
        ("expect", toml::Value::String("world".to_string())),
    ]);
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
// ledger_has_event
// ---------------------------------------------------------------------------

#[test]
fn test_ledger_has_event_pass() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();
    ledger.append("my_event", rmp_serde::to_vec(&HashMap::<String,String>::new()).unwrap()).unwrap();
    ledger.append("my_event", rmp_serde::to_vec(&HashMap::<String,String>::new()).unwrap()).unwrap();

    let gate = make_gate("ledger_has_event", vec![
        ("event", toml::Value::String("my_event".to_string())),
        ("min_count", toml::Value::Integer(2)),
    ]);
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
    let ledger_path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();
    ledger.append("my_event", rmp_serde::to_vec(&HashMap::<String,String>::new()).unwrap()).unwrap();

    let gate = make_gate("ledger_has_event", vec![
        ("event", toml::Value::String("my_event".to_string())),
        ("min_count", toml::Value::Integer(2)),
    ]);
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
    let ledger_path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut payload = HashMap::new();
    payload.insert("status".to_string(), "ok".to_string());
    ledger.append("my_event", rmp_serde::to_vec(&payload).unwrap()).unwrap();

    let mut filter = toml::value::Table::new();
    filter.insert("status".to_string(), toml::Value::String("ok".to_string()));

    let gate = make_gate("ledger_has_event", vec![
        ("event", toml::Value::String("my_event".to_string())),
        ("min_count", toml::Value::Integer(1)),
        ("filter", toml::Value::Table(filter)),
    ]);
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
    let ledger_path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut payload = HashMap::new();
    payload.insert("status".to_string(), "error".to_string());
    ledger.append("my_event", rmp_serde::to_vec(&payload).unwrap()).unwrap();

    let mut filter = toml::value::Table::new();
    filter.insert("status".to_string(), toml::Value::String("ok".to_string()));

    let gate = make_gate("ledger_has_event", vec![
        ("event", toml::Value::String("my_event".to_string())),
        ("min_count", toml::Value::Integer(1)),
        ("filter", toml::Value::Table(filter)),
    ]);
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
    let ledger_path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Record a transition, then the event we want to detect.
    let mut trans_fields = HashMap::new();
    trans_fields.insert("from".to_string(), "idle".to_string());
    trans_fields.insert("to".to_string(), "working".to_string());
    trans_fields.insert("command".to_string(), "begin".to_string());
    ledger.append("state_transition", rmp_serde::to_vec(&trans_fields).unwrap()).unwrap();
    ledger.append("check_done", rmp_serde::to_vec(&HashMap::<String,String>::new()).unwrap()).unwrap();

    let gate = make_gate("ledger_has_event_since", vec![
        ("event", toml::Value::String("check_done".to_string())),
        ("since", toml::Value::String("last_transition".to_string())),
    ]);
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
    let ledger_path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Record the event BEFORE the transition — should not count.
    ledger.append("check_done", rmp_serde::to_vec(&HashMap::<String,String>::new()).unwrap()).unwrap();

    let mut trans_fields = HashMap::new();
    trans_fields.insert("from".to_string(), "idle".to_string());
    trans_fields.insert("to".to_string(), "working".to_string());
    trans_fields.insert("command".to_string(), "begin".to_string());
    ledger.append("state_transition", rmp_serde::to_vec(&trans_fields).unwrap()).unwrap();

    let gate = make_gate("ledger_has_event_since", vec![
        ("event", toml::Value::String("check_done".to_string())),
        ("since", toml::Value::String("last_transition".to_string())),
    ]);
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
    let ledger_path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    for member in &["tests", "lint"] {
        let mut fields = HashMap::new();
        fields.insert("set".to_string(), "check".to_string());
        fields.insert("member".to_string(), member.to_string());
        ledger.append("set_member_complete", rmp_serde::to_vec(&fields).unwrap()).unwrap();
    }

    let gate = make_gate("set_covered", vec![
        ("set", toml::Value::String("check".to_string())),
    ]);
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
    let ledger_path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Only record "tests", not "lint".
    let mut fields = HashMap::new();
    fields.insert("set".to_string(), "check".to_string());
    fields.insert("member".to_string(), "tests".to_string());
    ledger.append("set_member_complete", rmp_serde::to_vec(&fields).unwrap()).unwrap();

    let gate = make_gate("set_covered", vec![
        ("set", toml::Value::String("check".to_string())),
    ]);
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
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate("min_elapsed", vec![
        ("event", toml::Value::String("my_event".to_string())),
        ("seconds", toml::Value::Integer(3600)),
    ]);
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
    let ledger_path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();
    ledger.append("my_event", rmp_serde::to_vec(&HashMap::<String,String>::new()).unwrap()).unwrap();

    let gate = make_gate("min_elapsed", vec![
        ("event", toml::Value::String("my_event".to_string())),
        ("seconds", toml::Value::Integer(3600)),
    ]);
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
// no_violations
// ---------------------------------------------------------------------------

#[test]
fn test_no_violations_clean() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.bin");
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
    let ledger_path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();
    ledger.append("protocol_violation", rmp_serde::to_vec(&HashMap::<String,String>::new()).unwrap()).unwrap();

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

// ---------------------------------------------------------------------------
// field_not_empty
// ---------------------------------------------------------------------------

#[test]
fn test_field_not_empty_pass() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut fields = HashMap::new();
    fields.insert("summary".to_string(), "not empty".to_string());

    let gate = make_gate("field_not_empty", vec![
        ("field", toml::Value::String("summary".to_string())),
    ]);
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
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut fields = HashMap::new();
    fields.insert("summary".to_string(), "".to_string());

    let gate = make_gate("field_not_empty", vec![
        ("field", toml::Value::String("summary".to_string())),
    ]);
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
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let fields = HashMap::new();

    let gate = make_gate("field_not_empty", vec![
        ("field", toml::Value::String("summary".to_string())),
    ]);
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
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Command outputs JSON; we extract "count" and compare > 5.
    let gate = make_gate("snapshot_compare", vec![
        ("cmd", toml::Value::String(r#"echo '{"count": 10}'"#.to_string())),
        ("extract", toml::Value::String("count".to_string())),
        ("compare", toml::Value::String("gt".to_string())),
        ("reference", toml::Value::String("5".to_string())),
    ]);
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
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate("snapshot_compare", vec![
        ("cmd", toml::Value::String(r#"echo '{"value": 42}'"#.to_string())),
        ("extract", toml::Value::String("value".to_string())),
        ("compare", toml::Value::String("eq".to_string())),
        ("reference", toml::Value::String("42".to_string())),
    ]);
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
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate("snapshot_compare", vec![
        ("cmd", toml::Value::String(r#"echo '{"count": 3}'"#.to_string())),
        ("extract", toml::Value::String("count".to_string())),
        ("compare", toml::Value::String("gt".to_string())),
        ("reference", toml::Value::String("5".to_string())),
    ]);
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
// evaluate_gates (batch)
// ---------------------------------------------------------------------------

#[test]
fn test_evaluate_gates_all_pass() {
    use sahjhan::gates::evaluator::evaluate_gates;

    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gates = vec![
        make_gate("command_succeeds", vec![("cmd", toml::Value::String("true".to_string()))]),
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
    use sahjhan::gates::evaluator::evaluate_gates;

    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gates = vec![
        make_gate("command_succeeds", vec![("cmd", toml::Value::String("false".to_string()))]),
        make_gate("command_succeeds", vec![("cmd", toml::Value::String("true".to_string()))]),
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
