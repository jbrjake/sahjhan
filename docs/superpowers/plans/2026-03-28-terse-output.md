# Terse Output Overhaul Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite all CLI output to minimize tokens while maximizing agent self-documentation — terse structured text, intent-aware gate rejections, one-call decision tree in `status`.

**Architecture:** Add `intent` field to `GateConfig`, thread it through `GateResult`, then rewrite every CLI command's print statements to emit terse structured text. No new flags, no new modes. Integration tests updated to match new output.

**Tech Stack:** Rust, serde/toml, clap, assert_cmd/predicates (tests)

**Spec:** `docs/superpowers/specs/2026-03-28-terse-output-design.md`

---

### Task 1: Add `intent` to GateConfig and GateResult

**Files:**
- Modify: `src/config/transitions.rs:41-47`
- Modify: `src/gates/evaluator.rs:42-51`
- Modify: `src/gates/types.rs:26-47`
- Modify: `src/gates/file.rs:27-36,62-72`
- Modify: `src/gates/ledger.rs:55-67,102-114,180-193,226-231,243-255,267-279,298-316`
- Modify: `src/gates/command.rs` (all GateResult constructors)
- Modify: `src/gates/snapshot.rs` (all GateResult constructors)
- Modify: `src/gates/query.rs` (all GateResult constructors)
- Test: `tests/gate_tests.rs`

- [ ] **Step 1: Write the failing test**

Add to `tests/gate_tests.rs` near the top, after existing imports:

```rust
#[test]
fn test_gate_result_has_intent_from_config() {
    // A gate with an explicit intent field should surface it in GateResult
    let dir = tempdir().unwrap();
    let config = load_test_config(dir.path());
    let ledger = init_test_ledger(dir.path(), &config);
    let machine = StateMachine::new(&config, ledger);

    let gate = GateConfig {
        gate_type: "file_exists".to_string(),
        intent: Some("spec must have real content".to_string()),
        params: {
            let mut m = HashMap::new();
            m.insert("path".to_string(), toml::Value::String("/nonexistent".to_string()));
            m
        },
    };

    let ctx = GateContext {
        ledger: machine.ledger(),
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };

    let result = evaluate_gate(&gate, &ctx);
    assert!(!result.passed);
    assert_eq!(result.intent.as_deref(), Some("spec must have real content"));
}

#[test]
fn test_gate_result_has_default_intent_when_missing() {
    let dir = tempdir().unwrap();
    let config = load_test_config(dir.path());
    let ledger = init_test_ledger(dir.path(), &config);
    let machine = StateMachine::new(&config, ledger);

    let gate = GateConfig {
        gate_type: "file_exists".to_string(),
        intent: None,
        params: {
            let mut m = HashMap::new();
            m.insert("path".to_string(), toml::Value::String("/nonexistent".to_string()));
            m
        },
    };

    let ctx = GateContext {
        ledger: machine.ledger(),
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };

    let result = evaluate_gate(&gate, &ctx);
    assert!(!result.passed);
    assert_eq!(result.intent.as_deref(), Some("required files must exist before proceeding"));
}
```

Note: The test helpers `load_test_config` and `init_test_ledger` already exist in `gate_tests.rs`. Check the file for exact names — they may be called `setup_config` or similar. Match the existing pattern.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_gate_result_has_intent -- --nocapture`
Expected: FAIL — `GateConfig` has no field `intent`, `GateResult` has no field `intent`

- [ ] **Step 3: Add `intent` field to GateConfig**

In `src/config/transitions.rs`, add the `intent` field before the `#[serde(flatten)]`:

```rust
#[derive(Debug, Deserialize, Clone)]
pub struct GateConfig {
    #[serde(rename = "type")]
    pub gate_type: String,
    /// Why this gate exists — shown to agents on rejection.
    pub intent: Option<String>,
    #[serde(flatten)]
    pub params: HashMap<String, toml::Value>,
}
```

- [ ] **Step 4: Add `intent` field to GateResult and `default_intent` function**

In `src/gates/evaluator.rs`, add the field and the function:

```rust
/// The outcome of evaluating a single gate.
pub struct GateResult {
    pub passed: bool,
    pub gate_type: String,
    pub description: String,
    pub reason: Option<String>,
    /// Why this gate exists — from config or inferred from gate type.
    pub intent: Option<String>,
}

/// Default intent string for a gate type when no explicit intent is configured.
pub fn default_intent(gate_type: &str) -> &str {
    match gate_type {
        "file_exists" | "files_exist" => "required files must exist before proceeding",
        "command_succeeds" => "command must pass before proceeding",
        "command_output" => "command output must match expected value",
        "ledger_has_event" => "required events must be recorded first",
        "ledger_has_event_since" => "required events must occur since last transition",
        "set_covered" => "all set members must be completed",
        "min_elapsed" => "minimum time must elapse before proceeding",
        "no_violations" => "all protocol violations must be resolved",
        "field_not_empty" => "required field must have a value",
        "snapshot_compare" => "snapshot must match expected state",
        "query" => "query condition must be satisfied",
        _ => "gate condition must be met",
    }
}
```

- [ ] **Step 5: Thread intent through the dispatch function**

In `src/gates/types.rs`, update the `eval` function to inject intent into the result:

```rust
pub fn eval(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let mut result = match gate.gate_type.as_str() {
        "file_exists" => super::file::eval_file_exists(gate, ctx),
        "files_exist" => super::file::eval_files_exist(gate, ctx),
        "command_succeeds" => super::command::eval_command_succeeds(gate, ctx),
        "command_output" => super::command::eval_command_output(gate, ctx),
        "ledger_has_event" => super::ledger::eval_ledger_has_event(gate, ctx),
        "ledger_has_event_since" => super::ledger::eval_ledger_has_event_since(gate, ctx),
        "set_covered" => super::ledger::eval_set_covered(gate, ctx),
        "min_elapsed" => super::ledger::eval_min_elapsed(gate, ctx),
        "no_violations" => super::ledger::eval_no_violations(gate, ctx),
        "field_not_empty" => super::ledger::eval_field_not_empty(gate, ctx),
        "snapshot_compare" => super::snapshot::eval_snapshot_compare(gate, ctx),
        "query" => super::query::eval_query_gate(gate, ctx),
        other => GateResult {
            passed: false,
            gate_type: other.to_string(),
            description: format!("unknown gate type '{}'", other),
            reason: Some(format!("gate type '{}' is not implemented", other)),
            intent: None,
        },
    };
    // Inject intent: explicit from config, else default for gate type
    result.intent = Some(
        gate.intent
            .clone()
            .unwrap_or_else(|| super::evaluator::default_intent(&gate.gate_type).to_string()),
    );
    result
}
```

- [ ] **Step 6: Add `intent: None` to every GateResult constructor in gate modules**

Every gate module constructs `GateResult` directly. Add `intent: None` to each constructor. The `eval()` wrapper in `types.rs` will overwrite it, but the struct literal requires all fields.

Files to update:
- `src/gates/file.rs` — 2 constructors (eval_file_exists, eval_files_exist)
- `src/gates/ledger.rs` — 9 constructors (eval_ledger_has_event, eval_ledger_has_event_since, eval_set_covered x3, eval_min_elapsed x2, eval_no_violations, eval_field_not_empty x3)
- `src/gates/command.rs` — all GateResult constructors
- `src/gates/snapshot.rs` — all GateResult constructors
- `src/gates/query.rs` — all GateResult constructors

For each, add `intent: None,` as the last field in every `GateResult { ... }` block.

Example for `src/gates/file.rs` `eval_file_exists`:

```rust
GateResult {
    passed: exists,
    gate_type: "file_exists".to_string(),
    description: format!("file '{}' exists", resolved),
    reason: if exists {
        None
    } else {
        Some(format!("file '{}' does not exist", resolved))
    },
    intent: None,
}
```

- [ ] **Step 7: Run tests to verify intent flows through**

Run: `cargo test test_gate_result_has_intent -- --nocapture`
Expected: Both tests PASS

- [ ] **Step 8: Run full test suite**

