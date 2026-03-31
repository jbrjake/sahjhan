# Hooks, Guards, and Monitors Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add runtime hook evaluation, state-gated write guards, monitors, and a `hook eval` CLI command so Sahjhan can enforce protocol rules at tool-use time, not just at state transitions.

**Architecture:** New `hooks.toml` optional config file provides declarative hook/monitor rules. A single `sahjhan hook eval` command evaluates all applicable rules against live ledger state and returns a JSON decision. Generated hook scripts become thin wrappers delegating to this command. The `ledger_has_event_since` gate gets a required `since` parameter.

**Tech Stack:** Rust, serde/toml for config, existing gate evaluator, existing DataFusion query engine, clap CLI, Python for generated hook scripts.

**Spec:** `docs/superpowers/specs/2026-03-31-hooks-guards-monitors-design.md`

---

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `src/config/hooks.rs` | Create | Deserialization structs for hooks.toml |
| `src/config/protocol.rs` | Modify | Add `WriteGatedConfig` to `GuardsConfig` |
| `src/config/mod.rs` | Modify | Load hooks.toml, add to `ProtocolConfig`, validate, update seals |
| `src/gates/ledger.rs` | Modify | `since` required on `ledger_has_event_since`, since_event support |
| `src/hooks/mod.rs` | Modify | Re-export new eval module |
| `src/hooks/eval.rs` | Create | Hook evaluation engine |
| `src/cli/hooks_cmd.rs` | Modify | Add `cmd_hook_eval` function |
| `src/cli/output.rs` | Modify | Add `HookEvalData` output type |
| `src/main.rs` | Modify | Add `Eval` variant to `HookAction`, dispatch |
| `src/hooks/generate.rs` | Modify | Thin wrapper scripts, updated `suggested_hooks_json` |
| `tests/hook_eval_tests.rs` | Create | Hook evaluation integration tests |
| `tests/gate_tests.rs` | Modify | Update `ledger_has_event_since` tests for required `since` |
| `tests/hook_generation_tests.rs` | Modify | Update for new generated scripts |
| `tests/config_tests.rs` | Modify | Add hooks.toml loading/validation tests |
| `examples/minimal/hooks.toml` | Create | Example hooks config |
| `README.md` | Modify | Document new features in existing voice |
| `CLAUDE.md` | Modify | Update module tables, flow maps, gate docs |

---

### Task 1: Config Structs for `hooks.toml`

**Files:**
- Create: `src/config/hooks.rs`
- Modify: `src/config/mod.rs` (add `pub mod hooks;` and re-export)

- [ ] **Step 1: Write the failing test for hooks.toml deserialization**

Add to `tests/config_tests.rs`:

```rust
#[test]
fn test_hooks_toml_deserialization() {
    use sahjhan::config::hooks::{HooksFile, HookEvent};

    let toml_str = r#"
[[hooks]]
event = "PreToolUse"
tools = ["Edit", "Write"]
states = ["fix_loop"]
action = "block"
message = "TDD violation: write a failing test first."

[hooks.gate]
type = "ledger_has_event_since"
event = "failing_test"
since = "last_transition"

[hooks.filter]
path_not_matches = "tests/*"

[[hooks]]
event = "PostToolUse"
tools = ["Edit", "Write"]

[hooks.auto_record]
event_type = "source_edit"

[hooks.auto_record.fields]
file_path = "{tool.file_path}"

[[monitors]]
name = "stall_detector"
states = ["fix_loop"]
action = "warn"
message = "20 events since last state transition."

[monitors.trigger]
type = "event_count_since_last_transition"
threshold = 20
"#;

    let hooks_file: HooksFile = toml::from_str(toml_str).unwrap();
    assert_eq!(hooks_file.hooks.len(), 2);
    assert_eq!(hooks_file.monitors.len(), 1);

    let h0 = &hooks_file.hooks[0];
    assert_eq!(h0.event, HookEvent::PreToolUse);
    assert_eq!(h0.tools, Some(vec!["Edit".to_string(), "Write".to_string()]));
    assert_eq!(h0.states, Some(vec!["fix_loop".to_string()]));
    assert_eq!(h0.action, Some("block".to_string()));
    assert!(h0.gate.is_some());
    assert!(h0.filter.is_some());
    assert!(h0.auto_record.is_none());

    let h1 = &hooks_file.hooks[1];
    assert!(h1.auto_record.is_some());
    assert!(h1.gate.is_none());
    assert_eq!(h1.auto_record.as_ref().unwrap().event_type, "source_edit");

    let m0 = &hooks_file.monitors[0];
    assert_eq!(m0.name, "stall_detector");
    assert_eq!(m0.trigger.trigger_type, "event_count_since_last_transition");
    assert_eq!(m0.trigger.threshold, 20);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_hooks_toml_deserialization -- --nocapture`
Expected: compile error — `sahjhan::config::hooks` doesn't exist

- [ ] **Step 3: Create `src/config/hooks.rs` with all config structs**

```rust
// src/config/hooks.rs
//
// Deserialization structs for hooks.toml.
//
// ## Index
// - HooksFile               — top-level wrapper
// - HookConfig              — single hook rule (gate, check, or auto_record)
// - HookEvent               — PreToolUse | PostToolUse | Stop
// - HookFilter              — path glob filters for tool arguments
// - HookCheck               — threshold/pattern check config
// - AutoRecordConfig        — auto-record event config
// - MonitorConfig           — monitor rule
// - MonitorTrigger          — monitor trigger condition

use serde::Deserialize;
use std::collections::HashMap;

use super::transitions::GateConfig;

/// Wrapper for the full hooks.toml file.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct HooksFile {
    #[serde(default)]
    pub hooks: Vec<HookConfig>,
    #[serde(default)]
    pub monitors: Vec<MonitorConfig>,
}

/// Hook event types that correspond to harness lifecycle events.
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    Stop,
}

/// A single hook rule.
#[derive(Debug, Deserialize, Clone)]
pub struct HookConfig {
    pub event: HookEvent,
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    #[serde(default)]
    pub states: Option<Vec<String>>,
    #[serde(default)]
    pub states_not: Option<Vec<String>>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub gate: Option<GateConfig>,
    #[serde(default)]
    pub check: Option<HookCheck>,
    #[serde(default)]
    pub auto_record: Option<AutoRecordConfig>,
    #[serde(default)]
    pub filter: Option<HookFilter>,
}

/// Path glob filters for tool arguments.
#[derive(Debug, Deserialize, Clone)]
pub struct HookFilter {
    #[serde(default)]
    pub path_matches: Option<String>,
    #[serde(default)]
    pub path_not_matches: Option<String>,
}

/// Threshold or pattern check for hooks.
#[derive(Debug, Deserialize, Clone)]
pub struct HookCheck {
    #[serde(rename = "type")]
    pub check_type: String,
    #[serde(default)]
    pub sql: Option<String>,
    #[serde(default)]
    pub compare: Option<String>,
    #[serde(default)]
    pub threshold: Option<i64>,
    #[serde(default)]
    pub patterns: Option<Vec<String>>,
}

/// Auto-record configuration for PostToolUse hooks.
#[derive(Debug, Deserialize, Clone)]
pub struct AutoRecordConfig {
    pub event_type: String,
    #[serde(default)]
    pub fields: HashMap<String, String>,
}

/// A monitor rule evaluated on every hook eval call.
#[derive(Debug, Deserialize, Clone)]
pub struct MonitorConfig {
    pub name: String,
    #[serde(default)]
    pub states: Option<Vec<String>>,
    pub action: String,
    pub message: String,
    pub trigger: MonitorTrigger,
}

/// Monitor trigger condition.
#[derive(Debug, Deserialize, Clone)]
pub struct MonitorTrigger {
    #[serde(rename = "type")]
    pub trigger_type: String,
    pub threshold: u64,
}
```

- [ ] **Step 4: Add module declaration to `src/config/mod.rs`**

Add after the existing `pub mod transitions;` line:

```rust
pub mod hooks;
```

Add to re-exports:

```rust
pub use hooks::{
    AutoRecordConfig, HookCheck, HookConfig, HookEvent, HookFilter, HooksFile, MonitorConfig,
    MonitorTrigger,
};
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test test_hooks_toml_deserialization -- --nocapture`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/config/hooks.rs src/config/mod.rs tests/config_tests.rs
git commit -m "feat: add hooks.toml config structs for hook/monitor deserialization"
```

---

### Task 2: Extend `GuardsConfig` with `write_gated`

**Files:**
- Modify: `src/config/protocol.rs`

- [ ] **Step 1: Write the failing test for write_gated deserialization**

Add to `tests/config_tests.rs`:

```rust
#[test]
fn test_write_gated_guards_deserialization() {
    use sahjhan::config::protocol::ProtocolFile;

    let toml_str = r#"
[protocol]
name = "test"
version = "1.0.0"
description = "test"

[paths]
managed = ["output"]
data_dir = ".sahjhan"
render_dir = "output"

[guards]
read_blocked = [".sahjhan/session.key"]

[[guards.write_gated]]
path = "docs/SUMMARY.md"
writable_in = ["finalized"]
message = "SUMMARY.md can only be written in finalized state."

[[guards.write_gated]]
path = "docs/*.md"
writable_in = ["finalized", "converged"]
message = "Doc files gated."
"#;

    let proto: ProtocolFile = toml::from_str(toml_str).unwrap();
    let guards = proto.guards.unwrap();
    assert_eq!(guards.read_blocked.len(), 1);
    assert_eq!(guards.write_gated.len(), 2);
    assert_eq!(guards.write_gated[0].path, "docs/SUMMARY.md");
    assert_eq!(guards.write_gated[0].writable_in, vec!["finalized"]);
    assert!(guards.write_gated[0].message.contains("finalized"));
    assert_eq!(guards.write_gated[1].writable_in.len(), 2);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_write_gated_guards_deserialization -- --nocapture`
Expected: FAIL — `write_gated` field doesn't exist on `GuardsConfig`

- [ ] **Step 3: Add `WriteGatedConfig` to `src/config/protocol.rs`**

Add after `GuardsConfig`:

```rust
/// A state-gated write guard entry.
///
/// Files matching `path` (supports globs) are only writable in the
/// specified states.  Checked during `hook eval` for PreToolUse Edit/Write.
#[derive(Debug, Deserialize, Clone)]
pub struct WriteGatedConfig {
    pub path: String,
    pub writable_in: Vec<String>,
    pub message: String,
}
```

Add `write_gated` field to `GuardsConfig`:

```rust
#[derive(Debug, Deserialize, Clone, Default)]
pub struct GuardsConfig {
    #[serde(default)]
    pub read_blocked: Vec<String>,
    #[serde(default)]
    pub write_gated: Vec<WriteGatedConfig>,
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test test_write_gated_guards_deserialization -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/config/protocol.rs tests/config_tests.rs
git commit -m "feat: add write_gated guards to GuardsConfig for state-conditional write protection"
```

---

### Task 3: Load `hooks.toml` in `ProtocolConfig`

**Files:**
- Modify: `src/config/mod.rs`

- [ ] **Step 1: Write the failing test**

Add to `tests/config_tests.rs`:

```rust
#[test]
fn test_protocol_config_loads_hooks_toml() {
    // The minimal example doesn't have hooks.toml — should load fine with empty hooks
    let config = sahjhan::config::ProtocolConfig::load(std::path::Path::new("examples/minimal")).unwrap();
    assert!(config.hooks.is_empty());
    assert!(config.monitors.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_protocol_config_loads_hooks_toml -- --nocapture`
Expected: FAIL — `hooks` field doesn't exist on `ProtocolConfig`

- [ ] **Step 3: Add hooks/monitors to `ProtocolConfig` and update `load()`**

In `src/config/mod.rs`, add fields to `ProtocolConfig`:

```rust
pub struct ProtocolConfig {
    // ... existing fields ...
    pub hooks: Vec<hooks::HookConfig>,
    pub monitors: Vec<hooks::MonitorConfig>,
}
```

In `ProtocolConfig::load()`, after the renders loading block, add:

```rust
// --- hooks.toml (optional) ---
let (hooks_vec, monitors_vec) = {
    let hooks_path = dir.join("hooks.toml");
    match std::fs::read_to_string(&hooks_path) {
        Ok(src) => {
            let hf: hooks::HooksFile = toml::from_str(&src)
                .map_err(|e| format!("parse error in {}: {}", hooks_path.display(), e))?;
            (hf.hooks, hf.monitors)
        }
        Err(_) => (vec![], vec![]),
    }
};
```

In the `Ok(ProtocolConfig { ... })` return, add:

```rust
hooks: hooks_vec,
monitors: monitors_vec,
```

- [ ] **Step 4: Fix all existing code that constructs `ProtocolConfig` directly**

The test helpers in `tests/gate_tests.rs`, `tests/hook_generation_tests.rs`, `src/hooks/generate.rs`, and other test files construct `ProtocolConfig` manually. Each needs `hooks: vec![]` and `monitors: vec![]` added. Search for `ProtocolConfig {` across the codebase and add the new fields to every construction site.

Run: `cargo build` to find all sites.

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test test_protocol_config_loads_hooks_toml -- --nocapture`
Expected: PASS

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: All tests pass (no regressions from adding new fields)

- [ ] **Step 7: Commit**

```bash
git add src/config/mod.rs tests/
git commit -m "feat: load hooks.toml into ProtocolConfig (optional, empty default)"
```

---

### Task 4: Validate Hooks, Monitors, and Write-Gated Guards

**Files:**
- Modify: `src/config/mod.rs` (validate and validate_deep)

- [ ] **Step 1: Write failing tests for validation**

Add to `tests/config_tests.rs`:

```rust
#[test]
fn test_validate_hook_states_reference_existing() {
    use sahjhan::config::hooks::{HookConfig, HookEvent};

    let mut config = sahjhan::config::ProtocolConfig::load(
        std::path::Path::new("examples/minimal"),
    ).unwrap();

    config.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        tools: None,
        states: Some(vec!["nonexistent_state".to_string()]),
        states_not: None,
        action: Some("block".to_string()),
        message: Some("test".to_string()),
        gate: None,
        check: None,
        auto_record: None,
        filter: None,
    });

    let config_path = std::path::Path::new("examples/minimal");
    let (errors, _warnings) = config.validate_deep(config_path);
    assert!(errors.iter().any(|e| e.contains("nonexistent_state")),
        "Should error on hook referencing unknown state. Errors: {:?}", errors);
}

#[test]
fn test_validate_auto_record_requires_post_tool_use() {
    use sahjhan::config::hooks::{HookConfig, HookEvent, AutoRecordConfig};
    use std::collections::HashMap;

    let mut config = sahjhan::config::ProtocolConfig::load(
        std::path::Path::new("examples/minimal"),
    ).unwrap();

    config.hooks.push(HookConfig {
        event: HookEvent::PreToolUse, // wrong — should be PostToolUse
        tools: None,
        states: None,
        states_not: None,
        action: None,
        message: None,
        gate: None,
        check: None,
        auto_record: Some(AutoRecordConfig {
            event_type: "source_edit".to_string(),
            fields: HashMap::new(),
        }),
        filter: None,
    });

    let config_path = std::path::Path::new("examples/minimal");
    let (errors, _) = config.validate_deep(config_path);
    assert!(errors.iter().any(|e| e.contains("auto_record") && e.contains("PostToolUse")),
        "Should error on auto_record with non-PostToolUse event. Errors: {:?}", errors);
}

#[test]
fn test_validate_monitor_names_unique() {
    use sahjhan::config::hooks::{MonitorConfig, MonitorTrigger};

    let mut config = sahjhan::config::ProtocolConfig::load(
        std::path::Path::new("examples/minimal"),
    ).unwrap();

    let monitor = MonitorConfig {
        name: "dup".to_string(),
        states: None,
        action: "warn".to_string(),
        message: "test".to_string(),
        trigger: MonitorTrigger {
            trigger_type: "event_count_since_last_transition".to_string(),
            threshold: 10,
        },
    };
    config.monitors.push(monitor.clone());
    config.monitors.push(monitor);

    let config_path = std::path::Path::new("examples/minimal");
    let (errors, _) = config.validate_deep(config_path);
    assert!(errors.iter().any(|e| e.contains("duplicate monitor name")),
        "Should error on duplicate monitor names. Errors: {:?}", errors);
}

#[test]
fn test_validate_write_gated_states_exist() {
    use sahjhan::config::protocol::{GuardsConfig, WriteGatedConfig};

    let mut config = sahjhan::config::ProtocolConfig::load(
        std::path::Path::new("examples/minimal"),
    ).unwrap();

    config.guards = Some(GuardsConfig {
        read_blocked: vec![],
        write_gated: vec![WriteGatedConfig {
            path: "docs/SUMMARY.md".to_string(),
            writable_in: vec!["nonexistent".to_string()],
            message: "test".to_string(),
        }],
    });

    let config_path = std::path::Path::new("examples/minimal");
    let (errors, _) = config.validate_deep(config_path);
    assert!(errors.iter().any(|e| e.contains("nonexistent")),
        "Should error on write_gated referencing unknown state. Errors: {:?}", errors);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_validate_hook_states test_validate_auto_record test_validate_monitor_names test_validate_write_gated -- --nocapture`
Expected: FAIL — validation doesn't check hooks/monitors/write_gated yet

- [ ] **Step 3: Add validation rules to `validate_deep()` in `src/config/mod.rs`**

Add after the existing validation blocks (after the ledger template validation):

```rust
// 14. Hook validation.
for (i, hook) in self.hooks.iter().enumerate() {
    // Hook action must be block or warn (unless auto_record)
    if hook.auto_record.is_none() {
        match hook.action.as_deref() {
            Some("block") | Some("warn") => {}
            Some(other) => {
                errors.push(format!(
                    "hooks.toml: hook {} has invalid action '{}' (must be 'block' or 'warn')",
                    i, other
                ));
            }
            None => {
                errors.push(format!(
                    "hooks.toml: hook {} requires 'action' field (unless auto_record)",
                    i
                ));
            }
        }
        if hook.message.is_none() {
            errors.push(format!(
                "hooks.toml: hook {} requires 'message' field (unless auto_record)",
                i
            ));
        }
    }

    // auto_record requires PostToolUse
    if hook.auto_record.is_some() && hook.event != hooks::HookEvent::PostToolUse {
        errors.push(format!(
            "hooks.toml: hook {} has auto_record but event is not PostToolUse",
            i
        ));
    }

    // auto_record event_type must be a defined event
    if let Some(ref ar) = hook.auto_record {
        if !self.events.contains_key(&ar.event_type) {
            errors.push(format!(
                "hooks.toml: hook {} auto_record event_type '{}' is not defined in events.toml",
                i, ar.event_type
            ));
        }
    }

    // states reference existing states
    if let Some(ref states) = hook.states {
        for s in states {
            if !self.states.contains_key(s) {
                errors.push(format!(
                    "hooks.toml: hook {} references unknown state '{}'",
                    i, s
                ));
            }
        }
    }
    if let Some(ref states_not) = hook.states_not {
        for s in states_not {
            if !self.states.contains_key(s) {
                errors.push(format!(
                    "hooks.toml: hook {} states_not references unknown state '{}'",
                    i, s
                ));
            }
        }
    }

    // gate validated through existing recursive validator
    if let Some(ref gate) = hook.gate {
        Self::validate_gate(gate, &format!("hook[{}]", i), &known_gates, &mut errors);
    }

    // check type must be known
    if let Some(ref check) = hook.check {
        let known_checks = ["query", "output_contains_any", "event_count_since_last_transition"];
        if !known_checks.contains(&check.check_type.as_str()) {
            errors.push(format!(
                "hooks.toml: hook {} has unknown check type '{}'",
                i, check.check_type
            ));
        }
    }

    // exactly one of gate, check, auto_record
    let condition_count = [hook.gate.is_some(), hook.check.is_some(), hook.auto_record.is_some()]
        .iter().filter(|&&x| x).count();
    if condition_count != 1 {
        errors.push(format!(
            "hooks.toml: hook {} must have exactly one of 'gate', 'check', or 'auto_record' (has {})",
            i, condition_count
        ));
    }
}

// 15. Monitor validation.
{
    let mut monitor_names = HashSet::new();
    for monitor in &self.monitors {
        if !monitor_names.insert(&monitor.name) {
            errors.push(format!(
                "hooks.toml: duplicate monitor name '{}'",
                monitor.name
            ));
        }
        if monitor.action != "warn" {
            errors.push(format!(
                "hooks.toml: monitor '{}' action must be 'warn' (got '{}')",
                monitor.name, monitor.action
            ));
        }
        if let Some(ref states) = monitor.states {
            for s in states {
                if !self.states.contains_key(s) {
                    errors.push(format!(
                        "hooks.toml: monitor '{}' references unknown state '{}'",
                        monitor.name, s
                    ));
                }
            }
        }
        let known_triggers = ["event_count_since_last_transition"];
        if !known_triggers.contains(&monitor.trigger.trigger_type.as_str()) {
            errors.push(format!(
                "hooks.toml: monitor '{}' has unknown trigger type '{}'",
                monitor.name, monitor.trigger.trigger_type
            ));
        }
    }
}

// 16. Write-gated guards validation.
if let Some(ref guards) = self.guards {
    for wg in &guards.write_gated {
        for s in &wg.writable_in {
            if !self.states.contains_key(s) {
                errors.push(format!(
                    "protocol.toml: write_gated path '{}' references unknown state '{}'",
                    wg.path, s
                ));
            }
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test test_validate_hook_states test_validate_auto_record test_validate_monitor_names test_validate_write_gated -- --nocapture`
Expected: All PASS

- [ ] **Step 5: Commit**

```bash
git add src/config/mod.rs tests/config_tests.rs
git commit -m "feat: validate hooks, monitors, and write_gated guards in validate_deep"
```

---

### Task 5: Gate Enhancement — Required `since` Parameter

**Files:**
- Modify: `src/gates/ledger.rs`
- Modify: `src/config/mod.rs` (validate_deep known_gates)
- Modify: `tests/gate_tests.rs`

- [ ] **Step 1: Write failing test for since_event behavior**

Add to `tests/gate_tests.rs`:

```rust
#[test]
fn test_ledger_has_event_since_custom_event() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Sequence: transition → fix_commit → failing_test
    // The gate should find "failing_test" since last "fix_commit"
    let mut trans_fields = BTreeMap::new();
    trans_fields.insert("from".to_string(), "idle".to_string());
    trans_fields.insert("to".to_string(), "working".to_string());
    trans_fields.insert("command".to_string(), "begin".to_string());
    ledger.append("state_transition", trans_fields).unwrap();
    ledger.append("fix_commit", BTreeMap::new()).unwrap();
    ledger.append("failing_test", BTreeMap::new()).unwrap();

    let gate = make_gate(
        "ledger_has_event_since",
        vec![
            ("event", toml::Value::String("failing_test".to_string())),
            ("since", toml::Value::String("fix_commit".to_string())),
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
fn test_ledger_has_event_since_custom_event_fail() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Sequence: transition → failing_test → fix_commit
    // The gate should NOT find "failing_test" since last "fix_commit"
    let mut trans_fields = BTreeMap::new();
    trans_fields.insert("from".to_string(), "idle".to_string());
    trans_fields.insert("to".to_string(), "working".to_string());
    trans_fields.insert("command".to_string(), "begin".to_string());
    ledger.append("state_transition", trans_fields).unwrap();
    ledger.append("failing_test", BTreeMap::new()).unwrap();
    ledger.append("fix_commit", BTreeMap::new()).unwrap();

    let gate = make_gate(
        "ledger_has_event_since",
        vec![
            ("event", toml::Value::String("failing_test".to_string())),
            ("since", toml::Value::String("fix_commit".to_string())),
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

#[test]
fn test_ledger_has_event_since_custom_event_fallback_to_transition() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // No "fix_commit" exists — should fall back to last transition
    let mut trans_fields = BTreeMap::new();
    trans_fields.insert("from".to_string(), "idle".to_string());
    trans_fields.insert("to".to_string(), "working".to_string());
    trans_fields.insert("command".to_string(), "begin".to_string());
    ledger.append("state_transition", trans_fields).unwrap();
    ledger.append("failing_test", BTreeMap::new()).unwrap();

    let gate = make_gate(
        "ledger_has_event_since",
        vec![
            ("event", toml::Value::String("failing_test".to_string())),
            ("since", toml::Value::String("fix_commit".to_string())),
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
    // fix_commit doesn't exist, falls back to last_transition — failing_test IS after transition
    assert!(evaluate_gate(&gate, &ctx).passed);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_ledger_has_event_since_custom -- --nocapture`
Expected: FAIL — current impl ignores `since` value and always uses last_transition

- [ ] **Step 3: Update `eval_ledger_has_event_since` in `src/gates/ledger.rs`**

Replace the existing function (lines 75-122):

```rust
pub(super) fn eval_ledger_has_event_since(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let event = gate
        .params
        .get("event")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let since = gate
        .params
        .get("since")
        .and_then(|v| v.as_str())
        .unwrap_or("last_transition");

    // Determine the sequence threshold based on the `since` parameter.
    let threshold_seq = if since == "last_transition" {
        // Since last state_transition event.
        ctx.ledger
            .events_of_type("state_transition")
            .last()
            .map(|e| e.seq)
            .unwrap_or(0)
    } else {
        // Since last event of the specified type, falling back to last transition.
        let since_event_seq = ctx
            .ledger
            .entries()
            .iter()
            .rev()
            .find(|e| e.event_type == since)
            .map(|e| e.seq);

        match since_event_seq {
            Some(seq) => seq,
            None => {
                // Fallback: since last state_transition
                ctx.ledger
                    .events_of_type("state_transition")
                    .last()
                    .map(|e| e.seq)
                    .unwrap_or(0)
            }
        }
    };

    let found = ctx
        .ledger
        .entries()
        .iter()
        .any(|e| e.event_type == event && e.seq > threshold_seq);

    let since_desc = if since == "last_transition" {
        "last state_transition".to_string()
    } else {
        format!("last '{}' event", since)
    };

    GateResult {
        passed: found,
        evaluable: true,
        gate_type: "ledger_has_event_since".to_string(),
        description: format!("'{}' event exists since {}", event, since_desc),
        reason: if found {
            None
        } else {
            Some(format!(
                "no '{}' event found after {}",
                event, since_desc
            ))
        },
        intent: None,
        attestation: None,
    }
}
```

- [ ] **Step 4: Add `since` to required params in `validate_deep`**

In `src/config/mod.rs`, find the `known_gates` map in `validate_deep` and change:

```rust
("ledger_has_event_since", vec!["event"]),
```

to:

```rust
("ledger_has_event_since", vec!["event", "since"]),
```

- [ ] **Step 5: Run new tests**

Run: `cargo test test_ledger_has_event_since_custom -- --nocapture`
Expected: All 3 PASS

- [ ] **Step 6: Run full test suite to check regressions**

Run: `cargo test`

The existing `test_ledger_has_event_since_pass` and `test_ledger_has_event_since_fail` already pass `since = "last_transition"` in their gate config so they should still pass. Verify no regressions.

- [ ] **Step 7: Commit**

```bash
git add src/gates/ledger.rs src/config/mod.rs tests/gate_tests.rs
git commit -m "feat: make 'since' required on ledger_has_event_since, add since_event support"
```

---

### Task 6: Config Seals — Add `hooks.toml` as 6th Hash

**Files:**
- Modify: `src/config/mod.rs` (`compute_config_seals`)

- [ ] **Step 1: Write failing test**

Add to `tests/config_integrity_tests.rs`:

```rust
#[test]
fn test_config_seals_include_hooks_toml() {
    use sahjhan::config::compute_config_seals;
    let dir = std::path::Path::new("examples/minimal");
    let seals = compute_config_seals(dir);
    assert!(seals.contains_key("config_seal_hooks"),
        "Seals should include hooks.toml hash. Keys: {:?}", seals.keys().collect::<Vec<_>>());
    // minimal example has no hooks.toml — should hash as empty bytes
    // SHA-256 of empty string
    assert_eq!(seals["config_seal_hooks"],
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_config_seals_include_hooks_toml -- --nocapture`
Expected: FAIL — no `config_seal_hooks` key

- [ ] **Step 3: Add hooks.toml to `compute_config_seals` in `src/config/mod.rs`**

In the `files` array, add:

```rust
let files = [
    ("config_seal_protocol", "protocol.toml"),
    ("config_seal_states", "states.toml"),
    ("config_seal_transitions", "transitions.toml"),
    ("config_seal_events", "events.toml"),
    ("config_seal_renders", "renders.toml"),
    ("config_seal_hooks", "hooks.toml"),
];
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test test_config_seals_include_hooks_toml -- --nocapture`
Expected: PASS

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: All pass. Existing config_integrity_tests may need updating if they assert exact seal count or keys. Fix any regressions.

- [ ] **Step 6: Commit**

```bash
git add src/config/mod.rs tests/config_integrity_tests.rs
git commit -m "feat: include hooks.toml in config seal computation (6th hash)"
```

---

### Task 7: Hook Evaluation Engine

**Files:**
- Create: `src/hooks/eval.rs`
- Modify: `src/hooks/mod.rs`

- [ ] **Step 1: Write failing test for basic hook evaluation**

Create `tests/hook_eval_tests.rs`:

```rust
use std::collections::{BTreeMap, HashMap};
use std::path::Path;
use tempfile::tempdir;

use sahjhan::config::ProtocolConfig;
use sahjhan::config::hooks::{HookConfig, HookEvent, HookFilter, AutoRecordConfig, MonitorConfig, MonitorTrigger};
use sahjhan::hooks::eval::{HookEvalRequest, HookEvalResult, evaluate_hooks};
use sahjhan::ledger::chain::Ledger;

fn setup_ledger_in_state(dir: &std::path::Path, state: &str) -> Ledger {
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

#[test]
fn test_hook_eval_no_hooks_allows() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
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
}

#[test]
fn test_hook_eval_gate_blocks_when_condition_not_met() {
    let dir = tempdir().unwrap();
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger = setup_ledger_in_state(dir.path(), "working");

    // Add a hook: PreToolUse Edit in state "working" requires "check_done" event since last transition
    config.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        tools: Some(vec!["Edit".to_string()]),
        states: Some(vec!["working".to_string()]),
        states_not: None,
        action: Some("block".to_string()),
        message: Some("Must complete check first.".to_string()),
        gate: Some(sahjhan::config::GateConfig {
            gate_type: "ledger_has_event_since".to_string(),
            intent: None,
            gates: vec![],
            params: {
                let mut p = HashMap::new();
                p.insert("event".to_string(), toml::Value::String("check_done".to_string()));
                p.insert("since".to_string(), toml::Value::String("last_transition".to_string()));
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
    assert!(result.messages[0].message.contains("Must complete check"));
}

#[test]
fn test_hook_eval_gate_allows_when_condition_met() {
    let dir = tempdir().unwrap();
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let mut ledger = setup_ledger_in_state(dir.path(), "working");

    // Record the required event
    ledger.append("check_done", BTreeMap::new()).unwrap();

    config.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        tools: Some(vec!["Edit".to_string()]),
        states: Some(vec!["working".to_string()]),
        states_not: None,
        action: Some("block".to_string()),
        message: Some("Must complete check first.".to_string()),
        gate: Some(sahjhan::config::GateConfig {
            gate_type: "ledger_has_event_since".to_string(),
            intent: None,
            gates: vec![],
            params: {
                let mut p = HashMap::new();
                p.insert("event".to_string(), toml::Value::String("check_done".to_string()));
                p.insert("since".to_string(), toml::Value::String("last_transition".to_string()));
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
}

#[test]
fn test_hook_eval_filter_excludes_test_files() {
    let dir = tempdir().unwrap();
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger = setup_ledger_in_state(dir.path(), "working");

    // Hook blocks Edit in working state, but only for non-test files
    config.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        tools: Some(vec!["Edit".to_string()]),
        states: Some(vec!["working".to_string()]),
        states_not: None,
        action: Some("block".to_string()),
        message: Some("Blocked.".to_string()),
        gate: Some(sahjhan::config::GateConfig {
            gate_type: "ledger_has_event_since".to_string(),
            intent: None,
            gates: vec![],
            params: {
                let mut p = HashMap::new();
                p.insert("event".to_string(), toml::Value::String("check_done".to_string()));
                p.insert("since".to_string(), toml::Value::String("last_transition".to_string()));
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

    // Editing a test file should be allowed (filter excludes it)
    let request = HookEvalRequest {
        event: HookEvent::PreToolUse,
        tool: Some("Edit".to_string()),
        file: Some("tests/test_thing.rs".to_string()),
        output_text: None,
    };
    let result = evaluate_hooks(&config, &ledger, &request, dir.path());
    assert_eq!(result.decision, "allow");
}

#[test]
fn test_hook_eval_state_filtering() {
    let dir = tempdir().unwrap();
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger = setup_ledger_in_state(dir.path(), "idle"); // NOT "working"

    // Hook only active in "working" state
    config.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        tools: Some(vec!["Edit".to_string()]),
        states: Some(vec!["working".to_string()]),
        states_not: None,
        action: Some("block".to_string()),
        message: Some("Blocked in working.".to_string()),
        gate: None,
        check: Some(sahjhan::config::hooks::HookCheck {
            check_type: "event_count_since_last_transition".to_string(),
            sql: None,
            compare: None,
            threshold: Some(0),
            patterns: None,
        }),
        auto_record: None,
        filter: None,
    });

    let request = HookEvalRequest {
        event: HookEvent::PreToolUse,
        tool: Some("Edit".to_string()),
        file: None,
        output_text: None,
    };
    let result = evaluate_hooks(&config, &ledger, &request, dir.path());
    assert_eq!(result.decision, "allow", "Hook should not fire in idle state");
}

#[test]
fn test_hook_eval_monitor_warning() {
    let dir = tempdir().unwrap();
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let mut ledger = setup_ledger_in_state(dir.path(), "working");

    // Record enough events to trigger the monitor
    for _ in 0..5 {
        ledger.append("some_event", BTreeMap::new()).unwrap();
    }

    config.monitors.push(MonitorConfig {
        name: "stall".to_string(),
        states: Some(vec!["working".to_string()]),
        action: "warn".to_string(),
        message: "Too many events without transition.".to_string(),
        trigger: MonitorTrigger {
            trigger_type: "event_count_since_last_transition".to_string(),
            threshold: 3,
        },
    });

    let request = HookEvalRequest {
        event: HookEvent::PreToolUse,
        tool: Some("Edit".to_string()),
        file: None,
        output_text: None,
    };
    let result = evaluate_hooks(&config, &ledger, &request, dir.path());
    assert_eq!(result.monitor_warnings.len(), 1);
    assert_eq!(result.monitor_warnings[0].name, "stall");
}

#[test]
fn test_hook_eval_write_gated_blocks() {
    let dir = tempdir().unwrap();
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger = setup_ledger_in_state(dir.path(), "working");

    config.guards = Some(sahjhan::config::GuardsConfig {
        read_blocked: vec![],
        write_gated: vec![sahjhan::config::protocol::WriteGatedConfig {
            path: "docs/SUMMARY.md".to_string(),
            writable_in: vec!["done".to_string()],
            message: "SUMMARY.md only writable in done state. Current: {current_state}.".to_string(),
        }],
    });

    let request = HookEvalRequest {
        event: HookEvent::PreToolUse,
        tool: Some("Edit".to_string()),
        file: Some("docs/SUMMARY.md".to_string()),
        output_text: None,
    };
    let result = evaluate_hooks(&config, &ledger, &request, dir.path());
    assert_eq!(result.decision, "block");
    assert!(result.messages[0].message.contains("working"));
}

#[test]
fn test_hook_eval_stop_output_pattern() {
    let dir = tempdir().unwrap();
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger = setup_ledger_in_state(dir.path(), "working");

    config.hooks.push(HookConfig {
        event: HookEvent::Stop,
        tools: None,
        states: None,
        states_not: Some(vec!["done".to_string()]),
        action: Some("block".to_string()),
        message: Some("Premature completion claim in state {current_state}.".to_string()),
        gate: None,
        check: Some(sahjhan::config::hooks::HookCheck {
            check_type: "output_contains_any".to_string(),
            sql: None,
            compare: None,
            threshold: None,
            patterns: Some(vec!["audit complete".to_string(), "all done".to_string()]),
        }),
        auto_record: None,
        filter: None,
    });

    let request = HookEvalRequest {
        event: HookEvent::Stop,
        tool: None,
        file: None,
        output_text: Some("The audit complete and everything looks good.".to_string()),
    };
    let result = evaluate_hooks(&config, &ledger, &request, dir.path());
    assert_eq!(result.decision, "block");
    assert!(result.messages[0].message.contains("working"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test hook_eval_tests -- --nocapture`
Expected: compile error — `hooks::eval` module doesn't exist

- [ ] **Step 3: Create `src/hooks/eval.rs`**

```rust
// src/hooks/eval.rs
//
// Runtime hook evaluation engine.
//
// ## Index
// - HookEvalRequest         — inputs from the harness
// - HookEvalResult          — combined evaluation result
// - HookMessage             — single triggered hook/guard message
// - AutoRecordResult        — event auto-recorded to ledger
// - MonitorWarning          — triggered monitor warning
// - evaluate_hooks          — evaluate all hooks, guards, monitors
// - [eval-write-gated]      — check write_gated guards
// - [eval-hook-condition]   — evaluate a single hook's gate/check
// - [eval-monitors]         — evaluate all monitors
// - [matches-filter]        — check path against hook filter
// - [derive-current-state]  — derive current state from ledger

use std::collections::HashMap;
use std::path::Path;

use crate::config::hooks::{HookConfig, HookEvent, HookFilter};
use crate::config::ProtocolConfig;
use crate::gates::evaluator::{evaluate_gate, GateContext};
use crate::ledger::chain::Ledger;

use serde::Serialize;

/// Inputs from the harness for hook evaluation.
#[derive(Debug)]
pub struct HookEvalRequest {
    pub event: HookEvent,
    pub tool: Option<String>,
    pub file: Option<String>,
    pub output_text: Option<String>,
}

/// Combined hook evaluation result.
#[derive(Debug, Serialize)]
pub struct HookEvalResult {
    pub decision: String,
    pub messages: Vec<HookMessage>,
    pub auto_records: Vec<AutoRecordResult>,
    pub monitor_warnings: Vec<MonitorWarning>,
}

/// A single triggered hook or guard message.
#[derive(Debug, Serialize)]
pub struct HookMessage {
    pub source: String,
    pub rule_index: usize,
    pub action: String,
    pub message: String,
}

/// An event auto-recorded to the ledger.
#[derive(Debug, Serialize)]
pub struct AutoRecordResult {
    pub event_type: String,
    pub fields: HashMap<String, String>,
}

/// A triggered monitor warning.
#[derive(Debug, Serialize)]
pub struct MonitorWarning {
    pub name: String,
    pub message: String,
}

/// Evaluate all hooks, write-gated guards, and monitors against the current state.
pub fn evaluate_hooks(
    config: &ProtocolConfig,
    ledger: &Ledger,
    request: &HookEvalRequest,
    working_dir: &Path,
) -> HookEvalResult {
    let mut messages = Vec::new();
    let mut auto_records = Vec::new();

    // Derive current state from ledger.
    let current_state = derive_current_state(config, ledger);

    // 1. Managed path checks (PreToolUse Edit/Write only).
    if request.event == HookEvent::PreToolUse {
        if let Some(ref tool) = request.tool {
            if tool == "Edit" || tool == "Write" {
                eval_managed_paths(config, request, &mut messages);
                eval_write_gated(config, &current_state, request, &mut messages);
            }
        }
    }

    // 2. Evaluate matching hooks.
    for (i, hook) in config.hooks.iter().enumerate() {
        if !hook_matches(hook, request, &current_state) {
            continue;
        }

        if let Some(ref ar) = hook.auto_record {
            // Auto-record: resolve field templates and record.
            let mut fields = HashMap::new();
            for (k, v) in &ar.fields {
                let resolved = resolve_tool_template(v, request);
                fields.insert(k.clone(), resolved);
            }
            auto_records.push(AutoRecordResult {
                event_type: ar.event_type.clone(),
                fields,
            });
            continue;
        }

        // Evaluate gate or check condition.
        let condition_failed = eval_hook_condition(hook, config, ledger, &current_state, request, working_dir);

        if condition_failed {
            let msg = interpolate_message(
                hook.message.as_deref().unwrap_or(""),
                &current_state,
                None,
            );
            messages.push(HookMessage {
                source: "hook".to_string(),
                rule_index: i,
                action: hook.action.as_deref().unwrap_or("block").to_string(),
                message: msg,
            });
        }
    }

    // 3. Monitors.
    let monitor_warnings = eval_monitors(config, ledger, &current_state);

    // Compute overall decision: block > warn > allow.
    let decision = if messages.iter().any(|m| m.action == "block") {
        "block".to_string()
    } else if !messages.is_empty() || !monitor_warnings.is_empty() {
        "warn".to_string()
    } else {
        "allow".to_string()
    };

    HookEvalResult {
        decision,
        messages,
        auto_records,
        monitor_warnings,
    }
}

// [derive-current-state]
fn derive_current_state(config: &ProtocolConfig, ledger: &Ledger) -> String {
    // Find last state_transition event.
    ledger
        .events_of_type("state_transition")
        .last()
        .and_then(|e| e.fields.get("to").cloned())
        .unwrap_or_else(|| {
            config
                .initial_state()
                .unwrap_or("unknown")
                .to_string()
        })
}

/// Check if a hook matches the current request context.
fn hook_matches(hook: &HookConfig, request: &HookEvalRequest, current_state: &str) -> bool {
    // Event must match.
    if hook.event != request.event {
        return false;
    }

    // Tool filter.
    if let Some(ref tools) = hook.tools {
        match &request.tool {
            Some(tool) if tools.contains(tool) => {}
            _ => return false,
        }
    }

    // State filter.
    if let Some(ref states) = hook.states {
        if !states.iter().any(|s| s == current_state) {
            return false;
        }
    }
    if let Some(ref states_not) = hook.states_not {
        if states_not.iter().any(|s| s == current_state) {
            return false;
        }
    }

    // Path filter.
    if let Some(ref filter) = hook.filter {
        if !matches_filter(filter, request) {
            return false;
        }
    }

    true
}

// [matches-filter]
fn matches_filter(filter: &HookFilter, request: &HookEvalRequest) -> bool {
    let file = match &request.file {
        Some(f) => f,
        None => return true, // No file to filter on — hook applies.
    };

    if let Some(ref pattern) = filter.path_matches {
        if !glob_match(pattern, file) {
            return false;
        }
    }
    if let Some(ref pattern) = filter.path_not_matches {
        if glob_match(pattern, file) {
            return false; // File matches the exclusion pattern — skip this hook.
        }
    }

    true
}

/// Simple glob matching: supports `*` (single segment) and `**` (any depth).
fn glob_match(pattern: &str, path: &str) -> bool {
    // Use a simple approach: convert glob to regex-like matching.
    // For now, handle common patterns: "tests/*", "src/**/*.rs"
    let pattern_parts: Vec<&str> = pattern.split('/').collect();
    let path_parts: Vec<&str> = path.split('/').collect();

    glob_match_parts(&pattern_parts, &path_parts)
}

fn glob_match_parts(pattern: &[&str], path: &[&str]) -> bool {
    if pattern.is_empty() && path.is_empty() {
        return true;
    }
    if pattern.is_empty() {
        return false;
    }

    if pattern[0] == "**" {
        // ** matches zero or more path segments.
        for i in 0..=path.len() {
            if glob_match_parts(&pattern[1..], &path[i..]) {
                return true;
            }
        }
        return false;
    }

    if path.is_empty() {
        return false;
    }

    // Match single segment: `*` matches any single segment.
    let seg_matches = if pattern[0] == "*" {
        true
    } else if pattern[0].contains('*') {
        // Pattern like "*.rs" — simple wildcard within segment.
        let parts: Vec<&str> = pattern[0].splitn(2, '*').collect();
        if parts.len() == 2 {
            path[0].starts_with(parts[0]) && path[0].ends_with(parts[1])
        } else {
            pattern[0] == path[0]
        }
    } else {
        pattern[0] == path[0]
    };

    if seg_matches {
        glob_match_parts(&pattern[1..], &path[1..])
    } else {
        false
    }
}

/// Check if file is in a managed path (always-blocked for direct writes).
fn eval_managed_paths(
    config: &ProtocolConfig,
    request: &HookEvalRequest,
    messages: &mut Vec<HookMessage>,
) {
    let file = match &request.file {
        Some(f) => f,
        None => return,
    };

    for managed in &config.paths.managed {
        if file.starts_with(managed) || file == managed.as_str() {
            messages.push(HookMessage {
                source: "managed_path".to_string(),
                rule_index: 0,
                action: "block".to_string(),
                message: format!(
                    "WRITE BLOCKED: {} is managed by sahjhan. Use CLI commands to modify protocol state.",
                    file
                ),
            });
            return;
        }
    }
}

// [eval-write-gated]
fn eval_write_gated(
    config: &ProtocolConfig,
    current_state: &str,
    request: &HookEvalRequest,
    messages: &mut Vec<HookMessage>,
) {
    let guards = match &config.guards {
        Some(g) => g,
        None => return,
    };

    let file = match &request.file {
        Some(f) => f,
        None => return,
    };

    for (i, wg) in guards.write_gated.iter().enumerate() {
        if glob_match(&wg.path, file) && !wg.writable_in.contains(&current_state.to_string()) {
            let msg = interpolate_message(&wg.message, current_state, None);
            messages.push(HookMessage {
                source: "write_gated".to_string(),
                rule_index: i,
                action: "block".to_string(),
                message: msg,
            });
        }
    }
}

// [eval-hook-condition]
/// Returns true if the hook condition is NOT met (i.e., the hook should fire/block).
fn eval_hook_condition(
    hook: &HookConfig,
    config: &ProtocolConfig,
    ledger: &Ledger,
    current_state: &str,
    request: &HookEvalRequest,
    working_dir: &Path,
) -> bool {
    if let Some(ref gate) = hook.gate {
        // Gate-based: hook fires when gate FAILS.
        let ctx = GateContext {
            ledger,
            config,
            current_state,
            state_params: HashMap::new(),
            working_dir: working_dir.to_path_buf(),
            event_fields: None,
        };
        let result = evaluate_gate(gate, &ctx);
        return !result.passed;
    }

    if let Some(ref check) = hook.check {
        match check.check_type.as_str() {
            "output_contains_any" => {
                if let Some(ref patterns) = check.patterns {
                    if let Some(ref output) = request.output_text {
                        let output_lower = output.to_lowercase();
                        return patterns.iter().any(|p| output_lower.contains(&p.to_lowercase()));
                    }
                }
                return false;
            }
            "event_count_since_last_transition" => {
                let threshold = check.threshold.unwrap_or(0) as u64;
                let last_transition_seq = ledger
                    .events_of_type("state_transition")
                    .last()
                    .map(|e| e.seq)
                    .unwrap_or(0);
                let count = ledger
                    .entries()
                    .iter()
                    .filter(|e| e.seq > last_transition_seq)
                    .count() as u64;
                let compare = check.compare.as_deref().unwrap_or("gte");
                return compare_threshold(count, threshold, compare);
            }
            "query" => {
                // SQL query check — use DataFusion.
                // For now, a simplified version that counts events.
                // Full implementation would use the query engine.
                if let Some(ref _sql) = check.sql {
                    // TODO: integrate with query engine in a future iteration.
                    // For now, fall through to false (don't block).
                }
                return false;
            }
            _ => return false,
        }
    }

    false
}

fn compare_threshold(count: u64, threshold: u64, compare: &str) -> bool {
    match compare {
        "gte" => count >= threshold,
        "gt" => count > threshold,
        "lte" => count <= threshold,
        "lt" => count < threshold,
        "eq" => count == threshold,
        _ => false,
    }
}

// [eval-monitors]
fn eval_monitors(
    config: &ProtocolConfig,
    ledger: &Ledger,
    current_state: &str,
) -> Vec<MonitorWarning> {
    let mut warnings = Vec::new();

    for monitor in &config.monitors {
        // State filter.
        if let Some(ref states) = monitor.states {
            if !states.iter().any(|s| s == current_state) {
                continue;
            }
        }

        match monitor.trigger.trigger_type.as_str() {
            "event_count_since_last_transition" => {
                let last_transition_seq = ledger
                    .events_of_type("state_transition")
                    .last()
                    .map(|e| e.seq)
                    .unwrap_or(0);
                let count = ledger
                    .entries()
                    .iter()
                    .filter(|e| e.seq > last_transition_seq)
                    .count() as u64;

                if count >= monitor.trigger.threshold {
                    let msg = interpolate_message(&monitor.message, current_state, Some(count));
                    warnings.push(MonitorWarning {
                        name: monitor.name.clone(),
                        message: msg,
                    });
                }
            }
            _ => {}
        }
    }

    warnings
}

fn interpolate_message(template: &str, current_state: &str, count: Option<u64>) -> String {
    let mut msg = template.replace("{current_state}", current_state);
    if let Some(c) = count {
        msg = msg.replace("{count}", &c.to_string());
    }
    msg
}

fn resolve_tool_template(template: &str, request: &HookEvalRequest) -> String {
    let mut result = template.to_string();
    if let Some(ref file) = request.file {
        result = result.replace("{tool.file_path}", file);
    }
    result
}
```

- [ ] **Step 4: Update `src/hooks/mod.rs`**

```rust
// src/hooks/mod.rs
//
// Hook bridge generation and runtime evaluation for Claude Code integration.

pub mod eval;
pub mod generate;

pub use generate::{GeneratedHook, HookGenerator};
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test hook_eval_tests -- --nocapture`
Expected: All PASS

- [ ] **Step 6: Commit**

```bash
git add src/hooks/eval.rs src/hooks/mod.rs tests/hook_eval_tests.rs
git commit -m "feat: add hook evaluation engine with gate, check, filter, monitor support"
```

---

### Task 8: CLI `hook eval` Command and Output Types

**Files:**
- Modify: `src/main.rs`
- Modify: `src/cli/hooks_cmd.rs`
- Modify: `src/cli/output.rs`

- [ ] **Step 1: Write failing CLI integration test**

Add to `tests/hook_eval_tests.rs`:

```rust
#[test]
fn test_hook_eval_cli_no_hooks_allows() {
    // Use the minimal example which has no hooks.toml
    let dir = tempfile::tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();

    // Copy minimal example config files
    for file in &["protocol.toml", "states.toml", "transitions.toml", "events.toml"] {
        let src = std::path::Path::new("examples/minimal").join(file);
        if src.exists() {
            std::fs::copy(&src, config_dir.join(file)).unwrap();
        }
    }

    // Initialize ledger
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sahjhan"))
        .args(["--config-dir", config_dir.to_str().unwrap(), "init"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "init failed: {}", String::from_utf8_lossy(&output.stderr));

    // Run hook eval
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_sahjhan"))
        .args([
            "--config-dir", config_dir.to_str().unwrap(),
            "hook", "eval",
            "--event", "PreToolUse",
            "--tool", "Edit",
            "--file", "src/main.rs",
        ])
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success());

    let stdout = String::from_utf8_lossy(&output.stdout);
    let result: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(result["decision"], "allow");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_hook_eval_cli_no_hooks_allows -- --nocapture`
Expected: FAIL — no `eval` subcommand on `hook`

- [ ] **Step 3: Add `HookEvalData` to `src/cli/output.rs`**

Add after the existing data structs:

```rust
/// Hook evaluation result data.
#[derive(Debug, Serialize, Clone)]
pub struct HookEvalData {
    pub decision: String,
    pub messages: Vec<HookEvalMessage>,
    pub auto_records: Vec<HookAutoRecord>,
    pub monitor_warnings: Vec<HookMonitorWarning>,
}

#[derive(Debug, Serialize, Clone)]
pub struct HookEvalMessage {
    pub source: String,
    pub rule_index: usize,
    pub action: String,
    pub message: String,
}

#[derive(Debug, Serialize, Clone)]
pub struct HookAutoRecord {
    pub event_type: String,
    pub fields: std::collections::HashMap<String, String>,
}

#[derive(Debug, Serialize, Clone)]
pub struct HookMonitorWarning {
    pub name: String,
    pub message: String,
}

impl Display for HookEvalData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Always output JSON for hook eval (machine interface).
        write!(f, "{}", serde_json::to_string_pretty(self).unwrap_or_default())
    }
}
```

- [ ] **Step 4: Add `Eval` variant to `HookAction` in `src/main.rs`**

```rust
#[derive(Subcommand)]
enum HookAction {
    /// Generate hook scripts
    Generate {
        #[arg(long)]
        harness: Option<String>,
        #[arg(long)]
        output_dir: Option<String>,
    },
    /// Evaluate hook rules against current state
    Eval {
        /// Hook event type (PreToolUse, PostToolUse, Stop)
        #[arg(long)]
        event: String,
        /// Tool name
        #[arg(long)]
        tool: Option<String>,
        /// File path being operated on
        #[arg(long)]
        file: Option<String>,
        /// Agent output text (for Stop hooks)
        #[arg(long)]
        output_text: Option<String>,
    },
}
```

Add dispatch in the `Commands::Hook { action }` match arm:

```rust
Commands::Hook { action } => match action {
    HookAction::Generate { harness, output_dir } => {
        let code = hooks_cmd::cmd_hook_generate(&cli.config_dir, &harness, &output_dir);
        Box::new(LegacyResult::new("hook_generate", code))
    }
    HookAction::Eval { event, tool, file, output_text } => {
        hooks_cmd::cmd_hook_eval(&cli.config_dir, &event, &tool, &file, &output_text, &targeting)
    }
},
```

- [ ] **Step 5: Implement `cmd_hook_eval` in `src/cli/hooks_cmd.rs`**

```rust
use super::commands::{load_config, resolve_config_dir, resolve_data_dir, LedgerTargeting, EXIT_SUCCESS, EXIT_GATE_FAILED};
use super::output::{CommandOutput, CommandResult, HookEvalData, HookEvalMessage, HookAutoRecord, HookMonitorWarning};
use crate::config::hooks::HookEvent;
use crate::hooks::eval::{HookEvalRequest, evaluate_hooks};
use crate::ledger::chain::Ledger;

pub fn cmd_hook_eval(
    config_dir: &str,
    event: &str,
    tool: &Option<String>,
    file: &Option<String>,
    output_text: &Option<String>,
    targeting: &LedgerTargeting,
) -> Box<dyn CommandOutput> {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((_code, _msg)) => {
            // If config can't load, allow by default (don't block the agent on config errors)
            let data = HookEvalData {
                decision: "allow".to_string(),
                messages: vec![],
                auto_records: vec![],
                monitor_warnings: vec![],
            };
            return Box::new(CommandResult::ok("hook_eval", data));
        }
    };

    let hook_event = match event {
        "PreToolUse" => HookEvent::PreToolUse,
        "PostToolUse" => HookEvent::PostToolUse,
        "Stop" => HookEvent::Stop,
        _ => {
            let data = HookEvalData {
                decision: "allow".to_string(),
                messages: vec![],
                auto_records: vec![],
                monitor_warnings: vec![],
            };
            return Box::new(CommandResult::ok("hook_eval", data));
        }
    };

    // Open ledger (best-effort — if no ledger exists, allow).
    let data_path = resolve_data_dir(&config.paths.data_dir);
    let ledger_file = data_path.join("ledger.jsonl");
    let ledger = match Ledger::open(&ledger_file) {
        Ok(l) => l,
        Err(_) => {
            // Try targeted ledger
            match super::commands::open_targeted_ledger(&config, targeting) {
                Ok(l) => l,
                Err(_) => {
                    let data = HookEvalData {
                        decision: "allow".to_string(),
                        messages: vec![],
                        auto_records: vec![],
                        monitor_warnings: vec![],
                    };
                    return Box::new(CommandResult::ok("hook_eval", data));
                }
            }
        }
    };

    let request = HookEvalRequest {
        event: hook_event,
        tool: tool.clone(),
        file: file.clone(),
        output_text: output_text.clone(),
    };

    let working_dir = std::env::current_dir().unwrap_or_default();
    let result = evaluate_hooks(&config, &ledger, &request, &working_dir);

    // Auto-record events to ledger.
    // We need a mutable ledger for this.
    if !result.auto_records.is_empty() {
        if let Ok(mut ledger_mut) = Ledger::open(&ledger_file) {
            for ar in &result.auto_records {
                let fields: std::collections::BTreeMap<String, String> =
                    ar.fields.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
                let _ = ledger_mut.append(&ar.event_type, fields);
            }
        }
    }

    let exit_code = if result.decision == "block" {
        EXIT_GATE_FAILED
    } else {
        EXIT_SUCCESS
    };

    let data = HookEvalData {
        decision: result.decision,
        messages: result.messages.into_iter().map(|m| HookEvalMessage {
            source: m.source,
            rule_index: m.rule_index,
            action: m.action,
            message: m.message,
        }).collect(),
        auto_records: result.auto_records.into_iter().map(|a| HookAutoRecord {
            event_type: a.event_type,
            fields: a.fields,
        }).collect(),
        monitor_warnings: result.monitor_warnings.into_iter().map(|w| HookMonitorWarning {
            name: w.name,
            message: w.message,
        }).collect(),
    };

    Box::new(CommandResult::ok_with_exit_code("hook_eval", data, exit_code))
}
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test test_hook_eval_cli_no_hooks_allows -- --nocapture`
Expected: PASS

- [ ] **Step 7: Run full test suite**

Run: `cargo test`
Expected: All pass

- [ ] **Step 8: Commit**

```bash
git add src/main.rs src/cli/hooks_cmd.rs src/cli/output.rs tests/hook_eval_tests.rs
git commit -m "feat: add 'sahjhan hook eval' CLI command with JSON output"
```

---

### Task 9: Hook Generator Updates — Thin Wrapper Scripts

**Files:**
- Modify: `src/hooks/generate.rs`
- Modify: `tests/hook_generation_tests.rs`

- [ ] **Step 1: Write failing test for new generated scripts**

Add to `tests/hook_generation_tests.rs`:

```rust
#[test]
fn hook_generation_produces_thin_wrappers() {
    let gen = HookGenerator::new().unwrap();
    let config = make_config(vec!["output"]);
    let hooks = gen.generate(&config, "cc", None).unwrap();

    // Should now produce 4 hooks: pre_tool_hook, post_tool_hook, stop_hook, bootstrap
    assert_eq!(hooks.len(), 4, "Expected 4 hooks, got: {:?}",
        hooks.iter().map(|h| &h.filename).collect::<Vec<_>>());

    let pre = hooks.iter().find(|h| h.filename == "pre_tool_hook.py").unwrap();
    assert!(pre.content.contains("hook"), "pre_tool_hook should delegate to sahjhan hook eval");
    assert!(pre.content.contains("PreToolUse"));
    assert_eq!(pre.hook_type, "PreToolUse");

    let post = hooks.iter().find(|h| h.filename == "post_tool_hook.py").unwrap();
    assert!(post.content.contains("hook"), "post_tool_hook should delegate to sahjhan hook eval");
    assert!(post.content.contains("PostToolUse"));
    assert_eq!(post.hook_type, "PostToolUse");

    let stop = hooks.iter().find(|h| h.filename == "stop_hook.py").unwrap();
    assert!(stop.content.contains("hook"), "stop_hook should delegate to sahjhan hook eval");
    assert!(stop.content.contains("Stop"));
    assert_eq!(stop.hook_type, "Stop");

    // Bootstrap should still exist and be self-contained
    let bootstrap = hooks.iter().find(|h| h.filename == "_sahjhan_bootstrap.py").unwrap();
    assert!(bootstrap.content.contains("PROTECTED"));
}

#[test]
fn suggested_hooks_json_includes_stop() {
    let gen = HookGenerator::new().unwrap();
    let config = make_config(vec!["output"]);
    let hooks = gen.generate(&config, "cc", None).unwrap();

    let json = HookGenerator::suggested_hooks_json(&hooks, ".hooks");
    assert!(json.contains("\"Stop\""), "Should include Stop hook type");
    assert!(json.contains("stop_hook.py"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test hook_generation_produces_thin_wrappers suggested_hooks_json_includes_stop -- --nocapture`
Expected: FAIL

- [ ] **Step 3: Update `src/hooks/generate.rs`**

Replace the three template constants and the `generate` method. Keep `BOOTSTRAP_HOOK` unchanged. Replace `WRITE_GUARD_TEMPLATE` and `BASH_GUARD_TEMPLATE` with new thin wrappers:

```rust
const PRE_TOOL_HOOK_TEMPLATE: &str = r##"# Generated hook: pre_tool_hook.py
# PreToolUse hook — delegates to sahjhan hook eval
import os, sys, json, subprocess, platform

def sahjhan_binary():
    env = os.environ.get("SAHJHAN_BIN")
    if env:
        return env
    arch = platform.machine()
    system = platform.system().lower()
    if arch == "arm64":
        arch = "aarch64"
    if system == "darwin":
        triple = f"{arch}-apple-darwin"
    else:
        triple = f"{arch}-unknown-linux-gnu"
    root = os.environ.get("CLAUDE_PLUGIN_ROOT",
           os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    return os.path.join(root, "bin", f"sahjhan-{triple}")

CONFIG_DIR = "{config_dir}"

def main():
    event = json.loads(sys.stdin.read())
    tool_name = event.get("tool_name", "")
    tool_input = event.get("tool_input", {})
    file_path = tool_input.get("file_path", "")

    cmd = [sahjhan_binary(), "--config-dir", CONFIG_DIR,
           "hook", "eval", "--event", "PreToolUse",
           "--tool", tool_name]
    if file_path:
        cmd.extend(["--file", file_path])

    try:
        result = subprocess.run(cmd, capture_output=True, text=True,
                                cwd=event.get("cwd", os.getcwd()), timeout=30)
        output = json.loads(result.stdout) if result.stdout.strip() else {"decision": "allow"}
    except Exception:
        print(json.dumps({"decision": "allow"}))
        return

    if output.get("decision") == "block":
        messages = output.get("messages", [])
        reason = messages[0]["message"] if messages else "Blocked by protocol hook."
        print(json.dumps({"decision": "block", "reason": reason}))
    elif output.get("decision") == "warn":
        messages = output.get("messages", [])
        warnings = output.get("monitor_warnings", [])
        parts = [m["message"] for m in messages] + [w["message"] for w in warnings]
        print(json.dumps({"decision": "allow", "message": "\n".join(parts)}))
    else:
        print(json.dumps({"decision": "allow"}))

if __name__ == "__main__":
    main()
"##;

const POST_TOOL_HOOK_TEMPLATE: &str = r##"# Generated hook: post_tool_hook.py
# PostToolUse hook — delegates to sahjhan hook eval
import os, sys, json, subprocess, platform

def sahjhan_binary():
    env = os.environ.get("SAHJHAN_BIN")
    if env:
        return env
    arch = platform.machine()
    system = platform.system().lower()
    if arch == "arm64":
        arch = "aarch64"
    if system == "darwin":
        triple = f"{arch}-apple-darwin"
    else:
        triple = f"{arch}-unknown-linux-gnu"
    root = os.environ.get("CLAUDE_PLUGIN_ROOT",
           os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    return os.path.join(root, "bin", f"sahjhan-{triple}")

CONFIG_DIR = "{config_dir}"

def main():
    event = json.loads(sys.stdin.read())
    tool_name = event.get("tool_name", "")
    tool_input = event.get("tool_input", {})
    file_path = tool_input.get("file_path", "")

    cmd = [sahjhan_binary(), "--config-dir", CONFIG_DIR,
           "hook", "eval", "--event", "PostToolUse",
           "--tool", tool_name]
    if file_path:
        cmd.extend(["--file", file_path])

    try:
        result = subprocess.run(cmd, capture_output=True, text=True,
                                cwd=event.get("cwd", os.getcwd()), timeout=30)
        output = json.loads(result.stdout) if result.stdout.strip() else {"decision": "allow"}
    except Exception:
        print(json.dumps({"decision": "allow"}))
        return

    if output.get("decision") == "warn":
        messages = output.get("messages", [])
        warnings = output.get("monitor_warnings", [])
        parts = [m["message"] for m in messages] + [w["message"] for w in warnings]
        print(json.dumps({"decision": "allow", "message": "\n".join(parts)}))
    else:
        print(json.dumps({"decision": "allow"}))

if __name__ == "__main__":
    main()
"##;

const STOP_HOOK_TEMPLATE: &str = r##"# Generated hook: stop_hook.py
# Stop hook — delegates to sahjhan hook eval
import os, sys, json, subprocess, platform

def sahjhan_binary():
    env = os.environ.get("SAHJHAN_BIN")
    if env:
        return env
    arch = platform.machine()
    system = platform.system().lower()
    if arch == "arm64":
        arch = "aarch64"
    if system == "darwin":
        triple = f"{arch}-apple-darwin"
    else:
        triple = f"{arch}-unknown-linux-gnu"
    root = os.environ.get("CLAUDE_PLUGIN_ROOT",
           os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    return os.path.join(root, "bin", f"sahjhan-{triple}")

CONFIG_DIR = "{config_dir}"

def main():
    event = json.loads(sys.stdin.read())
    stop_message = event.get("stop_message", "")

    cmd = [sahjhan_binary(), "--config-dir", CONFIG_DIR,
           "hook", "eval", "--event", "Stop"]
    if stop_message:
        cmd.extend(["--output-text", stop_message])

    try:
        result = subprocess.run(cmd, capture_output=True, text=True,
                                cwd=event.get("cwd", os.getcwd()), timeout=30)
        output = json.loads(result.stdout) if result.stdout.strip() else {"decision": "allow"}
    except Exception:
        print(json.dumps({"decision": "allow"}))
        return

    if output.get("decision") == "block":
        messages = output.get("messages", [])
        reason = messages[0]["message"] if messages else "Blocked by protocol hook."
        print(json.dumps({"decision": "block", "reason": reason}))
    else:
        print(json.dumps({"decision": "allow"}))

if __name__ == "__main__":
    main()
"##;
```

Update the `generate` method to produce the 4 new hooks:

```rust
pub fn generate(
    &self,
    config: &ProtocolConfig,
    harness: &str,
    output_dir: Option<&Path>,
) -> Result<Vec<GeneratedHook>, String> {
    if harness != "cc" {
        return Err(format!(
            "Unknown harness '{}'. Only 'cc' (Claude Code) is supported.",
            harness
        ));
    }

    let config_dir_value = "enforcement";
    let mut hooks = Vec::new();

    // --- Pre-tool hook (PreToolUse) ---
    let pre_tool = PRE_TOOL_HOOK_TEMPLATE.replace("{config_dir}", config_dir_value);
    hooks.push(GeneratedHook {
        filename: "pre_tool_hook.py".to_string(),
        content: pre_tool,
        hook_type: "PreToolUse".to_string(),
    });

    // --- Post-tool hook (PostToolUse) ---
    let post_tool = POST_TOOL_HOOK_TEMPLATE.replace("{config_dir}", config_dir_value);
    hooks.push(GeneratedHook {
        filename: "post_tool_hook.py".to_string(),
        content: post_tool,
        hook_type: "PostToolUse".to_string(),
    });

    // --- Stop hook (Stop) ---
    let stop = STOP_HOOK_TEMPLATE.replace("{config_dir}", config_dir_value);
    hooks.push(GeneratedHook {
        filename: "stop_hook.py".to_string(),
        content: stop,
        hook_type: "Stop".to_string(),
    });

    // --- Bootstrap hook (PreToolUse) — self-contained ---
    hooks.push(GeneratedHook {
        filename: "_sahjhan_bootstrap.py".to_string(),
        content: BOOTSTRAP_HOOK.to_string(),
        hook_type: "PreToolUse".to_string(),
    });

    // Write to disk if output_dir is specified.
    if let Some(dir) = output_dir {
        std::fs::create_dir_all(dir)
            .map_err(|e| format!("Cannot create output directory {}: {}", dir.display(), e))?;
        for hook in &hooks {
            let path = dir.join(&hook.filename);
            std::fs::write(&path, &hook.content)
                .map_err(|e| format!("Cannot write {}: {}", path.display(), e))?;
        }
    }

    Ok(hooks)
}
```

Update `suggested_hooks_json` to handle Stop hooks:

```rust
pub fn suggested_hooks_json(hooks: &[GeneratedHook], hooks_dir: &str) -> String {
    let mut pre_hooks = Vec::new();
    let mut post_hooks = Vec::new();
    let mut stop_hooks = Vec::new();

    for hook in hooks {
        let entry = format!("\"python3 {}/{}\"", hooks_dir, hook.filename);
        match hook.hook_type.as_str() {
            "PreToolUse" => pre_hooks.push(entry),
            "PostToolUse" => post_hooks.push(entry),
            "Stop" => stop_hooks.push(entry),
            _ => {}
        }
    }

    format!(
        r#"{{
  "hooks": {{
    "PreToolUse": [
      {}
    ],
    "PostToolUse": [
      {}
    ],
    "Stop": [
      {}
    ]
  }}
}}"#,
        pre_hooks.join(",\n      "),
        post_hooks.join(",\n      "),
        stop_hooks.join(",\n      "),
    )
}
```

- [ ] **Step 4: Update existing tests in `tests/hook_generation_tests.rs`**

Update tests that reference `write_guard.py` and `bash_guard.py` to reference `pre_tool_hook.py` and `post_tool_hook.py`. Update the count assertion from 3 to 4. Update content assertions to match new script contents.

- [ ] **Step 5: Update inline tests in `src/hooks/generate.rs`**

Update the `mod tests` section to match new script names and count.

- [ ] **Step 6: Run tests**

Run: `cargo test hook_generation -- --nocapture`
Expected: All PASS

- [ ] **Step 7: Commit**

```bash
git add src/hooks/generate.rs tests/hook_generation_tests.rs
git commit -m "feat: replace static hook scripts with thin wrappers delegating to hook eval"
```

---

### Task 10: Example `hooks.toml`

**Files:**
- Create: `examples/minimal/hooks.toml`

- [ ] **Step 1: Write test that minimal example validates with hooks.toml**

Add to `tests/config_tests.rs`:

```rust
#[test]
fn test_minimal_example_loads_with_hooks() {
    let config = sahjhan::config::ProtocolConfig::load(
        std::path::Path::new("examples/minimal"),
    ).unwrap();
    // Should have hooks from the example (gate hook + stop hook = 2, no auto_record without defined event)
    assert!(!config.hooks.is_empty(), "minimal example should have hooks");
    assert!(!config.monitors.is_empty(), "minimal example should have monitors");

    // Should validate
    let (errors, _warnings) = config.validate_deep(std::path::Path::new("examples/minimal"));
    // Filter out errors not related to hooks (template files may not exist)
    let hook_errors: Vec<_> = errors.iter().filter(|e| e.contains("hooks.toml")).collect();
    assert!(hook_errors.is_empty(), "hooks should validate. Errors: {:?}", hook_errors);
}
```

- [ ] **Step 2: Create `examples/minimal/hooks.toml`**

```toml
# Example hooks.toml for the minimal protocol.
#
# Demonstrates: gate-based pre-tool hook, stop hook, and monitor.

[[hooks]]
event = "PreToolUse"
tools = ["Edit", "Write"]
states = ["working"]
action = "block"
message = "Cannot edit source files in working state without a check_done event. Record one first."

[hooks.gate]
type = "ledger_has_event_since"
event = "check_done"
since = "last_transition"

[[hooks]]
event = "Stop"
states_not = ["done"]
action = "block"
message = "Cannot claim completion in state {current_state}. Transition to done first."

[hooks.check]
type = "output_contains_any"
patterns = ["task complete", "all done", "finished"]

[[monitors]]
name = "idle_stall"
states = ["working"]
action = "warn"
message = "{count} events since last state transition without advancing."

[monitors.trigger]
type = "event_count_since_last_transition"
threshold = 10
```

- [ ] **Step 3: Run test**

Run: `cargo test test_minimal_example_loads_with_hooks -- --nocapture`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add examples/minimal/hooks.toml tests/config_tests.rs
git commit -m "feat: add hooks.toml example to minimal protocol"
```

---

### Task 11: Update README

**Files:**
- Modify: `README.md`

The README has a distinctive voice — dryly sarcastic, world-weary, told through war stories of agents misbehaving. New sections should weave in naturally, not read like appended documentation.

- [ ] **Step 1: Add runtime hooks section after "Integrating with Claude Code"**

Insert after the Claude Code integration section (after line ~798), before "CLI reference":

```markdown
### Runtime hooks

The generated hook scripts used to contain the enforcement logic themselves — managed path lists baked in at generation time, manifest verification hardcoded in Python. Which meant every time you changed the protocol, you had to regenerate them. And the scripts were readable. The agent could study the conditions and find the gap.

Now the scripts are three-line wrappers. They parse the harness event, call `sahjhan hook eval`, and forward the decision. All the intelligence lives in the Rust binary, evaluated against live ledger state. Change your `hooks.toml`, and the next tool use picks it up. No regeneration.

```bash
sahjhan hook eval --event PreToolUse --tool Edit --file src/main.rs
# {"decision": "block", "messages": [{"source": "hook", "action": "block",
#   "message": "TDD violation: write a failing test first."}],
#   "auto_records": [], "monitor_warnings": []}
```

Hooks fire on three events: `PreToolUse` (before the agent uses a tool), `PostToolUse` (after), and `Stop` (when the agent tries to end its turn). Each hook can be gated to specific tools, states, and file paths.

Three kinds of conditions:

**Gate hooks** reuse the existing gate system. Any gate type that works in a transition works in a hook. If the gate fails, the hook blocks:

```toml
[[hooks]]
event = "PreToolUse"
tools = ["Edit"]
states = ["fix_loop"]
action = "block"
message = "Write a failing test before editing source files."

[hooks.gate]
type = "ledger_has_event_since"
event = "failing_test"
since = "fix_commit"

[hooks.filter]
path_not_matches = "tests/*"
```

That filter means the hook only applies to source files. Edit `tests/test_thing.py` and the hook steps aside. Edit `src/main.py` without a `failing_test` event since the last `fix_commit` and you're blocked. TDD, mechanically enforced.

**Check hooks** evaluate conditions the gate system doesn't cover — pattern matching on agent output, event count thresholds:

```toml
[[hooks]]
event = "Stop"
states_not = ["converged", "finalized"]
action = "block"
message = "You're claiming completion but state is {current_state}."

[hooks.check]
type = "output_contains_any"
patterns = ["audit complete", "all fixes applied", "CONVERGED"]
```

This is the check that would have caught the original incident. The agent wrote "HOLTZ AUDIT COMPLETE" while still in `fix_loop`. The stop hook sees the completion language, checks the state, and blocks the output.

**Auto-record hooks** observe without enforcing. They automatically append events to the ledger when tools are used:

```toml
[[hooks]]
event = "PostToolUse"
tools = ["Edit", "Write"]

[hooks.auto_record]
event_type = "source_edit"
fields = { file_path = "{tool.file_path}" }
```

Now the ledger has ground truth about what files were actually modified, independent of what the agent claims. The stall detector and edit accumulation checks can work off these auto-recorded events, not self-reporting.

### State-gated write guards

The existing `paths.managed` blocks all writes to managed directories. But some files should be writable — just not yet. A summary document that should only exist after convergence. A punchlist that should only update in the finalized state.

`write_gated` guards in `protocol.toml` enforce this:

```toml
[guards]
read_blocked = [".sahjhan/session.key"]

[[guards.write_gated]]
path = "docs/SUMMARY.md"
writable_in = ["finalized"]
message = "SUMMARY.md can only be written after convergence. Current state: {current_state}."
```

The guard is checked during `hook eval` for Edit and Write tools. In the `finalized` state, the write goes through. In any other state, it's blocked. The path field supports globs.

### Monitors

Monitors catch drift. They don't block — they warn. A monitor fires when a threshold condition is met and the warning surfaces in every `hook eval` response until the agent does something about it.

```toml
[[monitors]]
name = "fix_loop_stall"
states = ["fix_loop"]
action = "warn"
message = "{count} events since last state transition. Commit your fixes."

[monitors.trigger]
type = "event_count_since_last_transition"
threshold = 20
```

Twenty events in `fix_loop` without advancing state. The monitor doesn't stop the agent — it just makes sure the agent (and anyone watching) knows something might be wrong. Monitors piggyback on `hook eval` calls, so they're checked on every tool use. No separate timer, no polling.
```

- [ ] **Step 2: Update the gate types table**

In the "Gate types" table, update the `ledger_has_event_since` row:

```markdown
| `ledger_has_event_since` | `event`, `since` | Event recorded since a reference point. `since = "last_transition"` or an event type name. |
```

- [ ] **Step 3: Update CLI reference**

Add to the CLI reference section:

```
sahjhan hook eval --event <E> [--tool <T>] [--file <F>] [--output-text <text>]
                                          Evaluate hooks against current state (JSON)
```

- [ ] **Step 4: Update architecture diagram**

Add hooks eval to the Sahjhan Engine box:

```
|   state machine, JSONL hash-chain ledger,         |
|   DataFusion query engine, gate evaluator,        |
|   manifest, template renderer, ledger registry,   |
|   hook evaluator                                  |
```

Update the Hook Bridge box:

```
|               Hook Bridge                         |
|        (generated scripts, per-harness)           |
|   PreToolUse / PostToolUse / Stop for Claude Code |
```

Update the config section:

```
  config/              TOML parsing (protocol, states, transitions, events, renders, hooks)
```

- [ ] **Step 5: Update the "What the agent tries" table**

Add row:

```markdown
| Edit source without failing test | Runtime hook checks gate, blocks the edit |
| Claim "audit complete" in non-terminal state | Stop hook pattern-matches output, blocks |
```

- [ ] **Step 6: Commit**

```bash
git add README.md
git commit -m "docs: add runtime hooks, state-gated guards, and monitors to README"
```

---

### Task 12: Update CLAUDE.md

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Add hooks/eval module to Module Lookup Tables**

In the `hooks/` table, add:

```markdown
### hooks/ — Hook Evaluation and Generation

| Concept | File | Anchor/Item | Purpose |
|---------|------|-------------|---------|
| Hook generator | `hooks/generate.rs` | `HookGenerator` | Produces Python hook scripts (thin wrappers) |
| Generated hook | `hooks/generate.rs` | `GeneratedHook` | Hook type + content |
| Hook eval request | `hooks/eval.rs` | `HookEvalRequest` | Inputs from harness (event, tool, file, output) |
| Hook eval result | `hooks/eval.rs` | `HookEvalResult` | Combined decision + messages + auto_records + monitor warnings |
| evaluate_hooks | `hooks/eval.rs` | `evaluate_hooks` | Main entry: evaluates write guards, hooks, auto-records, monitors |
| Write-gated eval | `hooks/eval.rs` | `[eval-write-gated]` | Check state-gated write guards |
| Hook condition eval | `hooks/eval.rs` | `[eval-hook-condition]` | Evaluate gate/check for single hook |
| Monitor eval | `hooks/eval.rs` | `[eval-monitors]` | Evaluate all monitor triggers |
| Glob matching | `hooks/eval.rs` | `glob_match` | Simple glob pattern matching for path filters |
```

- [ ] **Step 2: Add config/hooks.rs to config table**

```markdown
| Hook definitions | `config/hooks.rs` | `HooksFile`, `HookConfig`, `HookEvent` | hooks.toml; hooks, monitors, filters, checks, auto_record |
```

- [ ] **Step 3: Update gate table for ledger_has_event_since**

Change:

```markdown
| Event since gate | `gates/ledger.rs` | `[eval-ledger-has-event-since]` | Event since last transition |
```

To:

```markdown
| Event since gate | `gates/ledger.rs` | `[eval-ledger-has-event-since]` | Event since reference point (required `since` param: "last_transition" or event type) |
```

- [ ] **Step 4: Update CLI table**

Add:

```markdown
| Hook eval | `cli/hooks_cmd.rs` | `cmd_hook_eval` | Runtime hook evaluation against live state |
```

- [ ] **Step 5: Add Flow Map for hook evaluation**

```markdown
### Flow: Hook Evaluation

How `sahjhan hook eval --event PreToolUse --tool Edit --file src/main.rs` executes:

\```
main.rs [cli-main]
  → cli/hooks_cmd.rs cmd_hook_eval
    → cli/commands.rs [load-config]
    → ledger/chain.rs [ledger-open]
    → hooks/eval.rs evaluate_hooks
      → hooks/eval.rs [derive-current-state]     ← last state_transition "to" field
      → hooks/eval.rs [eval-write-gated]          ← check guards.write_gated vs current state
      → for each hook in config.hooks:
        → hook_matches()                          ← event, tool, state, filter checks
        → if auto_record: resolve templates, collect
        → else: [eval-hook-condition]
          → gate: gates/evaluator.rs evaluate_gate
          → check: inline threshold/pattern eval
      → hooks/eval.rs [eval-monitors]             ← threshold checks
    → auto_records → ledger/chain.rs [ledger-append]  ← side effect
    → return HookEvalData (JSON)
\```
```

- [ ] **Step 6: Update config seal note**

Update the `compute_config_seals` entry to mention 6 files:

```markdown
| Config seal hashing | `config/mod.rs` | `compute_config_seals()` | SHA-256 hash all 6 TOML config files |
```

- [ ] **Step 7: Update test files table**

Add:

```markdown
| `tests/hook_eval_tests.rs` | Hook evaluation: gate/check/filter/monitor/write-gated/CLI integration |
```

- [ ] **Step 8: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md with hooks eval module, flow maps, gate changes"
```

---

### Task 13: Final Validation

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

- [ ] **Step 3: Run fmt**

Run: `cargo fmt --check`
Expected: No formatting issues

- [ ] **Step 4: Verify the minimal example end-to-end**

```bash
cd /tmp && mkdir hook-test && cd hook-test
cp -r /path/to/sahjhan/examples/minimal enforcement
sahjhan init
sahjhan status
sahjhan hook eval --event PreToolUse --tool Edit --file src/main.rs
# Should block (working state requires check_done)
sahjhan transition begin
sahjhan hook eval --event PreToolUse --tool Edit --file src/main.rs
# Should block (now in working state, no check_done since transition)
sahjhan event check_done
sahjhan hook eval --event PreToolUse --tool Edit --file src/main.rs
# Should allow (check_done recorded)
```

- [ ] **Step 5: Commit any remaining fixes**

```bash
git add -A && git commit -m "fix: final validation pass for hooks/guards/monitors"
```
