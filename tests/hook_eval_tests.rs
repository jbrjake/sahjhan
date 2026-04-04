// tests/hook_eval_tests.rs
//
// Integration tests for the hook evaluation engine and CLI `hook eval` command.

use sahjhan::config::hooks::{
    AutoRecordConfig, HookCheck, HookConfig, HookEvent, HookFilter, MonitorConfig, MonitorTrigger,
};
use sahjhan::config::{
    GuardsConfig, PathsConfig, ProtocolConfig, ProtocolMeta, SetConfig, WriteGatedConfig,
};
use sahjhan::hooks::eval::{evaluate_hooks, HookEvalRequest};
use sahjhan::ledger::chain::Ledger;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use tempfile::tempdir;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

/// Create a minimal config loaded from examples/minimal,
/// then return a mutable clone for test customization.
fn base_config() -> ProtocolConfig {
    ProtocolConfig {
        protocol: ProtocolMeta {
            name: "test-protocol".to_string(),
            version: "1.0.0".to_string(),
            description: "Test protocol".to_string(),
        },
        paths: PathsConfig {
            managed: vec!["output".to_string()],
            data_dir: "output/.sahjhan".to_string(),
            render_dir: "output".to_string(),
        },
        sets: {
            let mut m = HashMap::new();
            m.insert(
                "check".to_string(),
                SetConfig {
                    description: "Verification checks".to_string(),
                    values: vec!["tests".to_string(), "lint".to_string()],
                },
            );
            m
        },
        aliases: HashMap::new(),
        states: {
            let mut m = HashMap::new();
            m.insert(
                "idle".to_string(),
                sahjhan::config::StateConfig {
                    label: "Idle".to_string(),
                    initial: Some(true),
                    terminal: None,
                    params: None,
                    metadata: None,
                },
            );
            m.insert(
                "working".to_string(),
                sahjhan::config::StateConfig {
                    label: "Working".to_string(),
                    initial: None,
                    terminal: None,
                    params: None,
                    metadata: None,
                },
            );
            m.insert(
                "done".to_string(),
                sahjhan::config::StateConfig {
                    label: "Done".to_string(),
                    initial: None,
                    terminal: Some(true),
                    params: None,
                    metadata: None,
                },
            );
            m
        },
        transitions: vec![],
        events: HashMap::new(),
        renders: vec![],
        checkpoints: Default::default(),
        ledgers: HashMap::new(),
        guards: None,
        hooks: vec![],
        monitors: vec![],
    }
}

/// Create a ledger in a given state.
fn setup_ledger_in_state(dir: &Path, state: &str) -> Ledger {
    let ledger_path = dir.join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();
    if state != "idle" {
        let mut fields = BTreeMap::new();
        fields.insert("from".to_string(), "idle".to_string());
        fields.insert("to".to_string(), state.to_string());
        fields.insert("command".to_string(), "begin".to_string());
        ledger.append("state_transition", fields).unwrap();
    }
    ledger
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn test_hook_eval_no_hooks_allows() {
    let dir = tempdir().unwrap();
    let config = base_config();
    let ledger = setup_ledger_in_state(dir.path(), "idle");

    let request = HookEvalRequest {
        event: HookEvent::PreToolUse,
        tool: Some("Edit".to_string()),
        file: Some("src/main.rs".to_string()),
        output_text: None,
    };

    let result = evaluate_hooks(&config, &ledger, &request, dir.path());
    assert_eq!(result.decision, "allow");
    assert!(result.messages.is_empty());
    assert!(result.auto_records.is_empty());
    assert!(result.monitor_warnings.is_empty());
}

#[test]
fn test_hook_eval_gate_blocks_when_condition_not_met() {
    let dir = tempdir().unwrap();
    let mut config = base_config();
    let ledger = setup_ledger_in_state(dir.path(), "working");

    // Add a gate-based hook that requires a ledger_has_event
    config.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        tools: Some(vec!["Edit".to_string()]),
        states: None,
        states_not: None,
        action: Some("block".to_string()),
        message: Some("Must record a review event first".to_string()),
        gate: Some(sahjhan::config::GateConfig {
            gate_type: "ledger_has_event".to_string(),
            intent: None,
            gates: vec![],
            params: {
                let mut p = HashMap::new();
                p.insert(
                    "event".to_string(),
                    toml::Value::String("code_review".to_string()),
                );
                p
            },
        }),
        check: None,
        auto_record: None,
        filter: None,
    });

    let request = HookEvalRequest {
        event: HookEvent::PreToolUse,
        tool: Some("Edit".to_string()),
        file: Some("src/main.rs".to_string()),
        output_text: None,
    };

    let result = evaluate_hooks(&config, &ledger, &request, dir.path());
    assert_eq!(result.decision, "block");
    assert_eq!(result.messages.len(), 1);
    assert_eq!(result.messages[0].action, "block");
    assert_eq!(result.messages[0].source, "hook");
}

