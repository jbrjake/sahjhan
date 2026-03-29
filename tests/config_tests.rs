use sahjhan::config::ProtocolConfig;
use std::path::Path;

#[test]
fn test_gate_config_nested_gates_deserialize() {
    let toml_str = r#"
[[transitions]]
from = "idle"
to = "done"
command = "go"
gates = [
    { type = "any_of", gates = [
        { type = "file_exists", path = "a.txt" },
        { type = "file_exists", path = "b.txt" },
    ]},
]
"#;
    let tf: sahjhan::config::transitions::TransitionsFile = toml::from_str(toml_str).unwrap();
    let gate = &tf.transitions[0].gates[0];
    assert_eq!(gate.gate_type, "any_of");
    assert_eq!(gate.gates.len(), 2);
    assert_eq!(gate.gates[0].gate_type, "file_exists");
    assert_eq!(gate.gates[1].gate_type, "file_exists");
}

#[test]
fn test_event_field_optional_defaults_false() {
    let toml_str = r#"
[events.test_event]
description = "Test"
fields = [
    { name = "required_field", type = "string" },
]
"#;
    let events_file: sahjhan::config::events::EventsFile = toml::from_str(toml_str).unwrap();
    let event = &events_file.events["test_event"];
    assert!(!event.fields[0].optional);
}

#[test]
fn test_event_field_optional_true() {
    let toml_str = r#"
[events.test_event]
description = "Test"
fields = [
    { name = "opt_field", type = "string", optional = true },
]
"#;
    let events_file: sahjhan::config::events::EventsFile = toml::from_str(toml_str).unwrap();
    let event = &events_file.events["test_event"];
    assert!(event.fields[0].optional);
}

#[test]
fn test_guards_config_parsed() {
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let p = dir.path();

    std::fs::write(
        p.join("protocol.toml"),
        r#"
[protocol]
name = "test"
version = "1.0.0"
description = "test"

[paths]
managed = []
data_dir = ".data"
render_dir = "."

[guards]
read_blocked = [".sahjhan/session.key", "enforcement/quiz-bank.json"]
"#,
    )
    .unwrap();

    std::fs::write(
        p.join("states.toml"),
        r#"
[states.idle]
label = "Idle"
initial = true

[states.done]
label = "Done"
"#,
    )
    .unwrap();

    std::fs::write(
        p.join("transitions.toml"),
        r#"
[[transitions]]
from = "idle"
to = "done"
command = "finish"
"#,
    )
    .unwrap();

    let config = ProtocolConfig::load(p).unwrap();

    let guards = config
        .guards
        .expect("guards should be Some when [guards] section is present");
    assert_eq!(
        guards.read_blocked.len(),
        2,
        "read_blocked should have 2 entries"
    );
    assert_eq!(
        guards.read_blocked[0], ".sahjhan/session.key",
        "first entry should be .sahjhan/session.key"
    );
}

#[test]
fn test_guards_config_absent_is_none() {
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let p = dir.path();

    std::fs::write(
        p.join("protocol.toml"),
        r#"
[protocol]
name = "test"
version = "1.0.0"
description = "test"

[paths]
managed = []
data_dir = ".data"
render_dir = "."
"#,
    )
    .unwrap();

    std::fs::write(
        p.join("states.toml"),
        r#"
[states.idle]
label = "Idle"
initial = true

[states.done]
label = "Done"
"#,
    )
    .unwrap();

    std::fs::write(
        p.join("transitions.toml"),
        r#"
[[transitions]]
from = "idle"
to = "done"
command = "finish"
"#,
    )
    .unwrap();

    let config = ProtocolConfig::load(p).unwrap();
    assert!(
        config.guards.is_none(),
        "guards should be None when [guards] section is absent"
    );
}

