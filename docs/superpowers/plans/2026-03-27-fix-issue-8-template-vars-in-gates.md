# Fix Issue #8: Template Variables in Gates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make template variables like `{{current_perspective}}` resolve to set-derived values in `command_succeeds` and `query` gates.

**Architecture:** Extend `StateParam` with an optional `source` field (`"values"` | `"current"` | `"last_completed"`) that controls how the param value is derived from set state. Add template interpolation to the `query` gate. Both `build_state_params` call sites (machine.rs and commands.rs) gain ledger access to compute set completion status.

**Tech Stack:** Rust, serde, toml

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `src/config/states.rs` | Modify | Add `source` field to `StateParam` |
| `src/state/machine.rs:214-228` | Modify | Update `build_state_params` to handle `source` with ledger |
| `src/cli/commands.rs:275-290` | Modify | Update standalone `build_state_params` to accept `&Ledger` and handle `source` |
| `src/cli/transition.rs:209` | Modify | Pass ledger to `build_state_params` |
| `src/config/mod.rs:161-173` | Modify | Add validation for `source` field values |
| `src/gates/query.rs:12-13` | Modify | Add template interpolation to SQL |
| `src/config/mod.rs:208-220` | Modify | Add `query` to `known_gates` in `validate_deep` |
| `tests/gate_tests.rs` | Modify | Add tests for `source` param and query template interpolation |

---

### Task 1: Extend `StateParam` with `source` field

**Files:**
- Modify: `src/config/states.rs:20-25`

- [ ] **Step 1: Write the failing test**

Add to `tests/gate_tests.rs` at the end of the file:

```rust
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_state_param_source_current -- --nocapture 2>&1`
Expected: Compilation error — `StateParam` has no `source` field.

- [ ] **Step 3: Add `source` field to `StateParam`**

In `src/config/states.rs`, change `StateParam`:

```rust
/// A parameter bound to a set (used for state context).
///
/// The `source` field controls how the value is derived:
/// - `"values"` (default): comma-joined set values
/// - `"current"`: first incomplete member of the set
/// - `"last_completed"`: most recently completed member of the set
#[derive(Debug, Deserialize, Clone)]
pub struct StateParam {
    pub name: String,
    pub set: String,
    pub source: Option<String>,
}
```

- [ ] **Step 4: Run test to verify it compiles but fails**

Run: `cargo test test_state_param_source_current -- --nocapture 2>&1`
Expected: FAIL — gate fails because `build_state_params` still uses comma-joined values, producing `"tests,lint"` instead of `"lint"`.

- [ ] **Step 5: Commit**

```bash
git add src/config/states.rs tests/gate_tests.rs
git commit -m "test: add failing test for StateParam source=current (#8)"
```

---

### Task 2: Update `StateMachine::build_state_params` to handle `source`

**Files:**
- Modify: `src/state/machine.rs:214-228`

- [ ] **Step 1: Update `build_state_params` in `machine.rs`**

Replace the `build_state_params` method (lines 214-228) with:

```rust
    /// Build state_params from a state's param definitions.
    ///
    /// For each `StateParam` in the target state config, the param name is
    /// mapped to a value derived from the set according to `source`:
    /// - `"values"` (default): comma-joined set values
    /// - `"current"`: first incomplete member of the set
    /// - `"last_completed"`: most recently completed member of the set
    fn build_state_params(&self, state_name: &str) -> HashMap<String, String> {
        let mut params = HashMap::new();

        if let Some(state_config) = self.config.states.get(state_name) {
            if let Some(state_params) = &state_config.params {
                for param in state_params {
                    let source = param.source.as_deref().unwrap_or("values");
                    match source {
                        "current" => {
                            if let Some(set_config) = self.config.sets.get(&param.set) {
                                let completed = self.completed_members_for_set(
                                    &param.set,
                                    "set_member_complete",
                                    "member",
                                );
                                if let Some(current) = set_config
                                    .values
                                    .iter()
                                    .find(|v| !completed.contains(v))
                                {
                                    params.insert(param.name.clone(), current.clone());
                                }
                            }
                        }
                        "last_completed" => {
                            let completed = self.completed_members_for_set(
                                &param.set,
                                "set_member_complete",
                                "member",
                            );
                            if let Some(last) = completed.last() {
                                params.insert(param.name.clone(), last.clone());
                            }
                        }
                        _ => {
                            // Default: comma-joined set values
                            if let Some(set_config) = self.config.sets.get(&param.set) {
                                params.insert(param.name.clone(), set_config.values.join(","));
                            }
                        }
                    }
                }
            }
        }

        params
    }
```

