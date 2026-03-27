# Fix Issues #6 and #7 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix template variable interpolation in gate commands (#6) and render pipeline fallback to default ledger (#7).

**Architecture:** Issue #6 — parse transition CLI args as `key=value` pairs and merge them into `state_params` so `build_template_vars()` can resolve `{{var}}` placeholders in gate commands. Issue #7 — make `resolve_render_ledger()` fall back to the default ledger (return `Ok(None)`) when the named ledger can't be resolved, and wire up `with_registry()` in all `RenderEngine` creation sites.

**Tech Stack:** Rust, clap, Tera, TOML config

---

### Task 1: Issue #6 — Parse transition args into state_params

**Files:**
- Modify: `src/state/machine.rs:96-134` (transition method)
- Test: `tests/gate_tests.rs`

The `transition()` method currently ignores `_args: &[String]`. Fix: parse args as `key=value` pairs and merge them into `state_params` before gate evaluation.

- [ ] **Step 1: Write the failing test**

In `tests/gate_tests.rs`, first add the new imports at the top of the file alongside the existing imports:

```rust
use sahjhan::config::{StateParam, TransitionConfig};
use sahjhan::state::machine::StateMachine;
```

Then add these tests at the end of the file:

```rust
// ---------------------------------------------------------------------------
// transition args as template variables (Issue #6)
// ---------------------------------------------------------------------------

#[test]
fn test_transition_args_interpolated_in_gate_command() {
    let dir = tempdir().unwrap();

    // Build a config with a command_succeeds gate that uses {{item_id}}.
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();

    // Add a transition from idle->working with a gate that checks {{item_id}}.
    config.transitions = vec![
        TransitionConfig {
            from: "idle".to_string(),
            to: "working".to_string(),
            command: "begin".to_string(),
            gates: vec![make_gate(
                "command_succeeds",
                vec![(
                    "cmd",
                    toml::Value::String("test {{item_id}} = 'BH-019'".to_string()),
                )],
            )],
        },
    ];

    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut machine = StateMachine::new(&config, ledger);

    // Pass item_id=BH-019 as a transition arg.
    let result = machine.transition("begin", &["item_id=BH-019".to_string()]);
    assert!(
        result.is_ok(),
        "transition should succeed with interpolated arg: {:?}",
        result.err()
    );
}

#[test]
fn test_transition_args_override_state_params() {
    let dir = tempdir().unwrap();

    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();

    // Add a state param that maps "targets" to set "check"
    config.states.get_mut("working").unwrap().params = Some(vec![
        StateParam {
            name: "targets".to_string(),
            set: "check".to_string(),
        },
    ]);

    config.transitions = vec![
        TransitionConfig {
            from: "idle".to_string(),
            to: "working".to_string(),
            command: "begin".to_string(),
            gates: vec![make_gate(
                "command_succeeds",
                vec![(
                    "cmd",
                    toml::Value::String("test {{targets}} = 'override_val'".to_string()),
                )],
            )],
        },
    ];

    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let mut machine = StateMachine::new(&config, ledger);

    // CLI arg should override the state_param value
    let result = machine.transition("begin", &["targets=override_val".to_string()]);
    assert!(
        result.is_ok(),
        "CLI arg should override state param: {:?}",
        result.err()
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_transition_args -- --nocapture 2>&1 | head -30`
Expected: FAIL — the gate command sees literal `{{item_id}}` instead of `BH-019`.

- [ ] **Step 3: Implement arg parsing in StateMachine::transition()**

In `src/state/machine.rs`, modify the `transition` method to parse args and merge into state_params:

```rust
pub fn transition(&mut self, command: &str, args: &[String]) -> Result<(), StateError> {
    // Find a matching transition from the current state.
    let transition = self
        .config
        .transitions
        .iter()
        .find(|t| t.command == command && t.from == self.current_state)
        .ok_or_else(|| StateError::NoTransition {
            command: command.to_string(),
            state: self.current_state.clone(),
        })?
        .clone();

    // Build state_params from the target state's param definitions.
    let mut state_params = self.build_state_params(&transition.to);

    // Parse CLI args as key=value pairs and merge into state_params.
    // CLI args override state params from config.
    for arg in args {
        if let Some((key, value)) = arg.split_once('=') {
            state_params.insert(key.to_string(), value.to_string());
        }
    }

    // Evaluate gates.
    for gate in &transition.gates {
        self.evaluate_gate(gate, &state_params)?;
    }

    // Reload ledger from disk in case gate commands appended entries.
    self.ledger.reload().map_err(StateError::Ledger)?;

    // Record the transition event.
    let mut fields = BTreeMap::new();
    fields.insert("from".to_string(), self.current_state.clone());
    fields.insert("to".to_string(), transition.to.clone());
    fields.insert("command".to_string(), command.to_string());

    self.ledger
        .append("state_transition", fields)
        .map_err(StateError::Ledger)?;

    self.current_state = transition.to.clone();
    Ok(())
}
```