#[test]
fn test_hook_eval_gate_allows_when_condition_met() {
    let dir = tempdir().unwrap();
    let mut config = base_config();
    let mut ledger = setup_ledger_in_state(dir.path(), "working");

    // Record the required event
    let mut fields = BTreeMap::new();
    fields.insert("detail".to_string(), "reviewed".to_string());
    ledger.append("code_review", fields).unwrap();

    // Add a gate-based hook that requires a ledger_has_event
    config.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        tools: Some(vec!["Edit".to_string()]),
        states: None,
        states_not: None,
        action: Some("block".to_string()),
        message: Some("Must record a review event first".to_string()),
        gate: Some(sahjhan::config::GateConfig {
            gate_type: "ledger_has_event".to_string(),
            intent: None,
            gates: vec![],
            params: {
                let mut p = HashMap::new();
                p.insert(
                    "event".to_string(),
                    toml::Value::String("code_review".to_string()),
                );
                p
            },
        }),
        check: None,
        auto_record: None,
        filter: None,
    });

    let request = HookEvalRequest {
        event: HookEvent::PreToolUse,
        tool: Some("Edit".to_string()),
        file: Some("src/main.rs".to_string()),
        output_text: None,
    };

    let result = evaluate_hooks(&config, &ledger, &request, dir.path());
    assert_eq!(result.decision, "allow");
    assert!(result.messages.is_empty());
}

#[test]
fn test_hook_eval_filter_excludes_test_files() {
    let dir = tempdir().unwrap();
    let mut config = base_config();
    let ledger = setup_ledger_in_state(dir.path(), "working");

    // Hook with filter that excludes test files
    config.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        tools: Some(vec!["Edit".to_string()]),
        states: None,
        states_not: None,
        action: Some("block".to_string()),
        message: Some("Blocked".to_string()),
        gate: Some(sahjhan::config::GateConfig {
            gate_type: "ledger_has_event".to_string(),
            intent: None,
            gates: vec![],
            params: {
                let mut p = HashMap::new();
                p.insert(
                    "event".to_string(),
                    toml::Value::String("nonexistent_event".to_string()),
                );
                p
            },
        }),
        check: None,
        auto_record: None,
        filter: Some(HookFilter {
            path_matches: None,
            path_not_matches: Some("tests/*".to_string()),
        }),
    });

    // Request for a test file — should be excluded by filter
    let request = HookEvalRequest {
        event: HookEvent::PreToolUse,
        tool: Some("Edit".to_string()),
        file: Some("tests/my_test.rs".to_string()),
        output_text: None,
    };

    let result = evaluate_hooks(&config, &ledger, &request, dir.path());
    assert_eq!(result.decision, "allow");
    assert!(result.messages.is_empty());

    // Request for a source file — should fire
    let request2 = HookEvalRequest {
        event: HookEvent::PreToolUse,
        tool: Some("Edit".to_string()),
        file: Some("src/main.rs".to_string()),
        output_text: None,
    };

    let result2 = evaluate_hooks(&config, &ledger, &request2, dir.path());
    assert_eq!(result2.decision, "block");
    assert_eq!(result2.messages.len(), 1);
}

