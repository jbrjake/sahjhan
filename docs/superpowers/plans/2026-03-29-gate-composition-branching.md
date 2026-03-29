# Gate Composition, Branching Transitions, and Mermaid Export — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add boolean gate composition (any_of, all_of, not, k_of_n), conditional branching transitions (multiple candidates per command), and Mermaid/ASCII protocol visualization to Sahjhan.

**Architecture:** Extends the existing gate evaluation dispatch with four composite gate types that recursively evaluate nested gate arrays. The state machine's transition logic changes from first-match to first-passing-match across multiple candidates. A new `mermaid` module generates stateDiagram-v2 text and ASCII tree-walk output from ProtocolConfig.

**Tech Stack:** Rust, serde (recursive GateConfig deserialization), clap (new subcommand), existing gate evaluator infrastructure.

**Spec:** `docs/superpowers/specs/2026-03-29-gate-composition-branching-design.md`

---

## File Map

### New files
| File | Responsibility |
|------|---------------|
| `src/mermaid.rs` | Mermaid text + ASCII art generation from `&ProtocolConfig` |
| `src/cli/mermaid.rs` | CLI command module for `sahjhan mermaid` |
| `tests/mermaid_tests.rs` | Tests for Mermaid/ASCII output |

### Modified files
| File | What changes |
|------|-------------|
| `src/config/transitions.rs` | `GateConfig` gains `gates: Vec<GateConfig>` field |
| `src/gates/types.rs:29-58` | New match arms in `eval()` for composite gates |
| `src/gates/evaluator.rs:60-76` | `default_intent` gains composite gate entries |
| `src/state/machine.rs:110-161` | `transition()` becomes multi-candidate with fallthrough |
| `src/state/machine.rs:32-47` | `StateError` gains `AllCandidatesBlocked` variant |
| `src/config/mod.rs:246-284` | `validate_deep` gains composite gate validation + branching warnings |
| `src/cli/transition.rs:31-150` | `cmd_transition` handles new error variant |
| `src/cli/transition.rs:157-263` | `cmd_gate_check` shows all candidates |
| `src/main.rs:56-199` | New `Mermaid` variant in `Commands` enum |
| `src/lib.rs` | Export `mermaid` module |
| `src/cli/mod.rs` | Export `mermaid` CLI module |
| `tests/gate_tests.rs` | Tests for composite gate evaluation |
| `tests/state_machine_tests.rs` | Tests for multi-candidate transitions |
| `tests/config_tests.rs` | Validation tests for composite gates |

---

## Task 1: Add `gates` field to GateConfig

**Files:**
- Modify: `src/config/transitions.rs:41-51`
- Test: `tests/config_tests.rs`

- [ ] **Step 1: Write failing test — GateConfig with nested gates deserializes**

