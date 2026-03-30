# Gate Attestation Design

**Date:** 2026-03-30
**Issue:** #15 — event attestation is just agent testimony

## Problem

When an agent records an event via `sahjhan event record`, the ledger immortalizes the claim, not the fact. Gate evaluation produces evidence (exit codes, stdout, timing) that is discarded after the pass/fail decision. The `state_transition` event records only `{from, to, command}` — zero gate evidence reaches the ledger.

The gap: gates execute, produce evidence, and the evidence is thrown away. An observer can see that a transition happened but not *why it was allowed to happen*.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Where attestation lives | Separate `gate_attestation` events | Keeps `state_transition` lean; attestation is independently queryable |
| Which gates attest | `command_succeeds`, `command_output`, `snapshot_compare` | These execute external processes whose results can't be verified from the ledger alone |
| Opt-in vs opt-out | On by default, opt-out via `attest = false` | Good defaults; protocol authors suppress only when needed |
| What `command_succeeds` captures | stdout (hashed), exit code, wall time | Requires switching from `run_shell_with_timeout` to `run_shell_output_with_timeout` |
| Which gates get attested | Only passing gates on the winning candidate | Failed candidates and failed gates leave no attestation trace |
| Agent forgery prevention | `gate_attestation` is `restricted = true` | Blocks `sahjhan event record gate_attestation`; state machine writes via internal `ledger.append()` which bypasses CLI restriction check |

## Data Model

### GateAttestation (new struct in `gates/evaluator.rs`)

```rust
pub struct GateAttestation {
    /// Gate type that produced this attestation.
    pub gate_type: String,
    /// Resolved command that was executed.
    pub command: String,
    /// Numeric exit code.
    pub exit_code: i32,
    /// SHA-256 hex of raw stdout.
    pub stdout_hash: String,
    /// Execution wall time in milliseconds.
    pub wall_time_ms: u64,
    /// RFC3339 timestamp of execution start.
    pub executed_at: String,
}
```

### GateResult changes

`GateResult` gains one field:

```rust
pub attestation: Option<GateAttestation>,
```

All existing gate evaluators set it to `None`. Only `command_succeeds`, `command_output`, and `snapshot_compare` populate it (when `attest != Some(false)`).

### GateConfig changes

`GateConfig` in `config/transitions.rs` gains:

```rust
pub attest: Option<bool>,
```

Serde default: `None` (meaning "use default behavior" = attest). Gate evaluators check `gate.attest != Some(false)` before populating attestation.

### TransitionOutcome (new struct in `state/machine.rs`)

```rust
pub struct TransitionOutcome {
    pub from: String,
    pub to: String,
    pub attestations: Vec<GateAttestation>,
}
```

`StateMachine::transition()` changes from `Result<(), StateError>` to `Result<TransitionOutcome, StateError>`.

## Gate Changes

### `CommandOutputOutcome` changes

`CommandOutputOutcome::Completed` currently carries only the stdout string. It needs to also carry `ExitStatus` so attestation can extract the exit code. Change to:

```rust
pub(super) enum CommandOutputOutcome {
    Completed(String, std::process::ExitStatus),
    TimedOut,
}
```

Inside `run_shell_output_with_timeout`, `output.status` is already available from `wait_with_output()` — just include it in the return. All callers of `run_shell_output_with_timeout` (`command_output`, and now `command_succeeds`) destructure the new tuple.

### `command_succeeds`

Currently calls `run_shell_with_timeout` which pipes stdout to null. Changes to call `run_shell_output_with_timeout` instead. The stdout content is ignored for the pass/fail decision but:
- Hashed (SHA-256) for `stdout_hash`
- Exit code extracted from `ExitStatus` for `exit_code`
- Wall time measured for `wall_time_ms`
- Execution start recorded for `executed_at`

### `command_output`

Already captures stdout via `run_shell_output_with_timeout`. Adds:
- SHA-256 hash of raw (pre-trim) stdout
- Exit code from the new `ExitStatus` in `CommandOutputOutcome`
- Wall time, execution timestamp

### `snapshot_compare`

Runs a command and extracts JSON. Adds attestation with the same fields — hash of raw command output, exit code, timing.

### Opt-out

In `transitions.toml`:

```toml
gates = [
    { type = "command_succeeds", cmd = "echo warmup", attest = false },
    { type = "command_succeeds", cmd = "python -m pytest tests/" },
]
```

The first gate produces no attestation event. The second does (default behavior).

## State Machine Flow

Updated `transition()` flow:

1. Collect candidates, build state params, evaluate gates (unchanged)
2. If all gates pass on winning candidate:
   a. Collect `GateAttestation` from each `GateResult` (filtering `None`)
   b. Reload ledger from disk (unchanged, Issue #3)
   c. Append `state_transition` event with `{from, to, command}` (unchanged)
   d. **NEW:** For each attestation, append `gate_attestation` event with fields:
      - `gate_type` — e.g. "command_succeeds"
      - `command` — resolved command string
      - `exit_code` — numeric exit code as string
      - `stdout_hash` — SHA-256 hex
      - `wall_time_ms` — wall time as string
      - `executed_at` — RFC3339 timestamp
      - `transition_command` — the command name that triggered the transition (for correlation)
   e. Return `TransitionOutcome { from, to, attestations }`
3. `cmd_transition` destructures `TransitionOutcome` instead of `()`

The `gate_attestation` events land immediately after the `state_transition` in the hash chain.

## Event Schema

Protocol authors who want field validation add to `events.toml`:

```toml
[events.gate_attestation]
description = "Machine-attested gate passage evidence"
restricted = true
fields = [
    { name = "gate_type", pattern = "^(command_succeeds|command_output|snapshot_compare)$" },
    { name = "command" },
    { name = "exit_code", pattern = "^-?[0-9]+$" },
    { name = "stdout_hash", pattern = "^[0-9a-f]{64}$" },
    { name = "wall_time_ms", pattern = "^[0-9]+$" },
    { name = "executed_at" },
    { name = "transition_command" },
]
```

`restricted = true` blocks agent forgery via `sahjhan event record`. The state machine writes via `ledger.append()` which bypasses CLI restriction enforcement — this is the existing trust boundary (CLI validates, internal API doesn't).

## CLI Impact

### `cmd_transition`

Destructures `TransitionOutcome` instead of `()`. Uses `outcome.from` / `outcome.to` instead of locally tracked state names. No new output — attestation events are written silently as part of the transition.

### `cmd_gate_check` (dry-run)

No change to gate-check behavior. Attestation is only emitted on actual transitions, not dry runs.

## Testing

### Unit tests (in `tests/gate_tests.rs`)

- `GateAttestation` populated correctly by `command_succeeds`, `command_output`, `snapshot_compare`
- `stdout_hash` is deterministic SHA-256 of raw stdout (e.g., `echo hello` → `sha256("hello\n")`)
- `attest = false` suppresses attestation (`attestation` is `None`)
- `wall_time_ms` and `exit_code` are accurate
- Gates that don't attest (`file_exists`, `ledger_has_event`, etc.) return `attestation: None`

### Integration tests (in `tests/integration_tests.rs`)

- Full transition with command gates produces `gate_attestation` events in ledger after `state_transition`
- `gate_attestation` events contain correct `transition_command` correlation
- `sahjhan event record gate_attestation ...` is rejected (restricted)
- `attest = false` on a gate means no attestation event for that gate
- Multi-candidate branching: only the winning candidate's gates are attested
- Existing tests pass with new `TransitionOutcome` return type

### Attestation verification test

- Run deterministic command (`echo hello`), verify `stdout_hash == sha256("hello\n")`
- Replay command independently, verify hash matches

## Documentation Updates

### Source file indexes

- `gates/evaluator.rs` — Add `GateAttestation` struct
- `gates/command.rs` — Note stdout capture for attestation
- `config/transitions.rs` — Add `attest` field to `GateConfig`
- `state/machine.rs` — `TransitionOutcome` struct, updated `[transition]` return type

### CLAUDE.md

- Module lookup tables: add `GateAttestation`, `TransitionOutcome`, `attest`
- Flow maps: update Transition Lifecycle to show attestation event emission
- Test files table: note new attestation tests

### README.md

New section covering gate attestation, written in the README's existing voice (first-person, narrative, dryly sarcastic). Placed after the "Restricted events and HMAC authentication" section, as a natural continuation of the trust/verification narrative arc. Explains what attestation solves, shows the evidence in the ledger, demonstrates the restricted event protection.