#[test]
fn test_hook_eval_state_filtering() {
    let dir = tempdir().unwrap();
    let mut config = base_config();
    let ledger = setup_ledger_in_state(dir.path(), "idle");

    // Hook only fires in "working" state
    config.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        tools: Some(vec!["Edit".to_string()]),
        states: Some(vec!["working".to_string()]),
        states_not: None,
        action: Some("block".to_string()),
        message: Some("Only fires in working state".to_string()),
        gate: Some(sahjhan::config::GateConfig {
            gate_type: "ledger_has_event".to_string(),
            intent: None,
            gates: vec![],
            params: {
                let mut p = HashMap::new();
                p.insert(
                    "event".to_string(),
                    toml::Value::String("nonexistent".to_string()),
                );
                p
            },
        }),
        check: None,
        auto_record: None,
        filter: None,
    });

    // We're in "idle" — hook should NOT fire
    let request = HookEvalRequest {
        event: HookEvent::PreToolUse,
        tool: Some("Edit".to_string()),
        file: Some("src/main.rs".to_string()),
        output_text: None,
    };

    let result = evaluate_hooks(&config, &ledger, &request, dir.path());
    assert_eq!(result.decision, "allow");
    assert!(result.messages.is_empty());
}

#[test]
fn test_hook_eval_monitor_warning() {
    let dir = tempdir().unwrap();
    let mut config = base_config();
    let mut ledger = setup_ledger_in_state(dir.path(), "working");

    // Add some events after the transition
    for i in 0..5 {
        let mut fields = BTreeMap::new();
        fields.insert("detail".to_string(), format!("event {}", i));
        ledger.append("work_item", fields).unwrap();
    }

    // Monitor that fires when event count >= 3
    config.monitors.push(MonitorConfig {
        name: "high_event_count".to_string(),
        states: Some(vec!["working".to_string()]),
        action: "warn".to_string(),
        message: "High activity: {count} events since last transition in {current_state}"
            .to_string(),
        trigger: MonitorTrigger {
            trigger_type: "event_count_since_last_transition".to_string(),
            threshold: 3,
        },
    });

    let request = HookEvalRequest {
        event: HookEvent::PreToolUse,
        tool: Some("Edit".to_string()),
        file: Some("src/main.rs".to_string()),
        output_text: None,
    };

    let result = evaluate_hooks(&config, &ledger, &request, dir.path());
    assert_eq!(result.decision, "warn");
    assert_eq!(result.monitor_warnings.len(), 1);
    assert_eq!(result.monitor_warnings[0].name, "high_event_count");
    assert!(result.monitor_warnings[0]
        .message
        .contains("5 events since last transition"));
    assert!(result.monitor_warnings[0].message.contains("working"));
}

#[test]
fn test_hook_eval_write_gated_blocks() {
    let dir = tempdir().unwrap();
    let mut config = base_config();
    let ledger = setup_ledger_in_state(dir.path(), "idle");

    // Add write-gated guard
    config.guards = Some(GuardsConfig {
        write_gated: vec![WriteGatedConfig {
            path: "src/*.rs".to_string(),
            writable_in: vec!["working".to_string()],
            message: "Source files are only writable during working state".to_string(),
        }],
    });

    // We're in "idle", trying to edit a .rs file — should be blocked
    let request = HookEvalRequest {
        event: HookEvent::PreToolUse,
        tool: Some("Edit".to_string()),
        file: Some("src/main.rs".to_string()),
        output_text: None,
    };

    let result = evaluate_hooks(&config, &ledger, &request, dir.path());
    assert_eq!(result.decision, "block");
    assert_eq!(result.messages.len(), 1);
    assert_eq!(result.messages[0].source, "write_gated");
    assert_eq!(result.messages[0].action, "block");
    assert!(result.messages[0]
        .message
        .contains("Source files are only writable"));
}