Append to `tests/config_tests.rs`:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_gate_config_nested_gates_deserialize -- --nocapture 2>&1`
Expected: compilation error — `GateConfig` has no field `gates`.

- [ ] **Step 3: Add `gates` field to GateConfig**

In `src/config/transitions.rs`, replace the `GateConfig` struct:

```rust
#[derive(Debug, Deserialize, Clone)]
pub struct GateConfig {
    #[serde(rename = "type")]
    pub gate_type: String,
    /// Optional human-readable explanation of why this gate exists.
    #[serde(default)]
    pub intent: Option<String>,
    /// Nested gates for composite types (any_of, all_of, not, k_of_n).
    /// Empty for leaf gates.
    #[serde(default)]
    pub gates: Vec<GateConfig>,
    #[serde(flatten)]
    pub params: HashMap<String, toml::Value>,
}
```

- [ ] **Step 4: Fix make_gate helper in tests/gate_tests.rs**

The `make_gate` helper constructs GateConfig without the new `gates` field. Update it:

```rust
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
```

- [ ] **Step 5: Fix any other GateConfig construction sites**

Search the codebase for `GateConfig {` and add `gates: vec![],` to every construction site that is missing it. This includes test files and any programmatic construction in `src/`. Check with:

```bash
cargo build --tests 2>&1
```

Fix all compilation errors before proceeding.

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test test_gate_config_nested_gates_deserialize -- --nocapture 2>&1`
Expected: PASS

- [ ] **Step 7: Run full test suite**

Run: `cargo test 2>&1`
Expected: All existing tests still pass.

- [ ] **Step 8: Commit**

```bash
git add src/config/transitions.rs tests/config_tests.rs tests/gate_tests.rs
git commit -m "feat: add nested gates field to GateConfig for composite gate support"
```

---

## Task 2: Implement composite gate evaluation (any_of, all_of, not, k_of_n)

**Files:**
- Modify: `src/gates/types.rs:29-58`
- Modify: `src/gates/evaluator.rs:60-76`
- Test: `tests/gate_tests.rs`

- [ ] **Step 1: Write failing test — any_of passes when one child passes**

Append to `tests/gate_tests.rs`:

```rust
#[test]
fn test_any_of_passes_when_one_child_passes() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let existing_file = dir.path().join("exists.txt");
    std::fs::write(&existing_file, "content").unwrap();

    let gate = GateConfig {
        gate_type: "any_of".to_string(),
        intent: None,
        gates: vec![
            make_gate(
                "file_exists",
                vec![("path", toml::Value::String("/nonexistent".to_string()))],
            ),
            make_gate(
                "file_exists",
                vec![("path", toml::Value::String(existing_file.to_str().unwrap().to_string()))],
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
    assert!(result.passed, "any_of should pass when one child passes: {:?}", result.reason);
}
```

- [ ] **Step 2: Write failing test — any_of fails when no child passes**

```rust
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
                vec![("path", toml::Value::String("/nonexistent_a".to_string()))],
            ),
            make_gate(
                "file_exists",
                vec![("path", toml::Value::String("/nonexistent_b".to_string()))],
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
    assert!(!result.passed, "any_of should fail when no child passes");
}
```

- [ ] **Step 3: Write failing test — all_of passes when all children pass**

```rust
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
    assert!(result.passed, "all_of should pass when all children pass: {:?}", result.reason);
}
```

- [ ] **Step 4: Write failing test — all_of fails when one child fails**

```rust
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
                vec![("path", toml::Value::String("/nonexistent".to_string()))],
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
    assert!(!result.passed, "all_of should fail when one child fails");
}
```

- [ ] **Step 5: Write failing test — not inverts child result**

```rust
#[test]
fn test_not_inverts_passing_child() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let file = dir.path().join("exists.txt");
    std::fs::write(&file, "content").unwrap();

    let gate = GateConfig {
        gate_type: "not".to_string(),
        intent: None,
        gates: vec![make_gate(
            "file_exists",
            vec![("path", toml::Value::String(file.to_str().unwrap().to_string()))],
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
    assert!(!result.passed, "not should fail when child passes");
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
            vec![("path", toml::Value::String("/nonexistent".to_string()))],
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
    assert!(result.passed, "not should pass when child fails: {:?}", result.reason);
}
```

- [ ] **Step 6: Write failing test — k_of_n passes and fails at threshold**

```rust
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
                vec![("path", toml::Value::String("/nonexistent".to_string()))],
            ),
        ],
        params: vec![("k".to_string(), toml::Value::Integer(2))].into_iter().collect(),
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
    assert!(result.passed, "k_of_n(2/3) should pass with 2 passing: {:?}", result.reason);
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
                vec![("path", toml::Value::String("/nonexistent_a".to_string()))],
            ),
            make_gate(
                "file_exists",
                vec![("path", toml::Value::String("/nonexistent_b".to_string()))],
            ),
        ],
        params: vec![("k".to_string(), toml::Value::Integer(2))].into_iter().collect(),
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
    assert!(!result.passed, "k_of_n(2/3) should fail with only 1 passing");
}
```

- [ ] **Step 7: Run tests to verify they fail**

Run: `cargo test test_any_of test_all_of test_not_inverts test_k_of_n 2>&1`
Expected: FAIL — unknown gate type errors for any_of, all_of, not, k_of_n.

- [ ] **Step 8: Add default_intent entries for composite gates**

In `src/gates/evaluator.rs`, add to the `default_intent` match (before the `_ =>` arm):

```rust
        "any_of" => "at least one alternative must pass",
        "all_of" => "all conditions must pass",
        "not" => "condition must not be met",
        "k_of_n" => "minimum number of conditions must pass",
```

- [ ] **Step 9: Implement composite gate evaluation in eval()**

In `src/gates/types.rs`, add these match arms in the `eval` function before the `other =>` catch-all:

```rust
        "any_of" => {
            let child_results: Vec<GateResult> = gate.gates.iter().map(|g| eval(g, ctx)).collect();
            let passed_count = child_results.iter().filter(|r| r.passed).count();
            let total = child_results.len();
            let passed = passed_count > 0;
            let failed_descriptions: Vec<String> = child_results
                .iter()
                .filter(|r| !r.passed)
                .map(|r| format!("{}: {}", r.gate_type, r.reason.as_deref().unwrap_or("failed")))
                .collect();
            GateResult {
                passed,
                gate_type: "any_of".to_string(),
                description: format!("{} of {} alternatives passed", passed_count, total),
                reason: if passed {
                    None
                } else {
                    Some(format!("no alternatives passed: {}", failed_descriptions.join("; ")))
                },
                intent: None,
            }
        }
        "all_of" => {
            let child_results: Vec<GateResult> = gate.gates.iter().map(|g| eval(g, ctx)).collect();
            let passed_count = child_results.iter().filter(|r| r.passed).count();
            let total = child_results.len();
            let passed = passed_count == total;
            let failed_descriptions: Vec<String> = child_results
                .iter()
                .filter(|r| !r.passed)
                .map(|r| format!("{}: {}", r.gate_type, r.reason.as_deref().unwrap_or("failed")))
                .collect();
            GateResult {
                passed,
                gate_type: "all_of".to_string(),
                description: format!("{} of {} conditions passed", passed_count, total),
                reason: if passed {
                    None
                } else {
                    Some(format!("failed conditions: {}", failed_descriptions.join("; ")))
                },
                intent: None,
            }
        }
        "not" => {
            if gate.gates.len() != 1 {
                return GateResult {
                    passed: false,
                    gate_type: "not".to_string(),
                    description: "not gate requires exactly 1 child".to_string(),
                    reason: Some(format!("not gate has {} children, expected 1", gate.gates.len())),
                    intent: None,
                };
            }
            let child = eval(&gate.gates[0], ctx);
            GateResult {
                passed: !child.passed,
                gate_type: "not".to_string(),
                description: format!("not({})", child.gate_type),
                reason: if !child.passed {
                    None // child failed, so not passes — no reason needed
                } else {
                    Some(format!(
                        "child gate '{}' passed but was expected to fail",
                        child.gate_type
                    ))
                },
                intent: None,
            }
        }
        "k_of_n" => {
            let k = gate
                .params
                .get("k")
                .and_then(|v| v.as_integer())
                .unwrap_or(0) as usize;
            let child_results: Vec<GateResult> = gate.gates.iter().map(|g| eval(g, ctx)).collect();
            let passed_count = child_results.iter().filter(|r| r.passed).count();
            let total = child_results.len();
            let passed = passed_count >= k;
            let failed_descriptions: Vec<String> = child_results
                .iter()
                .filter(|r| !r.passed)
                .map(|r| format!("{}: {}", r.gate_type, r.reason.as_deref().unwrap_or("failed")))
                .collect();
            GateResult {
                passed,
                gate_type: "k_of_n".to_string(),
                description: format!("{} of {} passed ({} required)", passed_count, total, k),
                reason: if passed {
                    None
                } else {
                    Some(format!(
                        "{} of {} passed but {} required: {}",
                        passed_count,
                        total,
                        k,
                        failed_descriptions.join("; ")
                    ))
                },
                intent: None,
            }
        }
```

- [ ] **Step 10: Run tests to verify they pass**

Run: `cargo test test_any_of test_all_of test_not_inverts test_k_of_n 2>&1`
Expected: All 8 new tests PASS.

- [ ] **Step 11: Run full test suite**

Run: `cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 12: Commit**

```bash
git add src/gates/types.rs src/gates/evaluator.rs tests/gate_tests.rs
git commit -m "feat: implement composite gate evaluation (any_of, all_of, not, k_of_n)"
```

---

## Task 3: Validate composite gates in validate_deep

**Files:**
- Modify: `src/config/mod.rs:246-284`
- Test: `tests/config_tests.rs`

- [ ] **Step 1: Write failing test — any_of with empty gates is error**

Append to `tests/config_tests.rs`:

```rust
#[test]
fn test_validate_any_of_empty_gates_is_error() {
    use sahjhan::config::*;
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.transitions.push(TransitionConfig {
        from: "idle".to_string(),
        to: "working".to_string(),
        command: "bad_any".to_string(),
        args: vec![],
        gates: vec![GateConfig {
            gate_type: "any_of".to_string(),
            intent: None,
            gates: vec![],
            params: std::collections::HashMap::new(),
        }],
    });
    let (errors, _) = config.validate_deep(Path::new("examples/minimal"));
    assert!(
        errors.iter().any(|e| e.contains("any_of") && e.contains("empty")),
        "Expected error about empty gates: {:?}",
        errors
    );
}
```

- [ ] **Step 2: Write failing test — not with 2 children is error**

```rust
#[test]
fn test_validate_not_wrong_child_count_is_error() {
    use sahjhan::config::*;
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.transitions.push(TransitionConfig {
        from: "idle".to_string(),
        to: "working".to_string(),
        command: "bad_not".to_string(),
        args: vec![],
        gates: vec![GateConfig {
            gate_type: "not".to_string(),
            intent: None,
            gates: vec![
                GateConfig {
                    gate_type: "file_exists".to_string(),
                    intent: None,
                    gates: vec![],
                    params: vec![("path".to_string(), toml::Value::String("a.txt".to_string()))]
                        .into_iter().collect(),
                },
                GateConfig {
                    gate_type: "file_exists".to_string(),
                    intent: None,
                    gates: vec![],
                    params: vec![("path".to_string(), toml::Value::String("b.txt".to_string()))]
                        .into_iter().collect(),
                },
            ],
            params: std::collections::HashMap::new(),
        }],
    });
    let (errors, _) = config.validate_deep(Path::new("examples/minimal"));
    assert!(
        errors.iter().any(|e| e.contains("not") && e.contains("exactly 1")),
        "Expected error about not needing exactly 1 child: {:?}",
        errors
    );
}
```

- [ ] **Step 3: Write failing test — k_of_n missing k param is error**

```rust
#[test]
fn test_validate_k_of_n_missing_k_is_error() {
    use sahjhan::config::*;
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.transitions.push(TransitionConfig {
        from: "idle".to_string(),
        to: "working".to_string(),
        command: "bad_k".to_string(),
        args: vec![],
        gates: vec![GateConfig {
            gate_type: "k_of_n".to_string(),
            intent: None,
            gates: vec![GateConfig {
                gate_type: "file_exists".to_string(),
                intent: None,
                gates: vec![],
                params: vec![("path".to_string(), toml::Value::String("a.txt".to_string()))]
                    .into_iter().collect(),
            }],
            params: std::collections::HashMap::new(),
        }],
    });
    let (errors, _) = config.validate_deep(Path::new("examples/minimal"));
    assert!(
        errors.iter().any(|e| e.contains("k_of_n") && e.contains("'k'")),
        "Expected error about missing k param: {:?}",
        errors
    );
}
```

- [ ] **Step 4: Write failing test — nested child gates are recursively validated**

```rust
#[test]
fn test_validate_composite_validates_children_recursively() {
    use sahjhan::config::*;
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.transitions.push(TransitionConfig {
        from: "idle".to_string(),
        to: "working".to_string(),
        command: "bad_nested".to_string(),
        args: vec![],
        gates: vec![GateConfig {
            gate_type: "any_of".to_string(),
            intent: None,
            gates: vec![GateConfig {
                gate_type: "bogus_type".to_string(),
                intent: None,
                gates: vec![],
                params: std::collections::HashMap::new(),
            }],
            params: std::collections::HashMap::new(),
        }],
    });
    let (errors, _) = config.validate_deep(Path::new("examples/minimal"));
    assert!(
        errors.iter().any(|e| e.contains("bogus_type")),
        "Expected error about unknown child gate type: {:?}",
        errors
    );
}
```

- [ ] **Step 5: Run tests to verify they fail**

Run: `cargo test test_validate_any_of_empty test_validate_not_wrong test_validate_k_of_n_missing test_validate_composite_validates 2>&1`
Expected: FAIL — no validation errors produced for composite gates yet.

- [ ] **Step 6: Implement composite gate validation**

In `src/config/mod.rs`, replace the gate validation block (section 6, starting at the `// 6. Gate type validation.` comment) with a version that uses a recursive helper. Add this helper function as a private method on `ProtocolConfig` (or as a free function inside the impl block):

```rust
    /// Recursively validate a gate and its children.
    fn validate_gate(
        gate: &GateConfig,
        transition_command: &str,
        known_gates: &HashMap<&str, Vec<&str>>,
        errors: &mut Vec<String>,
    ) {
        match gate.gate_type.as_str() {
            "any_of" | "all_of" => {
                if gate.gates.is_empty() {
                    errors.push(format!(
                        "transitions.toml: gate '{}' in transition '{}' has empty gates list",
                        gate.gate_type, transition_command
                    ));
                }
                for child in &gate.gates {
                    Self::validate_gate(child, transition_command, known_gates, errors);
                }
            }
            "not" => {
                if gate.gates.len() != 1 {
                    errors.push(format!(
                        "transitions.toml: gate 'not' in transition '{}' requires exactly 1 child gate, has {}",
                        transition_command, gate.gates.len()
                    ));
                }
                for child in &gate.gates {
                    Self::validate_gate(child, transition_command, known_gates, errors);
                }
            }
            "k_of_n" => {
                if gate.gates.is_empty() {
                    errors.push(format!(
                        "transitions.toml: gate 'k_of_n' in transition '{}' has empty gates list",
                        transition_command
                    ));
                }
                let k = gate.params.get("k").and_then(|v| v.as_integer());
                match k {
                    None => {
                        errors.push(format!(
                            "transitions.toml: gate 'k_of_n' in transition '{}' missing required parameter 'k'",
                            transition_command
                        ));
                    }
                    Some(k_val) => {
                        if k_val < 1 || k_val as usize > gate.gates.len() {
                            errors.push(format!(
                                "transitions.toml: gate 'k_of_n' in transition '{}' has k={} but {} child gates (k must be 1..=n)",
                                transition_command, k_val, gate.gates.len()
                            ));
                        }
                    }
                }
                for child in &gate.gates {
                    Self::validate_gate(child, transition_command, known_gates, errors);
                }
            }
            _ => {
                // Leaf gate — validate type and required params
                match known_gates.get(gate.gate_type.as_str()) {
                    None => {
                        errors.push(format!(
                            "transitions.toml: transition '{}' has unknown gate type '{}'",
                            transition_command, gate.gate_type
                        ));
                    }
                    Some(required_params) => {
                        for &param in required_params {
                            if !gate.params.contains_key(param) {
                                errors.push(format!(
                                    "transitions.toml: gate '{}' in transition '{}' missing required parameter '{}'",
                                    gate.gate_type, transition_command, param
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
```

Then replace the existing section 6 loop body:

```rust
        // 6. Gate type validation (recursive for composite gates).
        for t in &self.transitions {
            for gate in &t.gates {
                Self::validate_gate(gate, &t.command, &known_gates, &mut errors);
            }
        }
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test test_validate_any_of_empty test_validate_not_wrong test_validate_k_of_n_missing test_validate_composite_validates 2>&1`
Expected: All 4 PASS.

- [ ] **Step 8: Run full test suite**

Run: `cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 9: Commit**

```bash
git add src/config/mod.rs tests/config_tests.rs
git commit -m "feat: validate composite gate structure in validate_deep"
```

---

## Task 4: Multi-candidate branching transitions

**Files:**
- Modify: `src/state/machine.rs:32-47,110-161`
- Test: `tests/state_machine_tests.rs`

- [ ] **Step 1: Write failing test — second candidate taken when first is blocked**

Append to `tests/state_machine_tests.rs`:

```rust
#[test]
fn test_branching_fallback_transition() {
    use sahjhan::config::*;

    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();

    // Add a second transition with same from+command but different target.
    // First candidate: idle→working with a gate that will fail.
    // Second candidate: idle→done with no gates (fallback).
    config.transitions = vec![
        TransitionConfig {
            from: "idle".to_string(),
            to: "working".to_string(),
            command: "go".to_string(),
            args: vec![],
            gates: vec![GateConfig {
                gate_type: "file_exists".to_string(),
                intent: None,
                gates: vec![],
                params: vec![("path".to_string(), toml::Value::String("/nonexistent".to_string()))]
                    .into_iter().collect(),
            }],
        },
        TransitionConfig {
            from: "idle".to_string(),
            to: "done".to_string(),
            command: "go".to_string(),
            args: vec![],
            gates: vec![],
        },
    ];

    let mut sm = StateMachine::new(&config, ledger);
    assert_eq!(sm.current_state(), "idle");

    let result = sm.transition("go", &[]);
    assert!(result.is_ok(), "fallback transition should succeed: {:?}", result.err());
    assert_eq!(sm.current_state(), "done");
}
```

- [ ] **Step 2: Write failing test — first candidate taken when its gates pass**

```rust
#[test]
fn test_branching_first_candidate_wins() {
    use sahjhan::config::*;

    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let existing_file = dir.path().join("exists.txt");
    std::fs::write(&existing_file, "content").unwrap();

    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.transitions = vec![
        TransitionConfig {
            from: "idle".to_string(),
            to: "working".to_string(),
            command: "go".to_string(),
            args: vec![],
            gates: vec![GateConfig {
                gate_type: "file_exists".to_string(),
                intent: None,
                gates: vec![],
                params: vec![(
                    "path".to_string(),
                    toml::Value::String(existing_file.to_str().unwrap().to_string()),
                )]
                .into_iter()
                .collect(),
            }],
        },
        TransitionConfig {
            from: "idle".to_string(),
            to: "done".to_string(),
            command: "go".to_string(),
            args: vec![],
            gates: vec![],
        },
    ];

    let mut sm = StateMachine::new(&config, ledger);
    let result = sm.transition("go", &[]);
    assert!(result.is_ok());
    assert_eq!(sm.current_state(), "working", "first candidate should win when its gates pass");
}
```

- [ ] **Step 3: Write failing test — all candidates blocked returns error**

```rust
#[test]
fn test_branching_all_candidates_blocked() {
    use sahjhan::config::*;

    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.transitions = vec![
        TransitionConfig {
            from: "idle".to_string(),
            to: "working".to_string(),
            command: "go".to_string(),
            args: vec![],
            gates: vec![GateConfig {
                gate_type: "file_exists".to_string(),
                intent: None,
                gates: vec![],
                params: vec![("path".to_string(), toml::Value::String("/no_a".to_string()))]
                    .into_iter().collect(),
            }],
        },
        TransitionConfig {
            from: "idle".to_string(),
            to: "done".to_string(),
            command: "go".to_string(),
            args: vec![],
            gates: vec![GateConfig {
                gate_type: "file_exists".to_string(),
                intent: None,
                gates: vec![],
                params: vec![("path".to_string(), toml::Value::String("/no_b".to_string()))]
                    .into_iter().collect(),
            }],
        },
    ];

    let mut sm = StateMachine::new(&config, ledger);
    let result = sm.transition("go", &[]);
    assert!(result.is_err(), "should fail when all candidates are blocked");
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test test_branching 2>&1`
Expected: `test_branching_fallback_transition` should fail (currently takes first match and blocks).

- [ ] **Step 5: Add AllCandidatesBlocked error variant**

In `src/state/machine.rs`, add to the `StateError` enum:

```rust
    #[error("all transition candidates for '{command}' from '{state}' were blocked")]
    AllCandidatesBlocked {
        command: String,
        state: String,
        /// (target_state, gate_type, reason) per failed candidate
        candidates: Vec<(String, String, String)>,
    },
```

- [ ] **Step 6: Rewrite transition() for multi-candidate matching**

Replace the `transition()` method in `src/state/machine.rs`:

```rust
    // [transition]
    pub fn transition(&mut self, command: &str, args: &[String]) -> Result<(), StateError> {
        // Collect ALL matching transitions from the current state (preserving order).
        let candidates: Vec<_> = self
            .config
            .transitions
            .iter()
            .filter(|t| t.command == command && t.from == self.current_state)
            .cloned()
            .collect();

        if candidates.is_empty() {
            return Err(StateError::NoTransition {
                command: command.to_string(),
                state: self.current_state.clone(),
            });
        }

        let mut failures: Vec<(String, String, String)> = Vec::new();

        for transition in &candidates {
            // Build state_params from the target state's param definitions.
            let mut state_params = self.build_state_params(&transition.to);

            // Map CLI args into state_params.
            let mut positional_idx = 0;
            for arg in args {
                if let Some((key, value)) = arg.split_once('=') {
                    state_params.insert(key.to_string(), value.to_string());
                } else if positional_idx < transition.args.len() {
                    state_params.insert(transition.args[positional_idx].clone(), arg.clone());
                    positional_idx += 1;
                }
            }

            // Evaluate all gates for this candidate.
            let ctx = GateContext {
                ledger: &self.ledger,
                config: &self.config,
                current_state: &self.current_state,
                state_params,
                working_dir: self.working_dir.clone(),
                event_fields: None,
            };

            let results = crate::gates::evaluator::evaluate_gates(&transition.gates, &ctx);
            let all_passed = results.iter().all(|r| r.passed);

            if all_passed {
                // This candidate passes — take this transition.
                self.ledger.reload().map_err(StateError::Ledger)?;

                let mut fields = BTreeMap::new();
                fields.insert("from".to_string(), self.current_state.clone());
                fields.insert("to".to_string(), transition.to.clone());
                fields.insert("command".to_string(), command.to_string());

                self.ledger
                    .append("state_transition", fields)
                    .map_err(StateError::Ledger)?;

                self.current_state = transition.to.clone();
                return Ok(());
            } else {
                // Stash first failure for reporting.
                if let Some(failed) = results.iter().find(|r| !r.passed) {
                    failures.push((
                        transition.to.clone(),
                        failed.gate_type.clone(),
                        failed.reason.clone().unwrap_or_else(|| "gate failed".to_string()),
                    ));
                }
            }
        }

        // No candidate passed.
        if candidates.len() == 1 {
            // Single candidate — use the original GateBlocked error for backward compatibility.
            let (_, gate_type, reason) = failures.into_iter().next().unwrap();
            Err(StateError::GateBlocked { gate_type, reason })
        } else {
            Err(StateError::AllCandidatesBlocked {
                command: command.to_string(),
                state: self.current_state.clone(),
                candidates: failures,
            })
        }
    }
```

- [ ] **Step 7: Run tests to verify they pass**

Run: `cargo test test_branching 2>&1`
Expected: All 3 branching tests PASS.

- [ ] **Step 8: Run full test suite**

Run: `cargo test 2>&1`
Expected: All tests pass. The existing single-candidate tests work unchanged because single-candidate behavior is preserved.

- [ ] **Step 9: Commit**

```bash
git add src/state/machine.rs tests/state_machine_tests.rs
git commit -m "feat: multi-candidate branching transitions with fallthrough"
```

---

## Task 5: Update CLI for branching (transition + gate-check)

**Files:**
- Modify: `src/cli/transition.rs:31-150,157-263`
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Write failing integration test — CLI fallback transition**

Append to `tests/integration_tests.rs`. First, create a helper that sets up a config with branching transitions:

```rust
fn setup_branching_dir() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        r#"
[protocol]
name = "branch-test"
version = "1.0.0"
description = "Branching test protocol"

[paths]
managed = ["output"]
data_dir = "output/.sahjhan"
render_dir = "output"
"#,
    )
    .unwrap();

    std::fs::write(
        config_dir.join("states.toml"),
        r#"
[states.idle]
label = "Idle"
initial = true

[states.happy]
label = "Happy path"

[states.fallback]
label = "Fallback path"
terminal = true
"#,
    )
    .unwrap();

    std::fs::write(
        config_dir.join("transitions.toml"),
        r#"
[[transitions]]
from = "idle"
to = "happy"
command = "go"
gates = [
    { type = "file_exists", path = "output/required.txt" },
]

[[transitions]]
from = "idle"
to = "fallback"
command = "go"
gates = []
"#,
    )
    .unwrap();

    std::fs::create_dir_all(dir.path().join("output")).unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "init"])
        .current_dir(dir.path())
        .assert()
        .success();

    dir
}