- [ ] **Step 2: Run the test**

Run: `cargo test test_state_param_source_current -- --nocapture 2>&1`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add src/state/machine.rs
git commit -m "feat: StateMachine::build_state_params handles source field (#8)"
```

---

### Task 3: Add test and implementation for `source = "last_completed"`

**Files:**
- Modify: `tests/gate_tests.rs`

- [ ] **Step 1: Write the failing test**

Add to `tests/gate_tests.rs`:

```rust
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
```

- [ ] **Step 2: Run tests**

Run: `cargo test test_state_param_source_last_completed -- --nocapture 2>&1`
Expected: PASS (implementation from Task 2 handles this).

- [ ] **Step 3: Add test for default source (backwards compatibility)**

Add to `tests/gate_tests.rs`:

```rust
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
```

- [ ] **Step 4: Run tests**

Run: `cargo test test_state_param_source_default -- --nocapture 2>&1`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add tests/gate_tests.rs
git commit -m "test: add tests for source=last_completed and default source (#8)"
```

---

### Task 4: Update standalone `build_state_params` in `commands.rs`

**Files:**
- Modify: `src/cli/commands.rs:275-290`
- Modify: `src/cli/transition.rs:209`

- [ ] **Step 1: Verify the standalone `build_state_params` call site needs updating**

In `src/cli/transition.rs:209`, `cmd_gate_check` calls `build_state_params(&config, &transition.to)` — this needs a `&Ledger` parameter to support the `source` field. Verify the compiler catches this after updating `commands.rs`.

- [ ] **Step 3: Update `build_state_params` in `commands.rs`**

In `src/cli/commands.rs`, replace the `build_state_params` function (lines 273-290) with:

```rust
// [build-state-params]
/// Build state_params for a target state (mirrors StateMachine::build_state_params).
///
/// Supports `StateParam.source`:
/// - `"values"` (default): comma-joined set values
/// - `"current"`: first incomplete member of the set (requires ledger scan)
/// - `"last_completed"`: most recently completed member (requires ledger scan)
pub fn build_state_params(
    config: &ProtocolConfig,
    state_name: &str,
    ledger: &crate::ledger::chain::Ledger,
) -> HashMap<String, String> {
    let mut params = HashMap::new();
    if let Some(state_config) = config.states.get(state_name) {
        if let Some(state_params) = &state_config.params {
            for param in state_params {
                let source = param.source.as_deref().unwrap_or("values");
                match source {
                    "current" => {
                        if let Some(set_config) = config.sets.get(&param.set) {
                            let completed =
                                completed_members_for_set(ledger, &param.set);
                            if let Some(current) = set_config
                                .values
                                .iter()
                                .find(|v| !completed.contains(v))
                            {
                                params.insert(param.name.clone(), current.clone());
                            }
                        }
                    }
                    "last_completed" => {
                        let completed =
                            completed_members_for_set(ledger, &param.set);
                        if let Some(last) = completed.last() {
                            params.insert(param.name.clone(), last.clone());
                        }
                    }
                    _ => {
                        if let Some(set_config) = config.sets.get(&param.set) {
                            params.insert(param.name.clone(), set_config.values.join(","));
                        }
                    }
                }
            }
        }
    }
    params
}

/// Scan ledger for completed members of a set.
fn completed_members_for_set(
    ledger: &crate::ledger::chain::Ledger,
    set_name: &str,
) -> Vec<String> {
    let mut covered = Vec::new();
    for entry in ledger.events_of_type("set_member_complete") {
        let set_matches = entry
            .fields
            .get("set")
            .map(|v| v.as_str() == set_name)
            .unwrap_or(false);
        if set_matches {
            if let Some(member) = entry.fields.get("member") {
                if !covered.contains(member) {
                    covered.push(member.clone());
                }
            }
        }
    }
    covered
}
```