#[test]
fn test_hook_eval_stop_output_pattern() {
    let dir = tempdir().unwrap();
    let mut config = base_config();
    let ledger = setup_ledger_in_state(dir.path(), "working");

    // Stop hook with output_contains_any check
    config.hooks.push(HookConfig {
        event: HookEvent::Stop,
        tools: None,
        states: None,
        states_not: None,
        action: Some("block".to_string()),
        message: Some("Agent output contains prohibited pattern".to_string()),
        gate: None,
        check: Some(HookCheck {
            check_type: "output_contains_any".to_string(),
            sql: None,
            compare: None,
            threshold: None,
            patterns: Some(vec![
                "I'll skip".to_string(),
                "let me skip".to_string(),
                "skipping".to_string(),
            ]),
        }),
        auto_record: None,
        filter: None,
    });

    // Output that matches
    let request = HookEvalRequest {
        event: HookEvent::Stop,
        tool: None,
        file: None,
        output_text: Some(
            "I've finished most of the work. I'll skip the testing for now.".to_string(),
        ),
    };

    let result = evaluate_hooks(&config, &ledger, &request, dir.path());
    assert_eq!(result.decision, "block");
    assert_eq!(result.messages.len(), 1);
    assert_eq!(result.messages[0].source, "hook");

    // Output that does NOT match
    let request2 = HookEvalRequest {
        event: HookEvent::Stop,
        tool: None,
        file: None,
        output_text: Some("All work is complete and tests pass.".to_string()),
    };

    let result2 = evaluate_hooks(&config, &ledger, &request2, dir.path());
    assert_eq!(result2.decision, "allow");
    assert!(result2.messages.is_empty());
}

#[test]
fn test_hook_eval_managed_path_blocks() {
    let dir = tempdir().unwrap();
    let config = base_config();
    let ledger = setup_ledger_in_state(dir.path(), "working");

    // Try to edit a managed path
    let request = HookEvalRequest {
        event: HookEvent::PreToolUse,
        tool: Some("Write".to_string()),
        file: Some("output/report.md".to_string()),
        output_text: None,
    };

    let result = evaluate_hooks(&config, &ledger, &request, dir.path());
    assert_eq!(result.decision, "block");
    assert_eq!(result.messages.len(), 1);
    assert_eq!(result.messages[0].source, "managed_path");
    assert!(result.messages[0].message.contains("managed by sahjhan"));
}

#[test]
fn test_hook_eval_auto_record() {
    let dir = tempdir().unwrap();
    let mut config = base_config();

    // Add an event definition for the auto-recorded event
    config.events.insert(
        "tool_usage".to_string(),
        sahjhan::config::EventConfig {
            description: "Tool usage event".to_string(),
            fields: vec![sahjhan::config::EventFieldConfig {
                name: "file_path".to_string(),
                field_type: "string".to_string(),
                pattern: None,
                values: None,
                optional: false,
            }],
            restricted: None,
        },
    );

    let ledger = setup_ledger_in_state(dir.path(), "working");

    // Auto-record hook
    config.hooks.push(HookConfig {
        event: HookEvent::PostToolUse,
        tools: Some(vec!["Edit".to_string()]),
        states: None,
        states_not: None,
        action: None,
        message: None,
        gate: None,
        check: None,
        auto_record: Some(AutoRecordConfig {
            event_type: "tool_usage".to_string(),
            fields: {
                let mut f = HashMap::new();
                f.insert("file_path".to_string(), "{tool.file_path}".to_string());
                f
            },
        }),
        filter: None,
    });

    let request = HookEvalRequest {
        event: HookEvent::PostToolUse,
        tool: Some("Edit".to_string()),
        file: Some("src/lib.rs".to_string()),
        output_text: None,
    };

    let result = evaluate_hooks(&config, &ledger, &request, dir.path());
    assert_eq!(result.decision, "allow");
    assert_eq!(result.auto_records.len(), 1);
    assert_eq!(result.auto_records[0].event_type, "tool_usage");
    assert_eq!(
        result.auto_records[0].fields.get("file_path").unwrap(),
        "src/lib.rs"
    );
}