#[test]
fn test_cli_branching_takes_fallback() {
    let dir = setup_branching_dir();

    // File doesn't exist, so first candidate fails → fallback taken
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "go"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("fallback"));
}

#[test]
fn test_cli_branching_takes_first_when_gates_pass() {
    let dir = setup_branching_dir();

    // Create the required file so first candidate passes
    std::fs::write(dir.path().join("output/required.txt"), "present").unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "go"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("happy"));
}
```

- [ ] **Step 2: Run tests to verify they pass (they should already work)**

Run: `cargo test test_cli_branching 2>&1`
Expected: Both PASS — the state machine already handles this; the CLI just reports the result.

- [ ] **Step 3: Handle AllCandidatesBlocked in cmd_transition**

In `src/cli/transition.rs`, in the `cmd_transition` function's match on `machine.transition()`, add the new error variant handler after the `GateBlocked` arm:

```rust
        Err(crate::state::machine::StateError::AllCandidatesBlocked {
            command: _,
            state: _,
            candidates,
        }) => {
            for (target, gate_type, reason) in &candidates {
                eprintln!("\u{2717} → {} blocked by {}: {}", target, gate_type, reason);
            }
            EXIT_GATE_FAILED
        }
```

- [ ] **Step 4: Update cmd_gate_check for multi-candidate display**

In `src/cli/transition.rs`, in the `cmd_gate_check` function, replace the transition lookup (the `config.transitions.iter().find(...)` block) to find all candidates and display each:

Replace from `let transition = match config` through the end of the function with:

```rust
    let candidates: Vec<_> = config
        .transitions
        .iter()
        .filter(|t| t.command == transition_name && t.from == current_state)
        .cloned()
        .collect();

    if candidates.is_empty() {
        eprintln!(
            "error: no transition '{}' from state '{}'",
            transition_name, current_state
        );
        return EXIT_USAGE_ERROR;
    }

    println!("gate-check: {}", transition_name);

    let mut would_take: Option<String> = None;

    for (idx, transition) in candidates.iter().enumerate() {
        let candidate_label = if candidates.len() > 1 {
            format!("  candidate {}: {} \u{2192} {}", idx + 1, current_state, transition.to)
        } else {
            format!("  {} \u{2192} {}", current_state, transition.to)
        };
        println!("{}", candidate_label);

        if transition.gates.is_empty() {
            println!("    (no gates)");
            if would_take.is_none() {
                would_take = Some(transition.to.clone());
            }
            continue;
        }

        let mut state_params = build_state_params(&config, &transition.to, machine.ledger());
        let mut positional_idx = 0;
        for arg in args {
            if let Some((key, value)) = arg.split_once('=') {
                state_params.insert(key.to_string(), value.to_string());
            } else if positional_idx < transition.args.len() {
                state_params.insert(transition.args[positional_idx].clone(), arg.clone());
                positional_idx += 1;
            }
        }

        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let ctx = GateContext {
            ledger: machine.ledger(),
            config: &config,
            current_state: &current_state,
            state_params,
            working_dir: cwd,
            event_fields: None,
        };

        let results = evaluate_gates(&transition.gates, &ctx);
        let all_passed = results.iter().all(|r| r.passed);

        for result in &results {
            if result.passed {
                println!("    \u{2713} {}", result.description);
            } else {
                println!(
                    "    \u{2717} {}: {} \u{2014} {}",
                    result.gate_type,
                    result.reason.as_deref().unwrap_or("failed"),
                    result.intent.as_deref().unwrap_or("gate condition must be met")
                );
            }
        }

        if all_passed && would_take.is_none() {
            would_take = Some(transition.to.clone());
        }
    }

    match would_take {
        Some(target) => {
            if candidates.len() > 1 {
                println!("result: would take \u{2192} {}", target);
            } else {
                println!("result: ready");
            }
        }
        None => {
            println!("result: blocked");
        }
    }

    EXIT_SUCCESS
