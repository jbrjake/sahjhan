# Gate Attestation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** When command/snapshot gates pass during a transition, emit `gate_attestation` events with content-addressed evidence (stdout hash, exit code, timing) so the ledger records machine-verified proof alongside agent testimony.

**Architecture:** Add `GateAttestation` to `GateResult`, modify command/snapshot gate evaluators to populate it, change `StateMachine::transition()` to return `TransitionOutcome` with attestation data, and emit `gate_attestation` events after each `state_transition`. The `attest` opt-out flag is read from `gate.params` (via existing serde flatten).

**Tech Stack:** Rust, sha2 (already a dependency), chrono (already a dependency)

**Spec:** `docs/superpowers/specs/2026-03-30-gate-attestation-design.md`

---

### Task 1: Add GateAttestation struct and attestation field to GateResult

**Files:**
- Modify: `src/gates/evaluator.rs:43-58`

- [ ] **Step 1: Write the failing test**

Add to `tests/gate_tests.rs`:

```rust
// ---------------------------------------------------------------------------
// attestation — command gates produce attestation data
// ---------------------------------------------------------------------------

#[test]
fn test_command_succeeds_produces_attestation() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate(
        "command_succeeds",
        vec![("cmd", toml::Value::String("echo hello".to_string()))],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(result.passed);
    let att = result.attestation.expect("command_succeeds should produce attestation");
    assert_eq!(att.gate_type, "command_succeeds");
    assert_eq!(att.exit_code, 0);
    assert!(!att.stdout_hash.is_empty());
    assert!(att.wall_time_ms < 10_000); // should complete quickly
    assert!(!att.executed_at.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_command_succeeds_produces_attestation -- --nocapture 2>&1 | head -30`
Expected: FAIL — `attestation` field doesn't exist on `GateResult`

- [ ] **Step 3: Add GateAttestation struct and attestation field to GateResult**

In `src/gates/evaluator.rs`, add after the existing `use` statements:

```rust
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::time::Instant;
```

Add after the `GateContext` struct (before `GateResult`):

```rust
/// Evidence produced by a gate that executes an external command.
///
/// Populated by command_succeeds, command_output, and snapshot_compare gates.
/// Carried on GateResult and used by StateMachine::transition() to emit
/// gate_attestation events.
pub struct GateAttestation {
    /// Gate type that produced this attestation (e.g. "command_succeeds").
    pub gate_type: String,
    /// Resolved command string that was executed.
    pub command: String,
    /// Numeric exit code of the command.
    pub exit_code: i32,
    /// SHA-256 hex digest of raw stdout.
    pub stdout_hash: String,
    /// Execution wall time in milliseconds.
    pub wall_time_ms: u64,
    /// RFC3339 timestamp of when execution started.
    pub executed_at: String,
}
```

Add one field to `GateResult`:

```rust
    /// Machine-attested evidence from command execution, if applicable.
    pub attestation: Option<GateAttestation>,
```

- [ ] **Step 4: Fix all compilation errors from the new field**

Every place that constructs a `GateResult` now needs `attestation: None`. This includes:
- `src/gates/command.rs` — all `GateResult { ... }` blocks (will be populated in Task 3, for now set to `None`)
- `src/gates/file.rs` — all `GateResult { ... }` blocks
- `src/gates/ledger.rs` — all `GateResult { ... }` blocks
- `src/gates/query.rs` — all `GateResult { ... }` blocks
- `src/gates/snapshot.rs` — all `GateResult { ... }` blocks (will be populated in Task 4, for now set to `None`)
- `src/gates/types.rs` — composite gate results (`any_of`, `all_of`, `not`, `k_of_n`, unknown)

Run: `cargo build 2>&1 | head -40`
Expected: compiles with no errors (test still fails because command gates return `None`)

- [ ] **Step 5: Run full test suite to verify nothing else broke**

Run: `cargo test 2>&1 | tail -5`
Expected: all existing tests pass, `test_command_succeeds_produces_attestation` still fails

- [ ] **Step 6: Commit**

```bash
git add src/gates/evaluator.rs src/gates/command.rs src/gates/file.rs src/gates/ledger.rs src/gates/query.rs src/gates/snapshot.rs src/gates/types.rs tests/gate_tests.rs
git commit -m "feat: add GateAttestation struct and attestation field to GateResult"
```

---

### Task 2: Change CommandOutputOutcome to carry ExitStatus

**Files:**
- Modify: `src/gates/command.rs:31-37`, `src/gates/command.rs:256-292`
- Modify: `src/gates/snapshot.rs:89-91`

- [ ] **Step 1: Write the failing test**

Add to `tests/gate_tests.rs`:

```rust
#[test]
fn test_command_output_produces_attestation() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate(
        "command_output",
        vec![
            ("cmd", toml::Value::String("echo hello".to_string())),
            ("expect", toml::Value::String("hello".to_string())),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(result.passed);
    let att = result.attestation.expect("command_output should produce attestation");
    assert_eq!(att.gate_type, "command_output");
    assert_eq!(att.exit_code, 0);

    // stdout_hash should be sha256 of "hello\n" (raw, before trim)
    use sha2::{Digest, Sha256};
    let expected_hash = format!("{:x}", Sha256::digest(b"hello\n"));
    assert_eq!(att.stdout_hash, expected_hash);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_command_output_produces_attestation -- --nocapture 2>&1 | head -30`
Expected: FAIL — attestation is `None`

- [ ] **Step 3: Update CommandOutputOutcome to carry ExitStatus**

In `src/gates/command.rs`, change the enum:

```rust
/// Outcome of running a shell command with output capture and timeout.
pub(super) enum CommandOutputOutcome {
    /// Command completed within the timeout, producing this stdout and exit status.
    Completed(String, std::process::ExitStatus),
    /// Command exceeded the timeout and was killed.
    TimedOut,
}
```

In `run_shell_output_with_timeout`, change the `Completed` construction (around line 278-280):

```rust
            Some(_status) => {
                // Process has exited — read stdout.
                let output = child.wait_with_output()?;
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                return Ok(CommandOutputOutcome::Completed(stdout, output.status));
            }
```

- [ ] **Step 4: Update command_output to destructure the new tuple**

In `src/gates/command.rs` `eval_command_output`, change the match arm (around line 179):

```rust
        Ok(CommandOutputOutcome::Completed(stdout, status)) => {
            let trimmed = stdout.trim().to_string();
            let passed = trimmed == expect;
            GateResult {
                passed,
                evaluable: true,
                gate_type: "command_output".to_string(),
                description: format!("command output matches '{}'", expect),
                reason: if passed {
                    None
                } else {
                    Some(format!("expected '{}', got '{}'", expect, trimmed))
                },
                intent: None,
                attestation: None, // populated in Task 3
            }
        }
```

- [ ] **Step 5: Update snapshot_compare to destructure the new tuple**

In `src/gates/snapshot.rs`, change the match arm (around line 89-90):

```rust
    let stdout = match run_shell_output_with_timeout(&cmd, &ctx.working_dir, timeout_secs) {
        Ok(CommandOutputOutcome::Completed(s, _status)) => s,
```

- [ ] **Step 6: Verify everything compiles and tests pass**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass (new attestation tests still fail — `None`)

- [ ] **Step 7: Commit**

```bash
git add src/gates/command.rs src/gates/snapshot.rs tests/gate_tests.rs
git commit -m "refactor: CommandOutputOutcome carries ExitStatus alongside stdout"
```

---

### Task 3: Populate attestation in command gates

**Files:**
- Modify: `src/gates/command.rs:39-124` (eval_command_succeeds)
- Modify: `src/gates/command.rs:126-215` (eval_command_output)

- [ ] **Step 1: Add imports to command.rs**

At the top of `src/gates/command.rs`, add:

```rust
use chrono::Utc;
use sha2::{Digest, Sha256};

use super::evaluator::GateAttestation;
```

- [ ] **Step 2: Implement attestation in eval_command_succeeds**

`eval_command_succeeds` currently calls `run_shell_with_timeout` which discards stdout. Switch it to `run_shell_output_with_timeout` and build attestation. The key change is in the `match` block starting around line 84. Replace the entire match block:

```rust
    let should_attest = gate
        .params
        .get("attest")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let started_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let start = Instant::now();

    match run_shell_output_with_timeout(&cmd, &ctx.working_dir, timeout_secs) {
        Ok(CommandOutputOutcome::Completed(stdout, status)) => {
            let wall_time_ms = start.elapsed().as_millis() as u64;
            let passed = status.success();
            let attestation = if passed && should_attest {
                let stdout_hash = format!("{:x}", Sha256::digest(stdout.as_bytes()));
                Some(GateAttestation {
                    gate_type: "command_succeeds".to_string(),
                    command: cmd.clone(),
                    exit_code: status.code().unwrap_or(-1),
                    stdout_hash,
                    wall_time_ms,
                    executed_at: started_at,
                })
            } else {
                None
            };
            GateResult {
                passed,
                evaluable: true,
                gate_type: "command_succeeds".to_string(),
                description: format!("command succeeds: {}", cmd),
                reason: if passed {
                    None
                } else {
                    Some(format!(
                        "command '{}' exited with status {}",
                        cmd,
                        status.code().unwrap_or(-1)
                    ))
                },
                intent: None,
                attestation,
            }
        }
        Ok(CommandOutputOutcome::TimedOut) => GateResult {
            passed: false,
            evaluable: true,
            gate_type: "command_succeeds".to_string(),
            description: format!("command succeeds: {}", cmd),
            reason: Some(format!(
                "command '{}' timed out after {}s",
                cmd, timeout_secs
            )),
            intent: None,
            attestation: None,
        },
        Err(e) => GateResult {
            passed: false,
            evaluable: true,
            gate_type: "command_succeeds".to_string(),
            description: format!("command succeeds: {}", cmd),
            reason: Some(format!("failed to run command '{}': {}", cmd, e)),
            intent: None,
            attestation: None,
        },
    }
```