#[test]
fn test_hook_eval_states_not_filtering() {
    let dir = tempdir().unwrap();
    let mut config = base_config();
    let ledger = setup_ledger_in_state(dir.path(), "idle");

    // Hook that fires in all states EXCEPT idle
    config.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        tools: Some(vec!["Edit".to_string()]),
        states: None,
        states_not: Some(vec!["idle".to_string()]),
        action: Some("warn".to_string()),
        message: Some("Warning".to_string()),
        gate: Some(sahjhan::config::GateConfig {
            gate_type: "ledger_has_event".to_string(),
            intent: None,
            gates: vec![],
            params: {
                let mut p = HashMap::new();
                p.insert(
                    "event".to_string(),
                    toml::Value::String("nonexistent".to_string()),
                );
                p
            },
        }),
        check: None,
        auto_record: None,
        filter: None,
    });

    let request = HookEvalRequest {
        event: HookEvent::PreToolUse,
        tool: Some("Edit".to_string()),
        file: Some("src/main.rs".to_string()),
        output_text: None,
    };

    // In idle — should NOT fire (excluded by states_not)
    let result = evaluate_hooks(&config, &ledger, &request, dir.path());
    assert_eq!(result.decision, "allow");
}

#[test]
fn test_hook_eval_event_count_check() {
    let dir = tempdir().unwrap();
    let mut config = base_config();
    let mut ledger = setup_ledger_in_state(dir.path(), "working");

    // Add 10 events
    for i in 0..10 {
        let mut fields = BTreeMap::new();
        fields.insert("detail".to_string(), format!("item {}", i));
        ledger.append("work_item", fields).unwrap();
    }

    // Check that fires when event count >= 5
    config.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        tools: None,
        states: None,
        states_not: None,
        action: Some("warn".to_string()),
        message: Some("Too many events: {count}".to_string()),
        gate: None,
        check: Some(HookCheck {
            check_type: "event_count_since_last_transition".to_string(),
            sql: None,
            compare: Some("gte".to_string()),
            threshold: Some(5),
            patterns: None,
        }),
        auto_record: None,
        filter: None,
    });

    let request = HookEvalRequest {
        event: HookEvent::PreToolUse,
        tool: Some("Edit".to_string()),
        file: Some("src/main.rs".to_string()),
        output_text: None,
    };

    let result = evaluate_hooks(&config, &ledger, &request, dir.path());
    assert_eq!(result.decision, "warn");
    assert_eq!(result.messages.len(), 1);
    assert!(result.messages[0].message.contains("10"));
}

#[test]
fn test_hook_eval_write_gated_allows_in_correct_state() {
    let dir = tempdir().unwrap();
    let mut config = base_config();
    let ledger = setup_ledger_in_state(dir.path(), "working");

    // Add write-gated guard
    config.guards = Some(GuardsConfig {
        write_gated: vec![WriteGatedConfig {
            path: "src/*.rs".to_string(),
            writable_in: vec!["working".to_string()],
            message: "Source files are only writable during working state".to_string(),
        }],
    });

    // We're in "working", trying to edit a .rs file — should be allowed
    let request = HookEvalRequest {
        event: HookEvent::PreToolUse,
        tool: Some("Edit".to_string()),
        file: Some("src/main.rs".to_string()),
        output_text: None,
    };

    let result = evaluate_hooks(&config, &ledger, &request, dir.path());
    assert_eq!(result.decision, "allow");
    assert!(result.messages.is_empty());
}

// ---------------------------------------------------------------------------
// CLI integration test
// ---------------------------------------------------------------------------

#[test]
fn test_hook_eval_cli_no_hooks_allows() {
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();
    for file in &[
        "protocol.toml",
        "states.toml",
        "transitions.toml",
        "events.toml",
    ] {
        let src = std::path::Path::new("examples/minimal").join(file);
        if src.exists() {
            std::fs::copy(&src, config_dir.join(file)).unwrap();
        }
    }
    // Init
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sahjhan"))
        .args(["--config-dir", config_dir.to_str().unwrap(), "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "init failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    // Hook eval
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sahjhan"))
        .args([
            "--config-dir",
            config_dir.to_str().unwrap(),
            "--json",
            "hook",
            "eval",
            "--event",
            "PreToolUse",
            "--tool",
            "Edit",
            "--file",
            "src/main.rs",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "hook eval failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(result["data"]["decision"], "allow");
}