Run: `cargo test`
Expected: All existing tests pass (the new `intent` field with `None` default doesn't break anything)

- [ ] **Step 9: Commit**

```bash
git add src/config/transitions.rs src/gates/evaluator.rs src/gates/types.rs src/gates/file.rs src/gates/ledger.rs src/gates/command.rs src/gates/snapshot.rs src/gates/query.rs tests/gate_tests.rs
git commit -m "feat: add intent field to GateConfig and GateResult"
```

---

### Task 2: Rewrite status command output

**Files:**
- Modify: `src/cli/status.rs:30-214` (cmd_status), `220-260` (cmd_set_status)
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Write the failing test**

Add to `tests/integration_tests.rs`:

```rust
#[test]
fn test_status_terse_format() {
    let dir = setup_initialized_dir();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Must start with "state:"
    assert!(stdout.starts_with("state:"), "stdout was: {}", stdout);
    // Must contain "next:" section
    assert!(stdout.contains("next:"), "stdout was: {}", stdout);
    // Must NOT contain decoration bars
    assert!(!stdout.contains("===="), "stdout was: {}", stdout);
    // Must NOT contain "State:" (old capitalized format)
    assert!(!stdout.contains("State:"), "stdout was: {}", stdout);
    // Must NOT contain "Ledger:" or "Manifest:" lines
    assert!(!stdout.contains("Ledger:"), "stdout was: {}", stdout);
    assert!(!stdout.contains("Manifest:"), "stdout was: {}", stdout);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_status_terse_format -- --nocapture`
Expected: FAIL — current output starts with "=====" and contains "State:"

- [ ] **Step 3: Rewrite cmd_status**

Replace the entire body of `cmd_status` in `src/cli/status.rs` (lines 31-213) with:

```rust
pub fn cmd_status(config_dir: &str, targeting: &LedgerTargeting) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("error: {}", msg);
            return code;
        }
    };

    let (ledger, mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("error: {}", msg);
            return code;
        }
    };

    // Event-only ledger: minimal status
    if let Some(LedgerMode::EventOnly) = mode {
        let chain_status = match ledger.verify() {
            Ok(()) => "chain valid",
            Err(_) => "chain INVALID",
        };
        println!("event-only: {} events, {}", ledger.len(), chain_status);
        return EXIT_SUCCESS;
    }

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let _manifest = match load_manifest(&data_dir) {
        Ok(m) => m,
        Err((code, msg)) => {
            eprintln!("error: {}", msg);
            return code;
        }
    };

    let machine = StateMachine::new(&config, ledger);
    let current_state = machine.current_state().to_string();
    let ledger_len = machine.ledger().len();
    let chain_status = match machine.ledger().verify() {
        Ok(()) => "chain valid".to_string(),
        Err(e) => format!("chain INVALID ({})", e),
    };

    println!("state: {} ({} events, {})", current_state, ledger_len, chain_status);

    // Sets: one line each
    let set_keys: Vec<_> = config.sets.keys().collect();
    if !set_keys.is_empty() {
        println!("sets:");
        for set_name in &set_keys {
            let status = machine.set_status(set_name);
            let members: Vec<String> = status
                .members
                .iter()
                .map(|m| {
                    let marker = if m.done { "\u{2713}" } else { "\u{00B7}" };
                    format!("{} {}", marker, m.name)
                })
                .collect();
            println!(
                "  {}: {}/{} [{}]",
                set_name,
                status.completed,
                status.total,
                members.join(", ")
            );
        }
    }

    // Transitions from current state with gate results
    let available: Vec<_> = config
        .transitions
        .iter()
        .filter(|t| t.from == current_state)
        .collect();

    if !available.is_empty() {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        println!("next:");
        for transition in &available {
            if transition.gates.is_empty() {
                println!("  {}: ready", transition.command);
            } else {
                let state_params =
                    build_state_params(&config, &transition.to, machine.ledger());
                let ctx = GateContext {
                    ledger: machine.ledger(),
                    config: &config,
                    current_state: &current_state,
                    state_params,
                    working_dir: cwd.clone(),
                    event_fields: None,
                };
                let results = evaluate_gates(&transition.gates, &ctx);
                let all_pass = results.iter().all(|r| r.passed);
                let label = if all_pass { "ready" } else { "blocked" };
                println!("  {}: {}", transition.command, label);
                for r in &results {
                    let marker = if r.passed { "\u{2713}" } else { "\u{2717}" };
                    if r.passed {
                        println!("    {} {}", marker, r.description);
                    } else {
                        let intent = r
                            .intent
                            .as_deref()
                            .unwrap_or("gate condition must be met");
                        println!(
                            "    {} {}: {} \u{2014} {}",
                            marker,
                            r.gate_type,
                            r.reason.as_deref().unwrap_or("failed"),
                            intent
                        );
                    }
                }
            }
        }
    }

    EXIT_SUCCESS
}
```

- [ ] **Step 4: Rewrite cmd_set_status**

Replace `cmd_set_status` body in `src/cli/status.rs`:

```rust
pub fn cmd_set_status(config_dir: &str, set_name: &str, targeting: &LedgerTargeting) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("error: {}", msg);
            return code;
        }
    };

    if !config.sets.contains_key(set_name) {
        eprintln!("error: unknown set '{}'", set_name);
        return EXIT_USAGE_ERROR;
    }

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("error: {}", msg);
            return code;
        }
    };

    let machine = StateMachine::new(&config, ledger);
    let status = machine.set_status(set_name);
    let members: Vec<String> = status
        .members
        .iter()
        .map(|m| {
            let marker = if m.done { "\u{2713}" } else { "\u{00B7}" };
            format!("{} {}", marker, m.name)
        })
        .collect();
    println!(
        "{}: {}/{} [{}]",
        set_name,
        status.completed,
        status.total,
        members.join(", ")
    );

    EXIT_SUCCESS
}
```

- [ ] **Step 5: Rewrite cmd_set_complete output**

In `cmd_set_complete`, replace the success println block (lines 342-345) and render output (lines 371-373) with:

```rust
            let status = machine.set_status(set_name);
The full rewrite of the success path inside `cmd_set_complete` (from `Ok(())` through the end of the render block) should be:

```rust
            // Trigger on_event renders for set_member_complete
            let mut render_count = 0usize;
            if !config.renders.is_empty() {
                let registry_path = super::commands::registry_path_from_config(&config);
                if let Ok(engine) = RenderEngine::new(&config, &config_path) {
                    let mut engine = engine.with_registry(registry_path);
                    if let Some(ref name) = targeting.ledger_name {
                        engine = engine.with_active_ledger_name(name.clone());
                    }
                    let render_dir = resolve_data_dir(&config.paths.render_dir);
                    let ledger_seq = machine
                        .ledger()
                        .entries()
                        .last()
                        .map(|e| e.seq)
                        .unwrap_or(0);
                    match engine.render_triggered(
                        "on_event",
                        Some("set_member_complete"),
                        machine.ledger(),
                        &render_dir,
                        &mut manifest,
                        ledger_seq,
                    ) {
                        Ok(rendered) => {
                            render_count = rendered.len();
                            if !rendered.is_empty() {
                                let _ = save_manifest(&mut manifest, &data_dir);
                            }
                        }
                        Err(e) => {
                            eprintln!("error: render: {}", e);
                        }
                    }
                }
            }

            let status = machine.set_status(set_name);
            if render_count > 0 {
                println!(
                    "set {}: {} done ({}/{}, {} rendered)",
                    set_name, member, status.completed, status.total, render_count
                );
            } else {
                println!(
                    "set {}: {} done ({}/{})",
                    set_name, member, status.completed, status.total
                );
            }
```

- [ ] **Step 6: Update existing integration tests for status**

In `tests/integration_tests.rs`, update these tests:

`test_status_shows_current_state` — change assertions:
```rust
.stdout(predicate::str::contains("state:"))
.stdout(predicate::str::contains("idle"))
```

`test_transition_advances_state` — change assertion on status:
```rust
.stdout(predicate::str::contains("working"))
```

`test_event_recording` — change assertion:
```rust
.stdout(predicate::str::contains("1/2"))
```

`test_full_workflow` — change assertion:
```rust
.stdout(predicate::str::contains("done"))
```

`test_status_shows_set_progress` — change assertion:
```rust
.stdout(predicate::str::contains("1/2"))
```

`test_event_only_status_metadata` — change assertions:
```rust
.stdout(predicate::str::contains("event-only"))
```
Remove the assertion for `"Events:"` — the new format uses `"event-only: N events"`.

- [ ] **Step 7: Run tests**

Run: `cargo test test_status -- --nocapture`
Expected: All status-related tests PASS

- [ ] **Step 8: Commit**

```bash
git add src/cli/status.rs tests/integration_tests.rs
git commit -m "feat: rewrite status/set output to terse format"
```

---

### Task 3: Rewrite transition and gate-check output

**Files:**
- Modify: `src/cli/transition.rs:28-148` (cmd_transition), `156-258` (cmd_gate_check), `265-413` (cmd_event)
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Write the failing test**

Add to `tests/integration_tests.rs`:

```rust
#[test]
fn test_transition_terse_output() {
    let dir = setup_initialized_dir();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    // New format: "idle → working" (with arrow, no "Transition:" prefix, no "Recorded.")
    assert!(stdout.contains("→"), "expected arrow in: {}", stdout);
    assert!(stdout.contains("idle"), "expected old state in: {}", stdout);
    assert!(stdout.contains("working"), "expected new state in: {}", stdout);
    assert!(!stdout.contains("Transition:"), "should not have old prefix in: {}", stdout);
    assert!(!stdout.contains("Recorded"), "should not have old suffix in: {}", stdout);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_transition_terse_output -- --nocapture`
Expected: FAIL — current output is `"Transition: idle -> working. Recorded."`

- [ ] **Step 3: Rewrite cmd_transition success and error output**

In `src/cli/transition.rs`, replace the success block (lines 83-87):

```rust
            // Count rendered files instead of printing each
            let mut render_count = 0usize;
```

Replace the render output loop (lines 112-115):
```rust
                        Ok(rendered) => {
                            render_count = rendered.len();
                            if !rendered.is_empty() {
                                let _ = save_manifest(&mut manifest, &data_dir);
                            }
                        }
```

After the render block, print the single line:
```rust
            if render_count > 0 {
                println!(
                    "{} \u{2192} {} ({} rendered)",
                    from_state,
                    machine.current_state(),
                    render_count
                );
            } else {
                println!("{} \u{2192} {}", from_state, machine.current_state());
            }
```

Replace the GateBlocked error (lines 129-135):
```rust
        Err(crate::state::machine::StateError::GateBlocked { gate_type, reason }) => {
            eprintln!("\u{2717} {}: {}", gate_type, reason);
            EXIT_GATE_FAILED
        }
```

Note: We lose the intent here because `StateError::GateBlocked` doesn't carry it. This is acceptable — the agent can run `gate-check` for full details. To add intent to the error, we'd need to modify `StateError`, which is a deeper change. For now, the terse rejection line is sufficient.

Replace the NoTransition error (lines 137-143):
```rust
        Err(crate::state::machine::StateError::NoTransition { command, state }) => {
            eprintln!("error: no transition '{}' from state '{}'", command, state);
            EXIT_USAGE_ERROR
        }
```

Replace the catch-all error (lines 144-147):
```rust
        Err(e) => {
            eprintln!("error: transition failed: {}", e);
            EXIT_INTEGRITY_ERROR
        }
```

- [ ] **Step 4: Rewrite cmd_gate_check output**

Replace the output section of `cmd_gate_check` (lines 240-257):

```rust
    println!("gate-check: {}", transition_name);
    for result in &results {
        let marker = if result.passed { "\u{2713}" } else { "\u{2717}" };
        if result.passed {
            println!("  {} {}", marker, result.description);
        } else {
            let intent = result
                .intent
                .as_deref()
                .unwrap_or("gate condition must be met");
            println!(
                "  {} {}: {} \u{2014} {}",
                marker,
                result.gate_type,
                result.reason.as_deref().unwrap_or("failed"),
                intent
            );
        }
    }

    if all_passed {
        println!("result: ready");
    } else {
        println!("result: blocked");
    }
    EXIT_SUCCESS
```

Also replace the "no gates" case (lines 204-209):
```rust
    if transition.gates.is_empty() {
        println!("gate-check: {}", transition_name);
        println!("result: ready (no gates)");
        return EXIT_SUCCESS;
    }
```

Also replace the "no transition" error (lines 196-202):
```rust
        None => {
            eprintln!(
                "error: no transition '{}' from state '{}'",
                transition_name, current_state
            );
            return EXIT_USAGE_ERROR;
        }
```

- [ ] **Step 5: Rewrite cmd_event output**

In `cmd_event`, replace the success output (line 366) and render loop (lines 391-394):

```rust
        Ok(()) => {
            // ... manifest tracking unchanged ...

            let mut render_count = 0usize;

            // Trigger on_event renders (existing render block, but collect count)
            if !config.renders.is_empty() {
                // ... existing render setup ...
                    match engine.render_triggered(/* ... */) {
                        Ok(rendered) => {
                            render_count = rendered.len();
                            if !rendered.is_empty() {
                                let _ = save_manifest(&mut manifest, &data_dir);
                            }
                        }
                        Err(e) => {
                            eprintln!("error: render: {}", e);
                        }
                    }
                // ...
            }

            if render_count > 0 {
                println!("recorded: {} ({} rendered)", event_type, render_count);
            } else {
                println!("recorded: {}", event_type);
            }

            EXIT_SUCCESS
        }
```

Also update error messages in cmd_event to use `error:` prefix:
- Line 303: `eprintln!("error: invalid field '{}': expected key=value", f);`
- Lines 313-316: `eprintln!("error: missing field '{}' for event '{}'", field_def.name, event_type);`
- Lines 328-330: `eprintln!("error: field '{}' value '{}' doesn't match pattern '{}'", ...);`
- Lines 340-342: `eprintln!("error: field '{}' value '{}' not in allowed values {:?}", ...);`
- Line 409: `eprintln!("error: cannot record event: {}", e);`

- [ ] **Step 6: Update existing integration tests**

In `tests/integration_tests.rs`:

`test_transition_with_template_args` — change:
```rust
// Old: .stdout(predicate::str::contains("Transition: idle -> working"))
.stdout(predicate::str::contains("\u{2192}"))
```

`test_gate_check_dry_run` — change to check for new markers:
```rust
.stdout(predicate::str::contains("\u{2717}"))
```

`test_gate_check_dry_run_with_template_args` — change:
```rust
.stdout(predicate::str::contains("\u{2713}"))
```

- [ ] **Step 7: Run tests**

Run: `cargo test -- --nocapture 2>&1 | head -50`
Expected: All transition and gate-check tests PASS

- [ ] **Step 8: Commit**

```bash
git add src/cli/transition.rs tests/integration_tests.rs
git commit -m "feat: rewrite transition/gate-check/event output to terse format"
```

---

### Task 4: Rewrite init, validate, and reset output

**Files:**
- Modify: `src/cli/init.rs:26-60` (cmd_validate), `67-160` (cmd_init), `167-248` (cmd_reset)
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Rewrite cmd_init output**

In `src/cli/init.rs`, replace line 158:
```rust
    println!("initialized. good luck.");
```

Replace the "already initialized" error (lines 87-91):
```rust
        eprintln!("error: already initialized ({}). run reset first.", lp.display());
```

Replace other error messages to use `error:` prefix throughout.

- [ ] **Step 2: Rewrite cmd_validate output**

Replace lines 43-59:
```rust
    for w in &warnings {
        eprintln!("warning: {}", w);
    }

    if errors.is_empty() {
        println!("valid.");
        EXIT_SUCCESS
    } else {
        for e in &errors {
            eprintln!("error: {}", e);
        }
        EXIT_CONFIG_ERROR
    }
```

Replace the parse error (lines 33-36):
```rust
        Err(e) => {
            eprintln!("error: {}", e);
            return EXIT_CONFIG_ERROR;
        }
```

- [ ] **Step 3: Rewrite cmd_reset output**

Replace lines 169-170:
```rust
        eprintln!("error: reset requires --confirm");
```

Replace lines 243-244 (token prompt):
```rust
            println!("reset requires --token {}", token_str);
```

Replace line 230:
```rust
                println!("reset. prior run archived.");
```

Replace lines 235-239 (token mismatch):
```rust
            eprintln!("error: token mismatch. expected '{}', got '{}'", token_str, provided_token);
```

- [ ] **Step 4: Update integration tests**

`test_validate_clean_config` — change:
```rust
.stdout(predicate::str::contains("valid."))
```

- [ ] **Step 5: Run tests**

Run: `cargo test test_validate -- --nocapture && cargo test test_init -- --nocapture`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/cli/init.rs tests/integration_tests.rs
git commit -m "feat: rewrite init/validate/reset output to terse format"
```

---

### Task 5: Rewrite ledger subcommand output

**Files:**
- Modify: `src/cli/ledger.rs:30-186` (cmd_ledger_create), `193-231` (cmd_ledger_list), `238-267` (cmd_ledger_remove), `274-333` (cmd_ledger_verify), `340-389` (cmd_ledger_checkpoint), `396-460` (cmd_ledger_import)
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Rewrite all ledger subcommand output**

`cmd_ledger_create` — replace line 180-184:
```rust
    println!("created: {}", ledger_name);
```

`cmd_ledger_list` — replace lines 213-228:
```rust
    if entries.is_empty() {
        println!("(no ledgers)");
        return EXIT_SUCCESS;
    }

    for entry in entries {
        let mode_str = match entry.mode {
            LedgerMode::Stateful => "stateful",
            LedgerMode::EventOnly => "event-only",
        };
        println!("{} ({}) {}", entry.name, mode_str, entry.path);
    }
```

`cmd_ledger_remove` — replace lines 262-266:
```rust
    println!("removed: {} (file kept)", name);
```

`cmd_ledger_verify` — replace lines 319-332:
```rust
    match ledger.verify() {
        Ok(()) => {
            println!("chain valid ({} entries)", ledger.len());
            EXIT_SUCCESS
        }
        Err(e) => {
            eprintln!("error: chain invalid: {} \u{2014} tampering detected", e);
            EXIT_INTEGRITY_ERROR
        }
    }
```

`cmd_ledger_checkpoint` — replace lines 377-382:
```rust
        Ok(cp) => {
            println!("checkpoint: seq {} scope={}", cp.seq, scope);
            EXIT_SUCCESS
        }
```

`cmd_ledger_import` — replace lines 454-458:
```rust
    println!("imported: {}", name);
```

Also update all error messages to use `error:` prefix throughout the file.

- [ ] **Step 2: Update integration tests**

`test_ledger_list_empty` — change:
```rust
.stdout(predicate::str::contains("default"))
```
(This still works — the default ledger is listed by name.)

`test_ledger_create_and_list` — change:
```rust
// Create
.stdout(predicate::str::contains("created: audit"))
// List
.stdout(predicate::str::contains("audit"))
.stdout(predicate::str::contains("event-only"))
```

`test_ledger_create_and_remove` — change:
```rust
.stdout(predicate::str::contains("removed:"))
```

`test_ledger_verify_by_name` — change:
```rust
.stdout(predicate::str::contains("chain valid"))
```

`test_ledger_checkpoint` — change:
```rust
.stdout(predicate::str::contains("checkpoint: seq"))
```

`test_ledger_import_from_stdin` — change:
```rust
.stdout(predicate::str::contains("imported:"))
```

For the verify-after-import test, change:
```rust
.stdout(predicate::str::contains("3 entries"))
```
(This should still match `"chain valid (3 entries)"`)

- [ ] **Step 3: Run tests**

Run: `cargo test test_ledger -- --nocapture`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/cli/ledger.rs tests/integration_tests.rs
git commit -m "feat: rewrite ledger subcommand output to terse format"
```

---

### Task 6: Rewrite manifest subcommand output

**Files:**
- Modify: `src/cli/manifest_cmd.rs:24-71` (cmd_manifest_verify), `78-111` (cmd_manifest_list), `118-164` (cmd_manifest_restore)
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Rewrite manifest output**

`cmd_manifest_verify` — replace lines 46-70:
```rust
    if result.clean {
        println!("manifest clean ({} tracked)", manifest.entries.len());
        EXIT_SUCCESS
    } else {
        eprintln!("manifest: {} modified", result.mismatches.len());
        for m in &result.mismatches {
            let actual_str = match &m.actual {
                Some(h) => format!("got {}", &h[..12]),
                None => "missing".to_string(),
            };
            eprintln!(
                "  {} \u{2014} expected {}, {}",
                m.path,
                &m.expected[..12],
                actual_str
            );
        }
        EXIT_INTEGRITY_ERROR
    }
```

`cmd_manifest_list` — replace lines 97-108:
```rust
    let mut paths: Vec<_> = manifest.entries.keys().collect();
    paths.sort();
    for path in paths {
        let entry = &manifest.entries[path];
        println!("{} {} ({})", &entry.sha256[..12], path, entry.last_operation);
    }
```

No header line.

`cmd_manifest_restore` — replace lines 138-163:
```rust
        crate::manifest::tracker::RestoreAction::ReRender {
            path: p,
            ledger_seq,
        } => {
            println!("restore: re-render {} (last tracked seq {})", p, ledger_seq);
            EXIT_SUCCESS
        }
        crate::manifest::tracker::RestoreAction::GitCheckout { path: p } => {
            println!("restore: git checkout -- {}", p);
            EXIT_SUCCESS
        }
        crate::manifest::tracker::RestoreAction::NotTracked { path: p } => {
            eprintln!("error: '{}' not tracked", p);
            EXIT_USAGE_ERROR
        }
```

- [ ] **Step 2: Update integration tests**

`test_manifest_list` �� change:
```rust
.stdout(predicate::str::contains("ledger.jsonl"))
```
(Still works — file name still appears in output.)

- [ ] **Step 3: Run tests**

Run: `cargo test test_manifest -- --nocapture`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add src/cli/manifest_cmd.rs tests/integration_tests.rs
git commit -m "feat: rewrite manifest subcommand output to terse format"
```

---

### Task 7: Rewrite render and log verify output

**Files:**
- Modify: `src/cli/render.rs:20-90` (cmd_render)
- Modify: `src/cli/log.rs:48-77` (cmd_log_verify)
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Rewrite cmd_render output**

Replace lines 31-33:
```rust
        println!("(no renders configured)");
```

Replace lines 71-83:
```rust
        Ok(rendered) => {
            if let Err((code, msg)) = save_manifest(&mut manifest, &data_dir) {
                eprintln!("error: {}", msg);
                return code;
            }
            println!("rendered: {} file(s)", rendered.len());
            EXIT_SUCCESS
        }
```

- [ ] **Step 2: Rewrite cmd_log_verify output**

Replace lines 66-76:
```rust
    match ledger.verify() {
        Ok(()) => {
            println!("chain valid ({} events)", ledger.len());
            EXIT_SUCCESS
        }
        Err(e) => {
            eprintln!("error: chain invalid: {} \u{2014} tampering detected", e);
            EXIT_INTEGRITY_ERROR
        }
    }
```

- [ ] **Step 3: Update integration tests**

`test_render_reports_files` — change:
```rust
// Old: .stdout(predicate::str::contains("Rendered: STATUS.md"))
.stdout(predicate::str::contains("rendered:"))
```

`test_render_tracked_in_manifest` — the manifest tracking test may check for "STATUS.md" in manifest list output. This still works since `manifest list` still shows file names.

`test_ledger_path_targeting_log_verify` — change:
```rust
// Old: .stdout(predicate::str::contains("OK:"))
.stdout(predicate::str::contains("chain valid"))
```

- [ ] **Step 4: Run tests**

Run: `cargo test test_render -- --nocapture && cargo test test_log -- --nocapture`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/cli/render.rs src/cli/log.rs tests/integration_tests.rs
git commit -m "feat: rewrite render and log verify output to terse format"
```

---

### Task 8: Update CLAUDE.md documentation

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update the output format documentation**

The CLAUDE.md doesn't currently document output format, but the lookup tables reference error messages and CLI behavior. No changes needed to lookup tables — the anchors and function signatures are unchanged.

However, if any `// ## Index` headers in modified files have changed, update them. Check:
- `src/cli/status.rs` — index unchanged (functions still exist with same names)
- `src/cli/transition.rs` — index unchanged
- `src/cli/init.rs` — index unchanged
- `src/cli/ledger.rs` — index unchanged
- `src/cli/manifest_cmd.rs` — index unchanged
- `src/cli/render.rs` — index unchanged
- `src/gates/evaluator.rs` — add `default_intent` to index

Add to `src/gates/evaluator.rs` index comment:
```
// - default_intent              default_intent()    — fallback intent string for gate types
```

- [ ] **Step 2: Commit**

```bash
git add src/gates/evaluator.rs CLAUDE.md
git commit -m "docs: update index headers for terse output changes"
```

---

### Task 9: Full test suite verification

**Files:**
- No modifications — verification only

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All 251+ tests pass

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

- [ ] **Step 3: Run fmt check**

Run: `cargo fmt -- --check`
Expected: No formatting issues

- [ ] **Step 4: Fix any remaining test failures**

If any tests still reference old output strings (e.g., "Suspicious", "The way is open", "I don't make the rules"), grep for them and update:

Run: `grep -rn "Suspicious\|The way is open\|I don't make the rules\|The protocol is clear\|The ledger made manifest\|Good luck\|Rendered:" tests/`

Update any remaining assertions to match new output format.

- [ ] **Step 5: Final commit if needed**

```bash
git add -A
git commit -m "fix: update remaining test assertions for terse output"
```