Also add `use std::time::Instant;` to the existing `use std::time::{Duration, Instant};` import (it's already there).

Remove the import of `run_shell_with_timeout` and `CommandOutcome` since `eval_command_succeeds` no longer uses them. If nothing else uses them, remove the `CommandOutcome` enum and `run_shell_with_timeout` function entirely. Check first:

Run: `cargo build 2>&1 | grep -c "unused"` — if `CommandOutcome` and `run_shell_with_timeout` are unused, delete them.

- [ ] **Step 3: Implement attestation in eval_command_output**

In the successful match arm of `eval_command_output` (the `CommandOutputOutcome::Completed` branch), add attestation. Replace the match block:

```rust
    let should_attest = gate
        .params
        .get("attest")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let started_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let start = Instant::now();

    match run_shell_output_with_timeout(&cmd, &ctx.working_dir, timeout_secs) {
        Ok(CommandOutputOutcome::Completed(stdout, status)) => {
            let wall_time_ms = start.elapsed().as_millis() as u64;
            let trimmed = stdout.trim().to_string();
            let passed = trimmed == expect;
            let attestation = if passed && should_attest {
                let stdout_hash = format!("{:x}", Sha256::digest(stdout.as_bytes()));
                Some(GateAttestation {
                    gate_type: "command_output".to_string(),
                    command: cmd.clone(),
                    exit_code: status.code().unwrap_or(-1),
                    stdout_hash,
                    wall_time_ms,
                    executed_at: started_at,
                })
            } else {
                None
            };
            GateResult {
                passed,
                evaluable: true,
                gate_type: "command_output".to_string(),
                description: format!("command output matches '{}'", expect),
                reason: if passed {
                    None
                } else {
                    Some(format!("expected '{}', got '{}'", expect, trimmed))
                },
                intent: None,
                attestation,
            }
        }
        Ok(CommandOutputOutcome::TimedOut) => GateResult {
            passed: false,
            evaluable: true,
            gate_type: "command_output".to_string(),
            description: format!("command output matches '{}'", expect),
            reason: Some(format!(
                "command '{}' timed out after {}s",
                cmd, timeout_secs
            )),
            intent: None,
            attestation: None,
        },
        Err(e) => GateResult {
            passed: false,
            evaluable: true,
            gate_type: "command_output".to_string(),
            description: format!("command output matches '{}'", expect),
            reason: Some(format!("failed to run command '{}': {}", cmd, e)),
            intent: None,
            attestation: None,
        },
    }
```

- [ ] **Step 4: Run the attestation tests**

Run: `cargo test test_command_succeeds_produces_attestation test_command_output_produces_attestation -- --nocapture 2>&1`
Expected: both PASS

- [ ] **Step 5: Run full test suite**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add src/gates/command.rs
git commit -m "feat: command gates populate attestation with stdout hash and exit code"
```

---

### Task 4: Populate attestation in snapshot_compare gate

**Files:**
- Modify: `src/gates/snapshot.rs`

- [ ] **Step 1: Write the failing test**

Add to `tests/gate_tests.rs`:

```rust
#[test]
fn test_snapshot_compare_produces_attestation() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate(
        "snapshot_compare",
        vec![
            ("cmd", toml::Value::String(r#"echo '{"count": 42}'"#.to_string())),
            ("extract", toml::Value::String("count".to_string())),
            ("compare", toml::Value::String("eq".to_string())),
            ("reference", toml::Value::String("42".to_string())),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(result.passed);
    let att = result.attestation.expect("snapshot_compare should produce attestation");
    assert_eq!(att.gate_type, "snapshot_compare");
    assert_eq!(att.exit_code, 0);
    assert!(!att.stdout_hash.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_snapshot_compare_produces_attestation -- --nocapture 2>&1 | head -20`
Expected: FAIL — attestation is `None`

- [ ] **Step 3: Add attestation to snapshot_compare**

In `src/gates/snapshot.rs`, add imports:

```rust
use chrono::Utc;
use sha2::{Digest, Sha256};
use std::time::Instant;

use super::evaluator::GateAttestation;
```

Then modify the command execution section. Replace from the `let stdout = match ...` block through the rest of the function. The key change: capture timing and status, then thread attestation through all the remaining return paths.

Replace the command execution and all subsequent code (from around line 88 to end of function):

```rust
    let should_attest = gate
        .params
        .get("attest")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let started_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let start = Instant::now();

    // Run command and get stdout with timeout enforcement.
    let (stdout, status) = match run_shell_output_with_timeout(&cmd, &ctx.working_dir, timeout_secs) {
        Ok(CommandOutputOutcome::Completed(s, st)) => (s, st),
        Ok(CommandOutputOutcome::TimedOut) => {
            return GateResult {
                passed: false,
                evaluable: true,
                gate_type: "snapshot_compare".to_string(),
                description,
                reason: Some(format!(
                    "command '{}' timed out after {}s",
                    cmd, timeout_secs
                )),
                intent: None,
                attestation: None,
            }
        }
        Err(e) => {
            return GateResult {
                passed: false,
                evaluable: true,
                gate_type: "snapshot_compare".to_string(),
                description,
                reason: Some(format!("command failed: {}", e)),
                intent: None,
                attestation: None,
            }
        }
    };

    let wall_time_ms = start.elapsed().as_millis() as u64;
```

Then keep all the existing JSON parsing and comparison logic (from `let json_value ...` through the final `GateResult`), but every `GateResult` returned after this point needs `attestation: None` (for error paths), and the final successful `GateResult` gets:

```rust
    // Build attestation for successful evaluation
    let attestation = if passed && should_attest {
        let stdout_hash = format!("{:x}", Sha256::digest(stdout.as_bytes()));
        Some(GateAttestation {
            gate_type: "snapshot_compare".to_string(),
            command: cmd.clone(),
            exit_code: status.code().unwrap_or(-1),
            stdout_hash,
            wall_time_ms,
            executed_at: started_at,
        })
    } else {
        None
    };

    GateResult {
        passed,
        evaluable: true,
        gate_type: "snapshot_compare".to_string(),
        description,
        reason: if passed {
            None
        } else {
            Some(format!(
                "{} {} {} is false",
                extracted_num, compare, reference_num
            ))
        },
        intent: None,
        attestation,
    }
```

Note: the string comparison early-return path also needs `attestation: None` on its `GateResult`.

- [ ] **Step 4: Run the test**

Run: `cargo test test_snapshot_compare_produces_attestation -- --nocapture 2>&1 | head -20`
Expected: PASS

- [ ] **Step 5: Run full test suite**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add src/gates/snapshot.rs tests/gate_tests.rs
git commit -m "feat: snapshot_compare gate populates attestation"
```

---

### Task 5: Test attest=false opt-out

**Files:**
- Test: `tests/gate_tests.rs`

- [ ] **Step 1: Write the test**

Add to `tests/gate_tests.rs`:

```rust
#[test]
fn test_attest_false_suppresses_attestation() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate(
        "command_succeeds",
        vec![
            ("cmd", toml::Value::String("true".to_string())),
            ("attest", toml::Value::Boolean(false)),
        ],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(result.passed);
    assert!(
        result.attestation.is_none(),
        "attest=false should suppress attestation"
    );
}

#[test]
fn test_non_command_gates_have_no_attestation() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let test_file = dir.path().join("exists.txt");
    std::fs::write(&test_file, "content").unwrap();

    let gate = make_gate(
        "file_exists",
        vec![(
            "path",
            toml::Value::String(test_file.to_str().unwrap().to_string()),
        )],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };
    let result = evaluate_gate(&gate, &ctx);
    assert!(result.passed);
    assert!(
        result.attestation.is_none(),
        "file_exists should never produce attestation"
    );
}
```

- [ ] **Step 2: Run both tests**

Run: `cargo test test_attest_false_suppresses_attestation test_non_command_gates_have_no_attestation -- --nocapture 2>&1`
Expected: both PASS

- [ ] **Step 3: Commit**

```bash
git add tests/gate_tests.rs
git commit -m "test: verify attest=false opt-out and non-command gates produce no attestation"
```

---

### Task 6: Test stdout hash determinism

**Files:**
- Test: `tests/gate_tests.rs`

- [ ] **Step 1: Write the test**

Add to `tests/gate_tests.rs`:

```rust
#[test]
fn test_attestation_stdout_hash_is_deterministic() {
    let dir = tempdir().unwrap();
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let gate = make_gate(
        "command_succeeds",
        vec![("cmd", toml::Value::String("echo hello".to_string()))],
    );
    let ctx = GateContext {
        ledger: &ledger,
        config: &config,
        current_state: "idle",
        state_params: HashMap::new(),
        working_dir: dir.path().to_path_buf(),
        event_fields: None,
    };

    // Compute expected hash independently
    use sha2::{Digest, Sha256};
    let expected_hash = format!("{:x}", Sha256::digest(b"hello\n"));

    let result = evaluate_gate(&gate, &ctx);
    let att = result.attestation.unwrap();
    assert_eq!(
        att.stdout_hash, expected_hash,
        "stdout hash should match sha256 of 'hello\\n'"
    );
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test test_attestation_stdout_hash_is_deterministic -- --nocapture 2>&1`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add tests/gate_tests.rs
git commit -m "test: verify attestation stdout hash matches independent sha256 computation"
```

---

### Task 7: Change StateMachine::transition() to return TransitionOutcome

**Files:**
- Modify: `src/state/machine.rs:61-67`, `src/state/machine.rs:112-218`

- [ ] **Step 1: Write the failing test**

Add to `tests/state_machine_tests.rs`:

```rust
use sahjhan::gates::evaluator::GateAttestation;
use sahjhan::state::machine::TransitionOutcome;
```

And add the test:

```rust
#[test]
fn test_transition_returns_outcome_with_states() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");
    let ledger = Ledger::init(&ledger_path, "minimal", "1.0.0").unwrap();
    let mut sm = StateMachine::new(&config, ledger);
    let outcome = sm.transition("begin", &[]).unwrap();
    assert_eq!(outcome.from, "idle");
    assert_eq!(outcome.to, "working");
    assert!(outcome.attestations.is_empty(), "no command gates, no attestations");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_transition_returns_outcome_with_states -- --nocapture 2>&1 | head -20`
Expected: FAIL — `TransitionOutcome` doesn't exist

- [ ] **Step 3: Add TransitionOutcome and update transition()**

In `src/state/machine.rs`, add the import:

```rust
use crate::gates::evaluator::GateAttestation;
```

Add the struct after `StateError`:

```rust
/// The result of a successful transition, including machine-attested gate evidence.
pub struct TransitionOutcome {
    /// State before the transition.
    pub from: String,
    /// State after the transition.
    pub to: String,
    /// Attestation evidence from command/snapshot gates that passed.
    pub attestations: Vec<GateAttestation>,
}
```

Change `transition()` signature from `Result<(), StateError>` to `Result<TransitionOutcome, StateError>`.

In the success path (after all gates pass), change the code after `self.current_state = candidate.to.clone();` to:

```rust
            // Collect attestation from passing gates.
            let attestations: Vec<GateAttestation> = results
                .into_iter()
                .filter_map(|r| r.attestation)
                .collect();

            // Emit gate_attestation events for each attestation.
            for att in &attestations {
                let mut att_fields = BTreeMap::new();
                att_fields.insert("gate_type".to_string(), att.gate_type.clone());
                att_fields.insert("command".to_string(), att.command.clone());
                att_fields.insert("exit_code".to_string(), att.exit_code.to_string());
                att_fields.insert("stdout_hash".to_string(), att.stdout_hash.clone());
                att_fields.insert("wall_time_ms".to_string(), att.wall_time_ms.to_string());
                att_fields.insert("executed_at".to_string(), att.executed_at.clone());
                att_fields.insert("transition_command".to_string(), command.to_string());
                self.ledger
                    .append("gate_attestation", att_fields)
                    .map_err(StateError::Ledger)?;
            }

            return Ok(TransitionOutcome {
                from: from_state,
                to: self.current_state.clone(),
                attestations,
            });
```

Note: you'll need to capture `from_state` before the loop starts. Add this line before the `for candidate in &candidates` loop:

```rust
        let from_state = self.current_state.clone();
```

Also, the `results` variable needs to be moved out of the if-let check. Currently the code does:

```rust
            let results = evaluate_gates(&candidate.gates, &ctx);
            let first_failure = results.iter().find(|r| !r.passed);

            if let Some(failed) = first_failure {
                // stash failure...
                continue;
            }
            // All gates passed — but `results` is still in scope
```

This should work as-is since `results` is bound before the `if let` and remains in scope after the `continue`. Just add the attestation collection after the ledger reload and transition append.

- [ ] **Step 4: Fix compilation errors in callers**

`transition()` is called in:
- `src/cli/transition.rs` `cmd_transition` — change `Ok(())` match arm to `Ok(outcome)`
- `tests/state_machine_tests.rs` — existing tests that call `sm.transition(...).unwrap()` or `.is_ok()` need updating

For `cmd_transition`, change `Ok(()) =>` to:

```rust
        Ok(outcome) => {
```

And replace `from_state` and `machine.current_state()` references with `outcome.from` and `outcome.to`.

For existing state_machine_tests, tests that do `sm.transition(...).unwrap()` (discarding the result) continue to work. Tests that do `sm.transition(...).is_ok()` also work. The test `test_valid_transition` checks `sm.current_state()` after transition — that still works because the state machine updates its internal state.

- [ ] **Step 5: Run full test suite**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests pass

- [ ] **Step 6: Commit**

```bash
git add src/state/machine.rs src/cli/transition.rs tests/state_machine_tests.rs
git commit -m "feat: transition() returns TransitionOutcome with gate attestation events"
```

---

### Task 8: Integration test — attestation events appear in ledger

**Files:**
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Create a test fixture with command gates**

We need a protocol config that has a `command_succeeds` gate on a transition. Create a helper function in `tests/integration_tests.rs`:

```rust
/// Create a temp directory with a protocol that includes command gates, and run `init`.
fn setup_attestation_dir() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        r#"
[protocol]
name = "attest-test"
version = "1.0.0"
description = "Attestation test protocol"

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

[states.done]
label = "Done"
terminal = true
"#,
    )
    .unwrap();

    std::fs::write(
        config_dir.join("transitions.toml"),
        r#"
[[transitions]]
from = "idle"
to = "done"
command = "finish"
gates = [
    { type = "command_succeeds", cmd = "echo attested" },
]
"#,
    )
    .unwrap();

    std::fs::write(config_dir.join("events.toml"), "").unwrap();
    std::fs::write(config_dir.join("renders.toml"), "").unwrap();

    std::fs::create_dir_all(dir.path().join("output")).unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "init"])
        .current_dir(dir.path())
        .assert()
        .success();

    dir
}
```

- [ ] **Step 2: Write the integration test**

```rust
#[test]
fn test_transition_emits_gate_attestation_events() {
    let dir = setup_attestation_dir();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "finish"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Dump the ledger and check for gate_attestation event
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "log", "dump"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        stdout.contains("gate_attestation"),
        "ledger should contain gate_attestation event, got:\n{}",
        stdout
    );
    assert!(
        stdout.contains("stdout_hash"),
        "gate_attestation should include stdout_hash field"
    );
    assert!(
        stdout.contains("command_succeeds"),
        "gate_attestation should reference gate_type command_succeeds"
    );

    // Verify the hash chain is still valid
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "log", "verify"])
        .current_dir(dir.path())
        .assert()
        .success();
}
```

- [ ] **Step 3: Run the test**

Run: `cargo test test_transition_emits_gate_attestation_events -- --nocapture 2>&1`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add tests/integration_tests.rs
git commit -m "test: integration test verifies gate_attestation events in ledger"
```

