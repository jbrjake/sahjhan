use sahjhan::config::ProtocolConfig;
use std::path::Path;

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
        errors.iter().any(|e| e.contains("bad") && e.contains("both")),
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
        errors.iter().any(|e| e.contains("empty") && e.contains("must have")),
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
        errors.iter().any(|e| e.contains("novar") && e.contains("{template.instance_id}")),
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

    let proto_file: sahjhan::config::protocol::ProtocolFile =
        toml::from_str(toml_str).unwrap();

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