#[test]
fn test_event_config_restricted_field() {
    use tempfile::tempdir;

    let dir = tempdir().unwrap();
    let p = dir.path();

    std::fs::write(
        p.join("protocol.toml"),
        r#"
[protocol]
name = "test"
version = "1.0.0"
description = "test"

[paths]
managed = []
data_dir = ".data"
render_dir = "."
"#,
    )
    .unwrap();

    std::fs::write(
        p.join("states.toml"),
        r#"
[states.idle]
label = "Idle"
initial = true

[states.done]
label = "Done"
"#,
    )
    .unwrap();

    std::fs::write(
        p.join("transitions.toml"),
        r#"
[[transitions]]
from = "idle"
to = "done"
command = "finish"
"#,
    )
    .unwrap();

    std::fs::write(
        p.join("events.toml"),
        r#"
[events.audit_logged]
description = "An auditable event requiring HMAC proof"
restricted = true
fields = []

[events.normal_note]
description = "An ordinary unrestricted event"
fields = []
"#,
    )
    .unwrap();

    let config = ProtocolConfig::load(p).unwrap();

    let audit = config
        .events
        .get("audit_logged")
        .expect("audit_logged event should exist");
    assert_eq!(
        audit.restricted,
        Some(true),
        "audit_logged should have restricted = Some(true)"
    );

    let note = config
        .events
        .get("normal_note")
        .expect("normal_note event should exist");
    assert_eq!(
        note.restricted, None,
        "normal_note should have restricted = None when field is absent"
    );
}

#[test]
fn test_validate_ledger_template_both_path_and_template() {
    use sahjhan::config::*;
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.ledgers.insert(
        "bad".to_string(),
        LedgerTemplateConfig {
            description: "bad".to_string(),
            path: Some("a.jsonl".to_string()),
            path_template: Some("b/{template.instance_id}.jsonl".to_string()),
        },
    );
    let (errors, _) = config.validate_deep(Path::new("examples/minimal"));
    assert!(
        errors
            .iter()
            .any(|e| e.contains("bad") && e.contains("both")),
        "Expected error about both path and path_template: {:?}",
        errors
    );
}

#[test]
fn test_validate_ledger_template_neither_path_nor_template() {
    use sahjhan::config::*;
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.ledgers.insert(
        "empty".to_string(),
        LedgerTemplateConfig {
            description: "empty".to_string(),
            path: None,
            path_template: None,
        },
    );
    let (errors, _) = config.validate_deep(Path::new("examples/minimal"));
    assert!(
        errors
            .iter()
            .any(|e| e.contains("empty") && e.contains("must have")),
        "Expected error about missing path: {:?}",
        errors
    );
}

#[test]
fn test_validate_ledger_template_missing_instance_id_var() {
    use sahjhan::config::*;
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.ledgers.insert(
        "novar".to_string(),
        LedgerTemplateConfig {
            description: "novar".to_string(),
            path: None,
            path_template: Some("runs/ledger.jsonl".to_string()),
        },
    );
    let (errors, _) = config.validate_deep(Path::new("examples/minimal"));
    assert!(
        errors
            .iter()
            .any(|e| e.contains("novar") && e.contains("{template.instance_id}")),
        "Expected error about missing {{template.instance_id}}: {:?}",
        errors
    );
}

#[test]
fn test_ledger_templates_loaded() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    // minimal has no [ledgers] section — should default to empty
    assert!(config.ledgers.is_empty());
}

#[test]
fn test_ledger_template_fields() {
    let toml_str = r#"
        [protocol]
        name = "test"
        version = "1.0.0"
        description = "test"

        [paths]
        managed = []
        data_dir = ".data"
        render_dir = "."

        [ledgers.run]
        description = "Per-run ledger"
        path_template = "runs/{template.instance_id}/ledger.jsonl"

        [ledgers.project]
        description = "Project ledger"
        path = "project.jsonl"
    "#;

    let proto_file: sahjhan::config::protocol::ProtocolFile = toml::from_str(toml_str).unwrap();

    assert_eq!(proto_file.ledgers.len(), 2);

    let run = &proto_file.ledgers["run"];
    assert_eq!(run.description, "Per-run ledger");
    assert!(run.path_template.is_some());
    assert!(run.path.is_none());
    assert_eq!(
        run.path_template.as_ref().unwrap(),
        "runs/{template.instance_id}/ledger.jsonl"
    );

    let project = &proto_file.ledgers["project"];
    assert!(project.path.is_some());
    assert!(project.path_template.is_none());
}