---

### Task 9: Integration test — attest=false suppresses event

**Files:**
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Create fixture and test**

```rust
#[test]
fn test_attest_false_suppresses_attestation_event() {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        r#"
[protocol]
name = "no-attest-test"
version = "1.0.0"
description = "Attestation opt-out test"

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

[states.done]
label = "Done"
terminal = true
"#,
    )
    .unwrap();

    std::fs::write(
        config_dir.join("transitions.toml"),
        r#"
[[transitions]]
from = "idle"
to = "done"
command = "finish"
gates = [
    { type = "command_succeeds", cmd = "echo suppressed", attest = false },
]
"#,
    )
    .unwrap();

    std::fs::write(config_dir.join("events.toml"), "").unwrap();
    std::fs::write(config_dir.join("renders.toml"), "").unwrap();

    std::fs::create_dir_all(dir.path().join("output")).unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "init"])
        .current_dir(dir.path())
        .assert()
        .success();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "finish"])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "log", "dump"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(
        !stdout.contains("gate_attestation"),
        "ledger should NOT contain gate_attestation when attest=false, got:\n{}",
        stdout
    );
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test test_attest_false_suppresses_attestation_event -- --nocapture 2>&1`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add tests/integration_tests.rs
git commit -m "test: verify attest=false suppresses gate_attestation event in ledger"
```

---

### Task 10: Integration test — restricted event blocks agent forgery

**Files:**
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Create fixture with restricted gate_attestation event**

```rust
#[test]
fn test_gate_attestation_restricted_blocks_agent() {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        r#"
[protocol]
name = "restricted-test"
version = "1.0.0"
description = "Restricted attestation test"

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

[states.done]
label = "Done"
terminal = true
"#,
    )
    .unwrap();

    std::fs::write(
        config_dir.join("transitions.toml"),
        r#"
[[transitions]]
from = "idle"
to = "done"
command = "finish"
gates = []
"#,
    )
    .unwrap();

    std::fs::write(
        config_dir.join("events.toml"),
        r#"
[events.gate_attestation]
description = "Machine-attested gate evidence"
restricted = true
fields = [
    { name = "gate_type" },
    { name = "command" },
    { name = "exit_code" },
    { name = "stdout_hash" },
    { name = "wall_time_ms" },
    { name = "executed_at" },
    { name = "transition_command" },
]
"#,
    )
    .unwrap();

    std::fs::write(config_dir.join("renders.toml"), "").unwrap();

    std::fs::create_dir_all(dir.path().join("output")).unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "init"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Agent tries to forge a gate_attestation via `event record`
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "event",
            "gate_attestation",
            "--field",
            "gate_type=command_succeeds",
            "--field",
            "command=true",
            "--field",
            "exit_code=0",
            "--field",
            "stdout_hash=abc123",
            "--field",
            "wall_time_ms=100",
            "--field",
            "executed_at=2026-03-30T00:00:00Z",
            "--field",
            "transition_command=finish",
        ])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("restricted"));
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test test_gate_attestation_restricted_blocks_agent -- --nocapture 2>&1`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add tests/integration_tests.rs
git commit -m "test: verify restricted gate_attestation blocks agent forgery via event record"
```