The key change: rename `_args` to `args`, parse each arg as `key=value`, and insert into `state_params` after building from config (so CLI args override config-derived params).

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test test_transition_args -- --nocapture 2>&1 | head -30`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src/state/machine.rs tests/gate_tests.rs
git commit -m "fix: interpolate transition CLI args as template variables in gate commands (#6)"
```

### Task 2: Issue #6 — Wire args through cmd_gate_check for dry-run support

**Files:**
- Modify: `src/cli/transition.rs:150-234` (cmd_gate_check)
- Modify: `src/main.rs:208-215` (GateAction::Check)
- Test: `tests/integration_tests.rs`

The `gate check` command should also accept args so users can dry-run gate evaluation with template vars.

- [ ] **Step 1: Write the failing integration test**

Add to `tests/integration_tests.rs`:

```rust
#[test]
fn test_gate_check_with_args() {
    let dir = setup_initialized_dir();

    // Overwrite transitions.toml with a gate that uses {{item_id}}
    std::fs::write(
        dir.path().join("enforcement/transitions.toml"),
        r#"
[[transitions]]
from = "idle"
to = "working"
command = "begin"
gates = [
    { type = "command_succeeds", cmd = "test {{item_id}} = 'BH-019'" },
]

[[transitions]]
from = "working"
to = "done"
command = "complete"
gates = [
    { type = "set_covered", set = "check", event = "set_member_complete", field = "member" },
]
"#,
    )
    .unwrap();

    // Gate check with args should show the gate passing
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "gate",
            "check",
            "begin",
            "--",
            "item_id=BH-019",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("✓"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_gate_check_with_args -- --nocapture 2>&1 | head -30`
Expected: FAIL — clap doesn't accept trailing args for `gate check`.

- [ ] **Step 3: Add args to GateAction::Check in main.rs**

In `src/main.rs`, modify the `GateAction::Check` variant:

```rust
#[derive(Subcommand)]
enum GateAction {
    /// Dry-run: show which gates pass/fail
    Check {
        /// Transition name
        transition: String,

        /// Additional arguments (key=value pairs for template variables)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
    },
}
```

Update the match arm:

```rust
Commands::Gate { action } => match action {
    GateAction::Check { transition, args } => {
        transition::cmd_gate_check(&cli.config_dir, &transition, &args, &targeting)
    }
},
```

- [ ] **Step 4: Update cmd_gate_check signature and logic**

In `src/cli/transition.rs`, update `cmd_gate_check` to accept and parse args:

```rust
pub fn cmd_gate_check(
    config_dir: &str,
    transition_name: &str,
    args: &[String],
    targeting: &LedgerTargeting,
) -> i32 {
```

Then in the body, after `build_state_params`, add:

```rust
    let mut state_params = build_state_params(&config, &transition.to);

    // Parse CLI args as key=value pairs and merge into state_params.
    for arg in args {
        if let Some((key, value)) = arg.split_once('=') {
            state_params.insert(key.to_string(), value.to_string());
        }
    }
```