```

- [ ] **Step 5: Run full test suite**

Run: `cargo test 2>&1`
Expected: All tests pass including the new CLI branching tests.

- [ ] **Step 6: Commit**

```bash
git add src/cli/transition.rs tests/integration_tests.rs
git commit -m "feat: update CLI transition and gate-check for branching candidates"
```

---

## Task 6: Branching validation warning

**Files:**
- Modify: `src/config/mod.rs`
- Test: `tests/config_tests.rs`

- [ ] **Step 1: Write failing test — warning when no fallback candidate**

Append to `tests/config_tests.rs`:

```rust
#[test]
fn test_validate_branching_no_fallback_warning() {
    use sahjhan::config::*;
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    // Two candidates from idle with "go", both gated — no fallback
    config.transitions.push(TransitionConfig {
        from: "idle".to_string(),
        to: "working".to_string(),
        command: "go".to_string(),
        args: vec![],
        gates: vec![GateConfig {
            gate_type: "file_exists".to_string(),
            intent: None,
            gates: vec![],
            params: vec![("path".to_string(), toml::Value::String("a.txt".to_string()))]
                .into_iter().collect(),
        }],
    });
    config.transitions.push(TransitionConfig {
        from: "idle".to_string(),
        to: "done".to_string(),
        command: "go".to_string(),
        args: vec![],
        gates: vec![GateConfig {
            gate_type: "file_exists".to_string(),
            intent: None,
            gates: vec![],
            params: vec![("path".to_string(), toml::Value::String("b.txt".to_string()))]
                .into_iter().collect(),
        }],
    });
    let (_, warnings) = config.validate_deep(Path::new("examples/minimal"));
    assert!(
        warnings.iter().any(|w| w.contains("go") && w.contains("no fallback")),
        "Expected warning about no fallback: {:?}",
        warnings
    );
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_validate_branching_no_fallback 2>&1`
Expected: FAIL — no such warning produced.

- [ ] **Step 3: Add branching warning to validate_deep**

In `src/config/mod.rs`, add after the existing section 11 (unreachable state detection), before the ledger template validation:

```rust
        // 11b. Branching transitions without fallback (warning).
        // Group transitions by (from, command). If a group has >1 member and none
        // has empty gates, warn that there's no fallback.
        {
            let mut groups: HashMap<(&str, &str), Vec<&TransitionConfig>> = HashMap::new();
            for t in &self.transitions {
                groups
                    .entry((t.from.as_str(), t.command.as_str()))
                    .or_default()
                    .push(t);
            }
            for ((from, command), members) in &groups {
                if members.len() > 1 && !members.iter().any(|t| t.gates.is_empty()) {
                    warnings.push(format!(
                        "transitions.toml: command '{}' from '{}' has {} candidates but no fallback (all have gates — agent may get stuck)",
                        command, from, members.len()
                    ));
                }
            }
        }
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test test_validate_branching_no_fallback 2>&1`
Expected: PASS

- [ ] **Step 5: Run full test suite**

Run: `cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/config/mod.rs tests/config_tests.rs
git commit -m "feat: warn on branching transitions without fallback candidate"
```

---

## Task 7: Mermaid raw output

**Files:**
- Create: `src/mermaid.rs`
- Test: `tests/mermaid_tests.rs`

- [ ] **Step 1: Create mermaid module with public API**

Create `src/mermaid.rs`:

```rust
// src/mermaid.rs
//
// Protocol visualization: Mermaid stateDiagram-v2 and ASCII tree-walk output.
//
// ## Index
// - [generate-mermaid]   generate_mermaid()   — raw Mermaid stateDiagram-v2 text
// - [generate-ascii]     generate_ascii()     — ASCII tree-walk diagram

use crate::config::ProtocolConfig;

// [generate-mermaid]
/// Generate a Mermaid stateDiagram-v2 string from a protocol config.
pub fn generate_mermaid(config: &ProtocolConfig) -> String {
    let mut lines = vec!["stateDiagram-v2".to_string()];

    // State ID sanitization: Mermaid can't use hyphens in state IDs.
    let sanitize = |name: &str| name.replace('-', "_");

    // Emit state labels for names that need sanitization.
    for name in config.states.keys() {
        let id = sanitize(name);
        if id != *name {
            lines.push(format!("    state \"{}\" as {}", name, id));
        }
    }

    // Initial state arrow.
    if let Some(initial) = config.initial_state() {
        lines.push(format!("    [*] --> {}", sanitize(initial)));
    }

    // Transitions.
    for t in &config.transitions {
        let from_id = sanitize(&t.from);
        let to_id = sanitize(&t.to);
        let label = if t.gates.is_empty() {
            t.command.clone()
        } else {
            let gate_summary: Vec<String> = t.gates.iter().map(|g| gate_short_label(g)).collect();
            format!("{} [{}]", t.command, gate_summary.join(", "))
        };
        lines.push(format!("    {} --> {} : {}", from_id, to_id, label));
    }

    // Terminal state arrows.
    for (name, state) in &config.states {
        if state.terminal.unwrap_or(false) {
            lines.push(format!("    {} --> [*]", sanitize(name)));
        }
    }

    lines.join("\n")
}

/// Short label for a gate (used in Mermaid edge annotations).
fn gate_short_label(gate: &crate::config::GateConfig) -> String {
    match gate.gate_type.as_str() {
        "any_of" => format!("any_of({})", gate.gates.len()),
        "all_of" => format!("all_of({})", gate.gates.len()),
        "not" => {
            if let Some(child) = gate.gates.first() {
                format!("not({})", child.gate_type)
            } else {
                "not(?)".to_string()
            }
        }
        "k_of_n" => {
            let k = gate.params.get("k").and_then(|v| v.as_integer()).unwrap_or(0);
            format!("{}-of-{}", k, gate.gates.len())
        }
        other => other.to_string(),
    }
}
```

- [ ] **Step 2: Register mermaid module in lib.rs**

In `src/lib.rs`, add:

```rust
pub mod mermaid;
```

- [ ] **Step 3: Write tests for Mermaid output**

Create `tests/mermaid_tests.rs`:

```rust
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
    use std::collections::HashMap;

    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.states.insert(
        "fix-and-retry".to_string(),
        StateConfig {
            label: "Fix and retry".to_string(),
            initial: None,
            terminal: None,
            params: None,
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
    assert!(output.contains("fix_and_retry"), "hyphens should be replaced with underscores");
    assert!(output.contains("\"fix-and-retry\""), "original name should appear in state label");
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
        gates: vec![
            GateConfig {
                gate_type: "any_of".to_string(),
                intent: None,
                gates: vec![
                    GateConfig {
                        gate_type: "file_exists".to_string(),
                        intent: None,
                        gates: vec![],
                        params: vec![("path".to_string(), toml::Value::String("a.txt".to_string()))]
                            .into_iter().collect(),
                    },
                    GateConfig {
                        gate_type: "file_exists".to_string(),
                        intent: None,
                        gates: vec![],
                        params: vec![("path".to_string(), toml::Value::String("b.txt".to_string()))]
                            .into_iter().collect(),
                    },
                ],
                params: std::collections::HashMap::new(),
            },
        ],
    });

    let output = mermaid::generate_mermaid(&config);
    assert!(output.contains("any_of(2)"), "composite gate should show abbreviated label");
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test test_mermaid 2>&1`
Expected: All 3 PASS.

- [ ] **Step 5: Commit**

```bash
git add src/mermaid.rs src/lib.rs tests/mermaid_tests.rs
git commit -m "feat: add Mermaid stateDiagram-v2 generation from protocol config"
```

---

## Task 8: ASCII tree-walk output

**Files:**
- Modify: `src/mermaid.rs`
- Test: `tests/mermaid_tests.rs`

- [ ] **Step 1: Write failing test for ASCII output**

Append to `tests/mermaid_tests.rs`:

```rust
#[test]
fn test_ascii_minimal_protocol() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let output = mermaid::generate_ascii(&config);

    assert!(output.contains("[idle]"), "should contain initial state");
    assert!(output.contains("(initial)"), "should mark initial state");
    assert!(output.contains("[done]"), "should contain terminal state");
    assert!(output.contains("(terminal)"), "should mark terminal state");
    assert!(output.contains("begin"), "should contain transition command");
    assert!(output.contains("complete"), "should contain transition command");
}