---

### Task 11: Integration test — multi-candidate branching attestation

**Files:**
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Write the test**

```rust
#[test]
fn test_branching_only_winning_candidate_attested() {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        r#"
[protocol]
name = "branch-attest-test"
version = "1.0.0"
description = "Branching attestation test"

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

[states.passed]
label = "Passed"
terminal = true

[states.failed]
label = "Failed"
terminal = true
"#,
    )
    .unwrap();

    // First candidate has a failing gate, second is fallback
    std::fs::write(
        config_dir.join("transitions.toml"),
        r#"
[[transitions]]
from = "idle"
to = "passed"
command = "check"
gates = [
    { type = "command_succeeds", cmd = "false" },
]

[[transitions]]
from = "idle"
to = "failed"
command = "check"
gates = [
    { type = "command_succeeds", cmd = "echo fallback" },
]
"#,
    )
    .unwrap();

    std::fs::write(config_dir.join("events.toml"), "").unwrap();
    std::fs::write(config_dir.join("renders.toml"), "").unwrap();

    std::fs::create_dir_all(dir.path().join("output")).unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "init"])
        .current_dir(dir.path())
        .assert()
        .success();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "check"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("failed"));

    // Check ledger: should have exactly one gate_attestation (from fallback candidate)
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "log", "dump"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);

    let attestation_count = stdout.matches("gate_attestation").count();
    assert_eq!(
        attestation_count, 1,
        "should have exactly 1 gate_attestation (from winning candidate only), found {}",
        attestation_count
    );

    // The attestation should reference the fallback command, not the failing one
    assert!(
        stdout.contains("echo fallback"),
        "attestation should be from the winning candidate's gate"
    );
}
```