(Replace the existing `let state_params = build_state_params(...)` line with the `let mut` version above.)

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test test_gate_check_with_args -- --nocapture 2>&1 | head -30`
Expected: PASS

- [ ] **Step 6: Commit**

```bash
git add src/main.rs src/cli/transition.rs tests/integration_tests.rs
git commit -m "feat: support key=value args in gate check for template variable dry-run (#6)"
```

### Task 3: Issue #7 — Wire registry path into RenderEngine creation

**Files:**
- Modify: `src/cli/transition.rs:89-120,344-375` (cmd_transition and cmd_event render blocks)
- Modify: `src/cli/render.rs:52-58` (cmd_render engine creation)
- Test: `tests/integration_tests.rs`

The `RenderEngine` has a `.with_registry()` method but nobody calls it. All three render creation sites (cmd_transition, cmd_event, cmd_render) need to pass the registry path.

- [ ] **Step 1: Write the failing test**

Add to `tests/integration_tests.rs`:

```rust
#[test]
fn test_render_with_named_ledger_falls_back_to_default() {
    let dir = setup_initialized_dir();

    // Overwrite renders.toml to reference a named ledger that doesn't exist
    std::fs::write(
        dir.path().join("enforcement/renders.toml"),
        r#"
[[renders]]
target = "STATUS.md"
template = "templates/status.md.tera"
trigger = "on_transition"
ledger = "run"
"#,
    )
    .unwrap();

    // Render should succeed by falling back to the default ledger
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "render"])
        .current_dir(dir.path())
        .assert()
        .success();

    // STATUS.md should exist and have content from the default ledger
    assert!(dir.path().join("output/STATUS.md").exists());
    let content = std::fs::read_to_string(dir.path().join("output/STATUS.md")).unwrap();
    assert!(content.contains("idle") || content.contains("Idle"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_render_with_named_ledger_falls_back_to_default -- --nocapture 2>&1 | head -30`
Expected: FAIL — currently errors with "requires ledger 'run' but no registry path was configured"

- [ ] **Step 3: Wire registry path into cmd_render**

In `src/cli/render.rs`, change the engine creation in `cmd_render` to:

```rust
    let registry_path = super::commands::registry_path_from_config(&config);
    let engine = match RenderEngine::new(&config, &config_path) {
        Ok(e) => e.with_registry(registry_path),
        Err(e) => {
            eprintln!("Cannot create render engine: {}", e);
            return EXIT_CONFIG_ERROR;
        }
    };
```

Do the same in `cmd_render_dump_context`:

```rust
    let registry_path = super::commands::registry_path_from_config(&config);
    let engine = match RenderEngine::new(&config, &config_path) {
        Ok(e) => e.with_registry(registry_path),
        Err(e) => {
            eprintln!("Cannot create render engine: {}", e);
            return EXIT_CONFIG_ERROR;
        }
    };
```

- [ ] **Step 4: Wire registry path into cmd_transition and cmd_event**

In `src/cli/transition.rs`, update both render blocks. In `cmd_transition` (around line 91):

```rust
            if !config.renders.is_empty() {
                let registry_path = super::commands::registry_path_from_config(&config);
                if let Ok(engine) = RenderEngine::new(&config, &config_path) {
                    let engine = engine.with_registry(registry_path);
```

In `cmd_event` (around line 346):

```rust
            if !config.renders.is_empty() {
                let registry_path = super::commands::registry_path_from_config(&config);
                if let Ok(engine) = RenderEngine::new(&config, &config_path) {
                    let engine = engine.with_registry(registry_path);
```

- [ ] **Step 5: Run test — it will still fail because resolve_render_ledger errors**

Run: `cargo test test_render_with_named_ledger_falls_back_to_default -- --nocapture 2>&1 | head -30`
Expected: Still FAIL — now the error will be "ledger 'run' not found in the registry" (or "cannot load ledger registry" if ledgers.toml doesn't exist).

- [ ] **Step 6: Commit the wiring**

```bash
git add src/cli/render.rs src/cli/transition.rs
git commit -m "refactor: wire registry path into all RenderEngine creation sites (#7)"
```

### Task 4: Issue #7 — Make resolve_render_ledger fall back to default ledger

**Files:**
- Modify: `src/render/engine.rs:85-132` (resolve_render_ledger)
- Test: `tests/integration_tests.rs` (the test from Task 3 should now pass)

Change `resolve_render_ledger` to return `Ok(None)` with a warning when the named ledger can't be resolved, instead of returning `Err`.

- [ ] **Step 1: Modify resolve_render_ledger to fall back gracefully**

Replace the `resolve_render_ledger` method in `src/render/engine.rs` with:

```rust
    /// Resolve the ledger for a render config.
    ///
    /// If the render has a `ledger` field, attempt to load that ledger from the
    /// registry. If the registry or named ledger doesn't exist, fall back to
    /// the default ledger (return `Ok(None)`) with a warning on stderr.
    fn resolve_render_ledger(
        &self,
        render_cfg: &crate::config::RenderConfig,
    ) -> Result<Option<Ledger>, String> {
        let ledger_name = match &render_cfg.ledger {
            Some(n) => n,
            None => return Ok(None),
        };

        let reg_path = match self.registry_path.as_ref() {
            Some(p) => p,
            None => {
                eprintln!(
                    "  Render '{}': ledger '{}' requested but no registry configured; using default ledger",
                    render_cfg.target, ledger_name
                );
                return Ok(None);
            }
        };

        let registry = match LedgerRegistry::new(reg_path) {
            Ok(r) => r,
            Err(_) => {
                eprintln!(
                    "  Render '{}': cannot load registry; using default ledger for '{}'",
                    render_cfg.target, ledger_name
                );
                return Ok(None);
            }
        };

        let entry = match registry.resolve(Some(ledger_name)) {
            Ok(e) => e,
            Err(_) => {
                eprintln!(
                    "  Render '{}': ledger '{}' not found in registry; using default ledger",
                    render_cfg.target, ledger_name
                );
                return Ok(None);
            }
        };

        // Resolve relative paths against the registry file's parent directory.
        let ledger_path = {
            let p = std::path::PathBuf::from(&entry.path);
            if p.is_absolute() {
                p
            } else {
                reg_path
                    .parent()
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .join(p)
            }
        };

        let ledger = match Ledger::open(&ledger_path) {
            Ok(l) => l,
            Err(_) => {
                eprintln!(
                    "  Render '{}': cannot open ledger '{}' at {}; using default ledger",
                    render_cfg.target, ledger_name, ledger_path.display()
                );
                return Ok(None);
            }
        };

        Ok(Some(ledger))
    }
```

- [ ] **Step 2: Run tests to verify the fallback test passes**

Run: `cargo test test_render_with_named_ledger_falls_back_to_default -- --nocapture 2>&1 | head -30`
Expected: PASS

- [ ] **Step 3: Run the full test suite**

Run: `cargo test 2>&1 | tail -20`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/render/engine.rs tests/integration_tests.rs
git commit -m "fix: render pipeline falls back to default ledger when named ledger missing (#7)"
```

### Task 5: Integration test — transition with args through CLI

**Files:**
- Test: `tests/integration_tests.rs`

Add an end-to-end CLI test that verifies transition args are interpolated through the full pipeline.

- [ ] **Step 1: Write the integration test**

Add to `tests/integration_tests.rs`:

```rust
#[test]
fn test_transition_with_template_args() {
    let dir = setup_initialized_dir();

    // Overwrite transitions.toml with a gate using {{item_id}}
    std::fs::write(
        dir.path().join("enforcement/transitions.toml"),
        r#"
[[transitions]]
from = "idle"
to = "working"
command = "begin"
gates = [
    { type = "command_succeeds", cmd = "test {{item_id}} = 'BH-019'" },
]

[[transitions]]
from = "working"
to = "done"
command = "complete"
gates = [
    { type = "set_covered", set = "check", event = "set_member_complete", field = "member" },
]
"#,
    )
    .unwrap();

    // Transition with item_id arg should succeed
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "transition",
            "begin",
            "item_id=BH-019",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Transition: idle -> working"));
}

#[test]
fn test_transition_without_required_arg_fails() {
    let dir = setup_initialized_dir();

    // Same gate requiring {{item_id}}
    std::fs::write(
        dir.path().join("enforcement/transitions.toml"),
        r#"
[[transitions]]
from = "idle"
to = "working"
command = "begin"
gates = [
    { type = "command_succeeds", cmd = "test {{item_id}} = 'BH-019'" },
]

[[transitions]]
from = "working"
to = "done"
command = "complete"
gates = [
    { type = "set_covered", set = "check", event = "set_member_complete", field = "member" },
]
"#,
    )
    .unwrap();

    // Transition without the arg — gate should fail because {{item_id}} is literal
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "transition",
            "begin",
        ])
        .current_dir(dir.path())
        .assert()
        .code(1); // EXIT_GATE_FAILED
}
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test test_transition_with_template -- --nocapture 2>&1 | head -40`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add tests/integration_tests.rs
git commit -m "test: add integration tests for transition template args and render fallback (#6, #7)"
```

### Task 6: Run full test suite and verify

- [ ] **Step 1: Run the complete test suite**

Run: `cargo test 2>&1`
Expected: All tests pass, no regressions.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy 2>&1`
Expected: No new warnings.