#[test]
fn test_ascii_cycle_detection() {
    use sahjhan::config::*;

    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    // Add a cycle: done → idle
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_ascii 2>&1`
Expected: FAIL — `generate_ascii` doesn't exist yet.

- [ ] **Step 3: Implement generate_ascii**

Add to `src/mermaid.rs`:

```rust
use std::collections::{HashMap, HashSet, VecDeque};

// [generate-ascii]
/// Generate an ASCII tree-walk diagram from a protocol config.
///
/// BFS from initial state. Each state shown with outgoing transitions
/// as tree branches. Cycles detected and marked with `(↑ cycle)`.
pub fn generate_ascii(config: &ProtocolConfig) -> String {
    let initial = config.initial_state().unwrap_or("idle");

    // Build adjacency: state → vec of (command, target, gates_summary)
    let mut adj: HashMap<&str, Vec<(&str, &str, String)>> = HashMap::new();
    for t in &config.transitions {
        let gates_str = if t.gates.is_empty() {
            String::new()
        } else {
            let parts: Vec<String> = t.gates.iter().map(|g| gate_short_label(g)).collect();
            parts.join(", ")
        };
        adj.entry(t.from.as_str())
            .or_default()
            .push((t.command.as_str(), t.to.as_str(), gates_str));
    }

    let mut output = String::new();
    let mut visited: HashSet<&str> = HashSet::new();
    let mut queue: VecDeque<(&str, String)> = VecDeque::new();

    queue.push_back((initial, String::new()));

    while let Some((state, prefix)) = queue.pop_front() {
        if visited.contains(state) {
            continue;
        }
        visited.insert(state);

        // State line
        let state_config = config.states.get(state);
        let mut annotations = Vec::new();
        if state_config.map(|s| s.initial.unwrap_or(false)).unwrap_or(false) {
            annotations.push("initial");
        }
        if state_config.map(|s| s.terminal.unwrap_or(false)).unwrap_or(false) {
            annotations.push("terminal");
        }
        let annotation_str = if annotations.is_empty() {
            String::new()
        } else {
            format!(" ({})", annotations.join(", "))
        };

        output.push_str(&format!("{}[{}]{}\n", prefix, state, annotation_str));

        // Outgoing transitions
        let transitions = adj.get(state).cloned().unwrap_or_default();
        let count = transitions.len();

        for (i, (command, target, gates_str)) in transitions.iter().enumerate() {
            let is_last = i == count - 1;
            let connector = if is_last { "└─" } else { "├─" };
            let child_prefix = if is_last {
                format!("{}   ", prefix)
            } else {
                format!("{}│  ", prefix)
            };

            if visited.contains(target) {
                // Cycle detected
                output.push_str(&format!(
                    "{}{} {} ──▶ [{}] (↑ cycle)\n",
                    prefix, connector, command, target
                ));
            } else {
                output.push_str(&format!(
                    "{}{} {} ──▶ ",
                    prefix, connector, command
                ));

                if !gates_str.is_empty() {
                    // Gates go on a sub-line after the transition target
                    // We need to push the target state into the queue with the child prefix
                    // but first print the arrow
                }

                // Enqueue target with the child prefix
                queue.push_back((target, child_prefix.clone()));

                // If there are gates, print them on a sub-line
                if !gates_str.is_empty() {
                    // The target will be printed by the queue, so just end the arrow line
                    // Actually, we need to print the target inline
                    // Let's restructure: print target inline, then gates below
                }
            }
        }
    }

    output.trim_end().to_string()
}
```

Wait — this recursive-in-BFS approach gets complicated with inline printing. Let me use a simpler recursive approach instead. Replace the `generate_ascii` function:

```rust
// [generate-ascii]
/// Generate an ASCII tree-walk diagram from a protocol config.
pub fn generate_ascii(config: &ProtocolConfig) -> String {
    let initial = config.initial_state().unwrap_or("idle");

    // Build adjacency: state → vec of (command, target, gates_summary)
    let mut adj: HashMap<&str, Vec<(&str, &str, String)>> = HashMap::new();
    for t in &config.transitions {
        let gates_str = if t.gates.is_empty() {
            String::new()
        } else {
            let parts: Vec<String> = t.gates.iter().map(|g| gate_short_label(g)).collect();
            parts.join(", ")
        };
        adj.entry(t.from.as_str())
            .or_default()
            .push((t.command.as_str(), t.to.as_str(), gates_str));
    }

    let mut output = String::new();
    let mut visited: HashSet<&str> = HashSet::new();

    fn walk<'a>(
        state: &'a str,
        prefix: &str,
        config: &'a ProtocolConfig,
        adj: &HashMap<&'a str, Vec<(&'a str, &'a str, String)>>,
        visited: &mut HashSet<&'a str>,
        output: &mut String,
    ) {
        visited.insert(state);

        let state_config = config.states.get(state);
        let mut annotations = Vec::new();
        if state_config.map(|s| s.initial.unwrap_or(false)).unwrap_or(false) {
            annotations.push("initial");
        }
        if state_config.map(|s| s.terminal.unwrap_or(false)).unwrap_or(false) {
            annotations.push("terminal");
        }
        let ann = if annotations.is_empty() {
            String::new()
        } else {
            format!(" ({})", annotations.join(", "))
        };
        output.push_str(&format!("[{}]{}\n", state, ann));

        let transitions = match adj.get(state) {
            Some(t) => t.clone(),
            None => return,
        };
        let count = transitions.len();

        for (i, (command, target, gates_str)) in transitions.iter().enumerate() {
            let is_last = i == count - 1;
            let connector = if is_last { "└─" } else { "├─" };
            let child_prefix = if is_last {
                format!("{}   ", prefix)
            } else {
                format!("{}│  ", prefix)
            };

            if visited.contains(target) {
                output.push_str(&format!(
                    "{}{} {} ──▶ [{}] (↑ cycle)\n",
                    prefix, connector, command, target
                ));
            } else {
                output.push_str(&format!("{}{} {} ──▶ ", prefix, connector, command));
                if !gates_str.is_empty() {
                    output.push_str(&format!("\n{}   │ {}\n{}   ", prefix, gates_str, child_prefix));
                }
                walk(target, &child_prefix, config, adj, visited, output);
            }
        }
    }

    walk(initial, " ", config, &adj, &mut visited, &mut output);

    output.trim_end().to_string()
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test test_ascii 2>&1`
Expected: Both PASS.

- [ ] **Step 5: Adjust output format if tests fail**

The exact output format may need tweaking to match the assertions. Run with `--nocapture` to see actual output and adjust either the implementation or the test assertions to match.

- [ ] **Step 6: Run full test suite**

Run: `cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/mermaid.rs tests/mermaid_tests.rs
git commit -m "feat: add ASCII tree-walk protocol diagram generation"
```

---

## Task 9: Wire up `sahjhan mermaid` CLI command

**Files:**
- Create: `src/cli/mermaid.rs`
- Modify: `src/cli/mod.rs`
- Modify: `src/main.rs:56-199,345-491`
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Create CLI command module**

Create `src/cli/mermaid.rs`:

```rust
// src/cli/mermaid.rs
//
// CLI command for protocol visualization.
//
// ## Index
// - [cmd-mermaid] cmd_mermaid() — generate Mermaid or ASCII diagram

use super::commands::{load_config, resolve_config_dir, EXIT_SUCCESS};

// [cmd-mermaid]
pub fn cmd_mermaid(config_dir: &str, rendered: bool) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    if rendered {
        println!("{}", crate::mermaid::generate_ascii(&config));
    } else {
        println!("{}", crate::mermaid::generate_mermaid(&config));
    }

    EXIT_SUCCESS
}
```

- [ ] **Step 2: Register in cli/mod.rs**

Add to `src/cli/mod.rs`:

```rust
pub mod mermaid;
```

- [ ] **Step 3: Add Mermaid subcommand to main.rs**

In `src/main.rs`, add to the `Commands` enum (after `Guards`):

```rust
    /// Generate protocol diagram (Mermaid or ASCII)
    Mermaid {
        /// Output ASCII art instead of raw Mermaid text
        #[arg(long)]
        rendered: bool,
    },