- [ ] **Step 2: Run the test**

Run: `cargo test test_branching_only_winning_candidate_attested -- --nocapture 2>&1`
Expected: PASS

- [ ] **Step 3: Commit**

```bash
git add tests/integration_tests.rs
git commit -m "test: multi-candidate branching only attests winning candidate"
```

---

### Task 12: Update documentation

**Files:**
- Modify: `src/gates/evaluator.rs` (index header)
- Modify: `src/gates/command.rs` (index header)
- Modify: `src/state/machine.rs` (index header)
- Modify: `CLAUDE.md` (module tables, flow maps, test table)
- Modify: `README.md` (new section)

- [ ] **Step 1: Update source file index headers**

In `src/gates/evaluator.rs`, update the index comment at top to add:

```
// - GateAttestation          — machine-attested evidence from command/snapshot gate execution
```

In `src/gates/command.rs`, update the index comment to note attestation:

```
// - [eval-command-succeeds]        eval_command_succeeds()         — run a shell command; pass if exit code is 0; captures stdout for attestation
// - [eval-command-output]          eval_command_output()           — run a shell command; pass if stdout matches expected string; captures stdout for attestation
```

In `src/state/machine.rs`, update the index to add:

```
// - TransitionOutcome        — result of successful transition: from, to, attestations
```

