use sahjhan::config::ProtocolConfig;
use sahjhan::mermaid;
use std::path::Path;

#[test]
fn test_mermaid_minimal_protocol() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let output = mermaid::generate_mermaid(&config);
    assert!(output.starts_with("stateDiagram-v2"));
    assert!(output.contains("[*] --> idle"));
    assert!(output.contains("idle --> working"));
    assert!(output.contains("working --> done"));
    assert!(output.contains("done --> [*]"));
}

#[test]
fn test_mermaid_sanitizes_hyphens() {
    use sahjhan::config::*;
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.states.insert(
        "fix-and-retry".to_string(),
        StateConfig {
            label: "Fix and retry".to_string(),
            initial: None,
            terminal: None,
            params: None,
            metadata: None,
        },
    );
    config.transitions.push(TransitionConfig {
        from: "working".to_string(),
        to: "fix-and-retry".to_string(),
        command: "fail".to_string(),
        args: vec![],
        gates: vec![],
    });
    let output = mermaid::generate_mermaid(&config);
    assert!(
        output.contains("fix_and_retry"),
        "hyphens should be replaced: {}",
        output
    );
    assert!(
        output.contains("\"fix-and-retry\""),
        "original name should appear in label: {}",
        output
    );
}

#[test]
fn test_mermaid_gate_labels() {
    use sahjhan::config::*;
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.transitions.push(TransitionConfig {
        from: "idle".to_string(),
        to: "working".to_string(),
        command: "gated".to_string(),
        args: vec![],
        gates: vec![GateConfig {
            gate_type: "any_of".to_string(),
            intent: None,
            gates: vec![
                GateConfig {
                    gate_type: "file_exists".to_string(),
                    intent: None,
                    gates: vec![],
                    params: vec![("path".to_string(), toml::Value::String("a.txt".to_string()))]
                        .into_iter()
                        .collect(),
                },
                GateConfig {
                    gate_type: "file_exists".to_string(),
                    intent: None,
                    gates: vec![],
                    params: vec![("path".to_string(), toml::Value::String("b.txt".to_string()))]
                        .into_iter()
                        .collect(),
                },
            ],
            params: std::collections::HashMap::new(),
        }],
    });
    let output = mermaid::generate_mermaid(&config);
    assert!(
        output.contains("any_of(2)"),
        "composite gate should show abbreviated label: {}",
        output
    );
}

#[test]
fn test_ascii_minimal_protocol() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let output = mermaid::generate_ascii(&config);
    assert!(
        output.contains("[idle]"),
        "should contain initial state: {}",
        output
    );
    assert!(
        output.contains("(initial)"),
        "should mark initial state: {}",
        output
    );
    assert!(
        output.contains("[done]"),
        "should contain terminal state: {}",
        output
    );
    assert!(
        output.contains("(terminal)"),
        "should mark terminal state: {}",
        output
    );
    assert!(
        output.contains("begin"),
        "should contain transition command: {}",
        output
    );
    assert!(
        output.contains("complete"),
        "should contain transition command: {}",
        output
    );
}

#[test]
fn test_ascii_cycle_detection() {
    use sahjhan::config::*;
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.states.get_mut("done").unwrap().terminal = Some(false);
    config.transitions.push(TransitionConfig {
        from: "done".to_string(),
        to: "idle".to_string(),
        command: "reset".to_string(),
        args: vec![],
        gates: vec![],
    });
    let output = mermaid::generate_ascii(&config);
    assert!(
        output.contains("cycle"),
        "should detect and mark cycles: {}",
        output
    );
}