```

Add the import at the top with the other cli imports:

```rust
use sahjhan::cli::mermaid as mermaid_cmd;
```

Add the dispatch arm in the main match (after the `Commands::Guards` arm):

```rust
        Commands::Mermaid { rendered } => mermaid_cmd::cmd_mermaid(&cli.config_dir, rendered),
```

- [ ] **Step 4: Write integration test**

Append to `tests/integration_tests.rs`:

```rust
#[test]
fn test_cli_mermaid_raw() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "mermaid"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("stateDiagram-v2"))
        .stdout(predicate::str::contains("[*] --> idle"));
}

#[test]
fn test_cli_mermaid_rendered() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "mermaid", "--rendered"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("[idle]"))
        .stdout(predicate::str::contains("(initial)"));
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test test_cli_mermaid 2>&1`
Expected: Both PASS.

- [ ] **Step 6: Run full test suite**

Run: `cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/cli/mermaid.rs src/cli/mod.rs src/main.rs tests/integration_tests.rs
git commit -m "feat: add sahjhan mermaid CLI command with --rendered ASCII output"
```

---

## Task 10: Update README

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Update the TDD example protocol**

In the README, update the states.toml example to add `fix-and-retry`:

After the `[states.verifying]` block, add:

```toml
[states.fix-and-retry]
label = "Fix and retry"
```

Replace the transitions.toml example with the version from the spec that demonstrates gate composition and branching:

```toml
# tdd-protocol/transitions.toml
[[transitions]]
from = "idle"
to = "writing-tests"
command = "start"
gates = []