And update the `[transition]` line to note:

```
// - [transition]             transition()              — execute named command (multi-candidate branching with fallthrough); returns TransitionOutcome with attestation
```

- [ ] **Step 2: Update CLAUDE.md module lookup tables**

In the `gates/ — Gate Evaluation` table, add:

```
| Gate attestation | `gates/evaluator.rs` | `GateAttestation` | Machine-attested evidence (stdout hash, exit code, timing) |
```

In the `state/ — State Machine` table, add:

```
| Transition outcome | `state/machine.rs` | `TransitionOutcome` | Returned by transition(): from, to, attestations |
```

- [ ] **Step 3: Update CLAUDE.md Transition Lifecycle flow map**

After the existing `→ ledger/chain.rs [ledger-append]` line, add:

```
      → for each GateAttestation from passing gates:
        → ledger/chain.rs [ledger-append]           ← gate_attestation event (stdout_hash, exit_code, etc.)
```

- [ ] **Step 4: Update CLAUDE.md test files table**

Update the `tests/gate_tests.rs` entry to:

```
| `tests/gate_tests.rs` | All gate types, template interpolation, field validation, StateParam source, attestation |
```

- [ ] **Step 5: Update README.md**

Add a new section after "### Config integrity" (before "## What a protocol looks like"). This section explains gate attestation in the README's voice — first-person, narrative, dryly sarcastic, building on the trust/verification arc. Write it as a natural continuation of the "what the agent tried next" narrative.