- [ ] **Step 4: Update call site in `transition.rs`**

In `src/cli/transition.rs`, line 209, change:

```rust
    let mut state_params = build_state_params(&config, &transition.to);
```

to:

```rust
    let mut state_params = build_state_params(&config, &transition.to, machine.ledger());
```

- [ ] **Step 5: Run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All tests pass (no regressions). The existing `test_state_param_source_current` test exercises the full path through `StateMachine::build_state_params`; the standalone version in `commands.rs` mirrors the same logic.

- [ ] **Step 6: Commit**

```bash
git add src/cli/commands.rs src/cli/transition.rs
git commit -m "feat: standalone build_state_params handles source field (#8)"
```

---

### Task 5: Add template interpolation to the `query` gate

**Files:**
- Modify: `src/gates/query.rs:12-13`
- Modify: `tests/gate_tests.rs`

- [ ] **Step 1: Write the failing test**

Add to `tests/gate_tests.rs`:

```rust
// ---------------------------------------------------------------------------
// query gate — template interpolation
// ---------------------------------------------------------------------------

#[test]
fn test_query_gate_interpolates_template_vars() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Append events with a "category" field
    let mut fields = BTreeMap::new();
    fields.insert("category".to_string(), "alpha".to_string());
    ledger.append("tagged_event", fields).unwrap();

    let mut fields = BTreeMap::new();
    fields.insert("category".to_string(), "alpha".to_string());
    ledger.append("tagged_event", fields).unwrap();

    // Query using {{target_category}} template var — should be interpolated
    let gate = make_gate(
        "query",
        vec![
            (
                "sql",
                toml::Value::String(
                    "SELECT count(*) >= 2 as result FROM events WHERE event_type = 'tagged_event' AND category = '{{target_category}}'".to_string(),
                ),
            ),
            ("expect", toml::Value::String("true".to_string())),
        ],
    );

    let mut state_params = HashMap::new();
    state_params.insert("target_category".to_string(), "alpha".to_string());

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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_query_gate_interpolates_template_vars -- --nocapture 2>&1`
Expected: FAIL — the literal `{{target_category}}` is passed to SQL, producing no matching rows.

- [ ] **Step 3: Add template interpolation to `query.rs`**

In `src/gates/query.rs`, add imports at the top:

```rust
use super::template::resolve_template_plain;
use super::types::{build_template_vars, validate_template_fields};
```

Then replace lines 12-22 (the `sql` extraction) with:

```rust
    let raw_sql = match gate.params.get("sql").and_then(|v| v.as_str()) {
        Some(s) => s.to_string(),
        None => {
            return GateResult {
                passed: false,
                gate_type: "query".to_string(),
                description: "SQL query against ledger".to_string(),
                reason: Some("gate missing required 'sql' param".to_string()),
            }
        }
    };

    // Validate template fields before interpolation.
    if let Err(reason) = validate_template_fields(&raw_sql, ctx) {
        return GateResult {
            passed: false,
            gate_type: "query".to_string(),
            description: format!("SQL: {}", raw_sql),
            reason: Some(reason),
        };
    }

    // Interpolate template variables (plain — no shell escaping for SQL).
    let vars = build_template_vars(ctx);
    let sql = resolve_template_plain(&raw_sql, &vars);
```

And update the rest of the function to use `sql` (the interpolated version) instead of `sql` (the raw version). The `description` on line 31 should use `sql` as well. The `sql_clone` on line 51 should clone `sql`.

The full updated function body after the `sql` derivation:

```rust
    let expect = gate
        .params
        .get("expect")
        .and_then(|v| v.as_str())
        .unwrap_or("true")
        .to_string();

    let description = format!("SQL: {}", sql);

    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            return GateResult {
                passed: false,
                gate_type: "query".to_string(),
                description,
                reason: Some(format!("failed to build tokio runtime: {}", e)),
            }
        }
    };

    let ledger_path = ctx.ledger.path().to_path_buf();
    let sql_clone = sql.clone();
    let events_config = ctx.config.events.clone();
    let results = rt.block_on(async {
        let engine = crate::query::QueryEngine::from_config(&events_config);
        engine.query_file(&ledger_path, &sql_clone).await
    });

    let rows = match results {
        Ok(r) => r,
        Err(e) => {
            return GateResult {
                passed: false,
                gate_type: "query".to_string(),
                description,
                reason: Some(format!("query execution failed: {}", e)),
            }
        }
    };

    let actual = rows
        .first()
        .and_then(|row| row.values().next())
        .map(|v| v.to_string())
        .unwrap_or_else(|| "null".to_string());

    let passed = actual == expect;

    GateResult {
        passed,
        gate_type: "query".to_string(),
        description,
        reason: Some(if passed {
            format!("query returned '{}'", actual)
        } else {
            format!("query returned '{}', expected '{}'", actual, expect)
        }),
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test test_query_gate_interpolates_template_vars -- --nocapture 2>&1`
Expected: PASS

Run: `cargo test test_query_gate -- --nocapture 2>&1`
Expected: All existing query gate tests still pass.

- [ ] **Step 5: Commit**

```bash
git add src/gates/query.rs tests/gate_tests.rs
git commit -m "feat: query gate interpolates template variables (#8)"
```

---

### Task 6: Add `query` to `validate_deep` known gates

**Files:**
- Modify: `src/config/mod.rs:208-220`

- [ ] **Step 1: Add `query` to the `known_gates` map**

In `src/config/mod.rs`, find the `known_gates` HashMap (line 208) and add:

```rust
            ("query", vec!["sql"]),
```

after the `("snapshot_compare", ...)` entry.

- [ ] **Step 2: Run tests**

Run: `cargo test 2>&1 | tail -5`
Expected: All pass.

- [ ] **Step 3: Commit**

```bash
git add src/config/mod.rs
git commit -m "fix: add query to validate_deep known gates (#8)"
```

---

### Task 7: Add validation for `StateParam.source` values

**Files:**
- Modify: `src/config/mod.rs:161-173`

- [ ] **Step 1: Write the failing test**

Add to `tests/gate_tests.rs`:

```rust
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
        errors.iter().any(|e| e.contains("source") && e.contains("bogus")),
        "should reject invalid source value, got: {:?}",
        errors
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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_validate_rejects_invalid_source -- --nocapture 2>&1`
Expected: FAIL — no validation for `source` exists yet.

- [ ] **Step 3: Add validation**

In `src/config/mod.rs`, in the `validate()` method, after the block that checks state param set references (section 4, around line 173), add:

```rust
        // 4b. State param source values are valid.
        let valid_sources = ["values", "current", "last_completed"];
        for (state_name, state) in &self.states {
            if let Some(params) = &state.params {
                for p in params {
                    if let Some(ref source) = p.source {
                        if !valid_sources.contains(&source.as_str()) {
                            errors.push(format!(
                                "state '{}' param '{}' has invalid source '{}' (valid: {})",
                                state_name,
                                p.name,
                                source,
                                valid_sources.join(", ")
                            ));
                        }
                    }
                }
            }
        }
```

- [ ] **Step 4: Run tests**

Run: `cargo test test_validate_rejects_invalid_source test_validate_accepts_valid_sources -- --nocapture 2>&1`
Expected: Both PASS.

- [ ] **Step 5: Commit**

```bash
git add src/config/mod.rs tests/gate_tests.rs
git commit -m "feat: validate StateParam source field values (#8)"
```

---

### Task 8: Run full test suite and format

- [ ] **Step 1: Format code**

Run: `cargo fmt`

- [ ] **Step 2: Run full test suite**

Run: `cargo test 2>&1`
Expected: All tests pass, no regressions.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -- -D warnings 2>&1`
Expected: No warnings.

- [ ] **Step 4: Commit any formatting changes**

```bash
git add -A
git commit -m "chore: cargo fmt (#8)"
```