[[transitions]]
from = "writing-tests"
to = "implementing"
command = "tests-done"
gates = [
    { type = "file_exists", path = "tests/test_feature.py",
      intent = "test file must exist on disk before implementation begins" },
    { type = "any_of", intent = "tests must run or be explicitly overridden", gates = [
        { type = "command_succeeds", cmd = "python -m pytest tests/", timeout = 60 },
        { type = "ledger_has_event", event = "manual_test_override" },
    ]},
    { type = "set_covered", set = "suites",
      event = "set_member_complete", field = "member",
      intent = "every test suite must be written before implementing" },
]

# Happy path: tests pass + quality checks → advance
[[transitions]]
from = "implementing"
to = "verifying"
command = "submit"
gates = [
    { type = "command_succeeds", cmd = "python -m pytest tests/", timeout = 120,
      intent = "all tests must pass before verification" },
    { type = "k_of_n", k = 2, intent = "at least 2 of 3 code quality checks must pass", gates = [
        { type = "command_succeeds", cmd = "python -m mypy src/" },
        { type = "command_succeeds", cmd = "python -m pylint src/" },
        { type = "command_succeeds", cmd = "python -m bandit -r src/" },
    ]},
    { type = "no_violations", intent = "clean record — no tampering" },
]

# Fallback: tests fail → go fix them
[[transitions]]
from = "implementing"
to = "fix-and-retry"
command = "submit"
gates = []

# Recovery loop
[[transitions]]
from = "fix-and-retry"
to = "implementing"
command = "retry"
gates = []
```

- [ ] **Step 2: Add "Gate composition" section**

After the gate types table in the README, add a new section:

```markdown
## Gate composition

Gate lists on transitions are implicitly AND — every gate must pass. For more complex logic, use composite gates:

| Composite | What it does | Example |
|-----------|-------------|---------|
| `any_of` | Pass if any child passes (OR) | Either tests or manual approval |
| `all_of` | Pass if all children pass (explicit AND) | Useful nested inside `any_of` |
| `not` | Pass if child fails (NOT) | No regressions recorded |
| `k_of_n` | Pass if k+ children pass | 2 of 3 security scans |

Composite gates contain a `gates` array of child gates. Children can be leaf gates or other composites — nesting is unlimited.