```markdown
### Gate attestation

So the ledger can't be edited. Restricted events need proof. Config is sealed. What about the gates themselves?

When a `command_succeeds` gate runs `python -m pytest tests/`, Sahjhan executes the command, checks the exit code, and records a `state_transition` event: "moved from implementing to verifying." That's it. The transition happened. But the ledger says nothing about *why* it was allowed. What command ran? What did it output? When? The evidence exists for about three hundred milliseconds inside a Rust struct, then gets thrown away.

Which means: you can see that the agent transitioned, but you can't see that the tests actually passed. You're trusting the gate's boolean. That's better than trusting the agent, but it's still a gap you could drive a fabricated quiz result through.

Gate attestation closes it. When a `command_succeeds`, `command_output`, or `snapshot_compare` gate passes during a transition, Sahjhan now emits a `gate_attestation` event immediately after the `state_transition`:

```bash
sahjhan log tail 2
# {"event_type": "state_transition", "fields": {"from": "implementing", "to": "verifying", "command": "submit"}, ...}
# {"event_type": "gate_attestation", "fields": {"gate_type": "command_succeeds", "command": "python -m pytest tests/", "exit_code": "0", "stdout_hash": "a3c2e88d1f2b...", "wall_time_ms": "4523", "executed_at": "2026-03-30T14:23:07.123Z", "transition_command": "submit"}, ...}
```

The `stdout_hash` is SHA-256 of the raw command output. The agent can't fabricate it because Sahjhan runs the command and computes the hash — the agent never touches either. For deterministic commands (most test suites, linters, build tools), replaying the command should reproduce the hash. That's an independently verifiable claim sitting in a hash-chained ledger.

Every command and snapshot gate attests by default. If a gate runs something trivial that isn't worth recording (a warmup check, an `echo`), suppress it:

```toml
{ type = "command_succeeds", cmd = "echo warmup", attest = false }
```

The attestation event is `restricted` — mark it in your `events.toml` and the agent can't forge one via `sahjhan event record`. It'll get the same rejection as a fabricated quiz result:

```bash
sahjhan event gate_attestation --field gate_type=command_succeeds --field stdout_hash=abc123 ...
# error: event type 'gate_attestation' is restricted. Use 'sahjhan authed-event' with a valid proof.
```

The ledger now has two tiers of evidence: machine-attested (the gate ran, here's the hash) and agent-reported (I reviewed this, trust me). Different confidence levels, explicitly marked. An auditor can tell which is which. The agent can't blur the line.
```

- [ ] **Step 6: Run clippy and tests one final time**

Run: `cargo clippy -- -D warnings 2>&1 | tail -10`
Run: `cargo test 2>&1 | tail -5`
Expected: no warnings, all tests pass

- [ ] **Step 7: Commit**

```bash
git add src/gates/evaluator.rs src/gates/command.rs src/state/machine.rs CLAUDE.md README.md
git commit -m "docs: update indexes, flow maps, and README for gate attestation"
```

---

### Task 13: Clean up unused code

**Files:**
- Modify: `src/gates/command.rs` (if CommandOutcome / run_shell_with_timeout are now unused)

- [ ] **Step 1: Check for unused code**

Run: `cargo build 2>&1 | grep "warning.*dead_code\|warning.*unused"`
If `CommandOutcome` and `run_shell_with_timeout` are flagged as unused (since `eval_command_succeeds` now uses `run_shell_output_with_timeout`), remove them.

- [ ] **Step 2: Remove unused code if present**

Delete the `CommandOutcome` enum and `run_shell_with_timeout` function from `src/gates/command.rs` if nothing references them.

Update the index comment at the top of `command.rs` to remove the deleted items.

- [ ] **Step 3: Run clippy and full tests**

Run: `cargo clippy -- -D warnings 2>&1 | tail -10`
Run: `cargo test 2>&1 | tail -5`
Expected: clean

- [ ] **Step 4: Commit**

```bash
git add src/gates/command.rs
git commit -m "chore: remove unused CommandOutcome and run_shell_with_timeout"
```