#[test]
fn test_load_minimal_protocol() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    assert_eq!(config.protocol.name, "minimal");
    assert_eq!(config.states.len(), 3);
    assert_eq!(config.transitions.len(), 2);
    assert_eq!(config.events.len(), 1);
    assert!(config.sets.contains_key("check"));
    assert_eq!(config.sets["check"].values.len(), 2);
}

#[test]
fn test_initial_state_exists() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    assert!(config.initial_state().is_some());
    assert_eq!(config.initial_state().unwrap(), "idle");
}

#[test]
fn test_transitions_reference_valid_states() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let errors = config.validate();
    assert!(errors.is_empty(), "Validation errors: {:?}", errors);
}

#[test]
fn test_renders_loaded() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    assert_eq!(config.renders.len(), 2);
}

#[test]
fn test_aliases_loaded() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    assert_eq!(config.aliases.len(), 2);
    assert_eq!(config.aliases["start"], "transition begin");
}

#[test]
fn test_paths_loaded() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    assert_eq!(config.paths.managed, vec!["output"]);
    assert_eq!(config.paths.data_dir, "output/.sahjhan");
}

#[test]
fn test_validate_catches_invalid_transition_state() {
    // Create a config programmatically with a bad transition
    use sahjhan::config::*;
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.transitions.push(TransitionConfig {
        from: "nonexistent".to_string(),
        to: "also_nonexistent".to_string(),
        command: "bad".to_string(),
        args: vec![],
        gates: vec![],
    });
    let errors = config.validate();
    assert!(!errors.is_empty());
}

#[test]
fn test_render_config_ledger_template_field() {
    use sahjhan::config::renders::RendersFile;

    let toml_str = r#"
[[renders]]
target = "STATUS.md"
template = "templates/status.md.tera"
trigger = "on_transition"
ledger_template = "run"
"#;

    let rf: RendersFile = toml::from_str(toml_str).unwrap();
    assert_eq!(rf.renders.len(), 1);
    assert_eq!(rf.renders[0].ledger_template.as_deref(), Some("run"));
    assert!(rf.renders[0].ledger.is_none());
}

#[test]
fn test_validate_render_both_ledger_and_ledger_template() {
    use sahjhan::config::*;
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.renders.push(RenderConfig {
        target: "bad.md".to_string(),
        template: "templates/status.md.tera".to_string(),
        trigger: "on_transition".to_string(),
        event_types: None,
        ledger: Some("default".to_string()),
        ledger_template: Some("run".to_string()),
    });
    let (errors, _) = config.validate_deep(Path::new("examples/minimal"));
    assert!(
        errors
            .iter()
            .any(|e| e.contains("bad.md") && e.contains("both")),
        "Expected error about both ledger and ledger_template: {:?}",
        errors
    );
}

#[test]
fn test_validate_render_ledger_template_references_valid_template() {
    use sahjhan::config::*;
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.renders.push(RenderConfig {
        target: "ref.md".to_string(),
        template: "templates/status.md.tera".to_string(),
        trigger: "on_transition".to_string(),
        event_types: None,
        ledger: None,
        ledger_template: Some("nonexistent".to_string()),
    });
    let (errors, _) = config.validate_deep(Path::new("examples/minimal"));
    assert!(
        errors
            .iter()
            .any(|e| e.contains("ref.md") && e.contains("nonexistent")),
        "Expected error about unknown ledger template: {:?}",
        errors
    );
}