\`\`\`toml
# At least 2 of 3 static analysis tools must pass
{ type = "k_of_n", k = 2, intent = "code quality consensus", gates = [
    { type = "command_succeeds", cmd = "python -m mypy src/" },
    { type = "command_succeeds", cmd = "python -m pylint src/" },
    { type = "command_succeeds", cmd = "python -m bandit -r src/" },
]}

# Either automated tests OR a manual override event
{ type = "any_of", intent = "tests or override", gates = [
    { type = "command_succeeds", cmd = "pytest" },
    { type = "ledger_has_event", event = "manual_test_override" },
]}

# No regression events in the ledger
{ type = "not", intent = "no regressions", gates = [
    { type = "ledger_has_event", event = "regression" },
]}
\`\`\`
```

- [ ] **Step 3: Add "Conditional transitions" section**

After the gate composition section:

```markdown
## Conditional transitions

Multiple transitions can share the same `from` state and `command` name. When the command is invoked, Sahjhan tries each candidate in TOML declaration order. The first one whose gates all pass is taken.

\`\`\`toml
# Happy path: tests pass → advance
[[transitions]]
from = "implementing"
to = "verifying"
command = "submit"
gates = [
    { type = "command_succeeds", cmd = "python -m pytest tests/" },
]

# Fallback: tests fail → error recovery
[[transitions]]
from = "implementing"
to = "fix-and-retry"
command = "submit"
gates = []
\`\`\`

A gateless transition at the end acts as a catch-all. Without it, the agent gets stuck if all candidates' gates fail. `sahjhan validate` warns when branching transitions have no fallback.

This replaces the pattern of creating separate commands for each path. Instead of `submit-if-passing` and `submit-if-failing`, you write one `submit` command that routes automatically.

Gate check shows all candidates:

\`\`\`bash
sahjhan gate check submit
# gate-check: submit
#   candidate 1: implementing → verifying
#     ✗ command_succeeds: 'pytest' exit 1
#   candidate 2: implementing → fix-and-retry
#     (no gates)
#   result: would take → fix-and-retry
\`\`\`
```

- [ ] **Step 4: Add mermaid to CLI reference**

In the CLI reference section, add:

```
sahjhan mermaid                           Generate Mermaid stateDiagram-v2
sahjhan mermaid --rendered                ASCII art protocol diagram
```

- [ ] **Step 5: Update the enforcement walkthrough**

Extend the existing walkthrough section to show a failed submit routing to fix-and-retry:

After the existing `# implementing → verifying` line, add:

```bash
# Or, if tests fail:
sahjhan --config-dir tdd-protocol transition submit
# implementing → fix-and-retry (fallback — tests didn't pass)

# Fix the code, then retry
sahjhan --config-dir tdd-protocol transition retry
# fix-and-retry → implementing

# Try again
sahjhan --config-dir tdd-protocol transition submit
# implementing → verifying
```

- [ ] **Step 6: Update the ASCII diagram at the top of the example**

Replace the existing ASCII box diagram with one that includes fix-and-retry and the new gate types:

```
states.toml            transitions.toml              events.toml
┌──────────────┐       ┌────────────────────┐        ┌──────────────────┐
│ idle         │◀─from─┤ start              │        │ finding          │
│ writing-tests│◀─to───┤                    │        │   severity       │
│ implementing │       │ tests-done         │        │   file           │
│ fix-and-retry│       │   file_exists      │        │                  │
│ verifying    │       │   any_of ──────────┼──OR──▶ │ manual_test_     │
└──────────────┘       │   set_covered──┐   │        │   override       │
                       │                │   │        │                  │
protocol.toml          │ submit (1)     │   │        │ set_member_      │
┌──────────────┐       │   command_succ. │   │        │   complete       │
│ sets:        │◀──────┼───k_of_n(2/3)  │   │        │   set, member    │
│   suites:    │       │   no_violations │   │        └──────────────────┘
│   - unit     │       │                │   │               ▲
│   - integr.  │       │ submit (2)     │   │               │
└──────────────┘       │   (fallback)───┼───┼──▶ fix-and-retry
                       │                │   │
                       │ retry ─────────┘   │
                       └─────────┬──────────┘
                                 │trigger
                       ┌─────────┴──────────┐
                       │ STATUS.md          │
                       │   on_transition    │
                       │ FINDINGS.md        │
                       │   on_event [finding]│
                       └────────────────────┘
                       renders.toml
```

- [ ] **Step 7: Commit**

```bash
git add README.md
git commit -m "docs: update README with gate composition, branching, and mermaid examples"
```

---

## Task 11: Update CLAUDE.md documentation

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update the Gate Types section in Gate Evaluation table**

Add composite gate types to the gates table:

```
| Composite gate dispatch | `gates/types.rs` | `[eval]` | Routes any_of, all_of, not, k_of_n |
```

- [ ] **Step 2: Update the Gate Dispatch flow map**

Add the new gate types to the dispatch list:

```
  "any_of"              → gates/types.rs   [eval] (recursive)
  "all_of"              → gates/types.rs   [eval] (recursive)
  "not"                 → gates/types.rs   [eval] (recursive)
  "k_of_n"              → gates/types.rs   [eval] (recursive)
```

- [ ] **Step 3: Update the Transition Lifecycle flow map**

Update the gate evaluation section to mention multi-candidate matching:

```
      → for each candidate transition (in TOML order):
        → for each gate:
          ...
        → if all gates pass: take this candidate, break
        → if any fail: try next candidate
      → if no candidate passed: error
```

- [ ] **Step 4: Add mermaid entries to Module Lookup Tables**

Add a new section for the mermaid module:

```
### mermaid/ — Protocol Visualization

| Concept | File | Anchor | Purpose |
|---------|------|--------|---------|
| Mermaid generator | `mermaid.rs` | `[generate-mermaid]` | stateDiagram-v2 text from config |
| ASCII generator | `mermaid.rs` | `[generate-ascii]` | ASCII tree-walk diagram |
```

Add CLI entry:

```
| Mermaid | `cli/mermaid.rs` | `[cmd-mermaid]` | Diagram generation command |
```

- [ ] **Step 5: Update State Machine table**

Add the new error variant:

```
| All blocked error | `state/machine.rs` | `StateError::AllCandidatesBlocked` | Multi-candidate transition failure |
```

- [ ] **Step 6: Update Test Files table**

Add:

```
| `tests/mermaid_tests.rs` | Mermaid and ASCII diagram generation |
```

- [ ] **Step 7: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md with composite gates, branching, and mermaid modules"
```

---

## Task 12: Final verification

- [ ] **Step 1: Run clippy**

Run: `cargo clippy -- -D warnings 2>&1`
Expected: No warnings.

- [ ] **Step 2: Run fmt**

Run: `cargo fmt -- --check 2>&1`
Expected: No formatting issues. If there are, run `cargo fmt` and commit.

- [ ] **Step 3: Run full test suite**

Run: `cargo test 2>&1`
Expected: All tests pass.

- [ ] **Step 4: Manual smoke test — mermaid output**

Run: `cargo run -- --config-dir examples/minimal mermaid 2>&1`
Expected: Mermaid stateDiagram-v2 text with idle, working, done states.

Run: `cargo run -- --config-dir examples/minimal mermaid --rendered 2>&1`
Expected: ASCII tree diagram with `[idle] (initial)` and `[done] (terminal)`.

- [ ] **Step 5: Fix any issues found and commit**

If clippy/fmt/tests found issues, fix them and commit:

```bash
git add -A
git commit -m "chore: fix clippy warnings and formatting"
```
