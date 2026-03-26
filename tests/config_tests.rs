use sahjhan::config::ProtocolConfig;
use std::path::Path;

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
        gates: vec![],
    });
    let errors = config.validate();
    assert!(!errors.is_empty());
}
