# Restricted Event Types with HMAC Authentication

**Date:** 2026-03-29
**Status:** Approved
**Issue:** jbrjake/sahjhan#11
**Goal:** Move from honor-system event recording to capability-restricted enforcement. Restricted event types require HMAC proof; session keys are per-ledger with global fallback; a read-guard manifest tells hooks which paths to block; a negation gate enables "must not have done X" constraints.

## Background

Holtz Run 25 post-mortem: the agent operates the CLI that constrains it. Any gate can be bypassed by recording success events directly via `sahjhan event`. This feature adds cryptographic proof requirements to critical event types.

## Feature 1: Restricted Event Types

### Config Change: `events.toml`

Add optional `restricted` field to `EventConfig`:

```rust
#[derive(Debug, Deserialize, Clone)]
pub struct EventConfig {
    pub description: String,
    #[serde(default)]
    pub restricted: Option<bool>,
    pub fields: Vec<EventFieldConfig>,
}
```

TOML usage:
```toml
[events.quiz_answered]
description = "Lens subagent answered quiz"
restricted = true
fields = [...]
```

Defaults to `false` when absent. Backward compatible.

### Behavior Change: `sahjhan event`

In `cmd_event`, after loading config and before field parsing, check restriction:

```
if config.events[event_type].restricted == Some(true) {
    error: event type '<type>' is restricted. Use 'sahjhan authed-event' with a valid proof.
    return EXIT_USAGE_ERROR
}
```

Exit code: 4 (usage error).

## Feature 2: Session Key Management

### Key Storage Layout

```
<data_dir>/
  session.key              <- global key (created by init)
  ledgers/
    <name>/
      session.key          <- per-ledger key (created by ledger create)
```

### Key Generation

32 bytes from `getrandom::getrandom()` (already a dependency). Written as raw bytes.

### Key Resolution Order

Used by `authed-event` and `config session-key-path`:

1. If `--ledger <name>` specified, check `<data_dir>/ledgers/<name>/session.key`
2. If that file exists, use it
3. Otherwise fall back to `<data_dir>/session.key`

### `sahjhan init` Changes

After creating ledger and registry, generate `<data_dir>/session.key`. Overwrites any existing key (new session = new key, per the issue spec).

### `sahjhan ledger create` Changes

After registering the ledger, create `<data_dir>/ledgers/<name>/` directory and generate `session.key` inside it.

### New Command: `sahjhan config session-key-path`

New subcommand group: `Config { action: ConfigAction }` with variant `SessionKeyPath`.

```rust
Config {
    #[command(subcommand)]
    action: ConfigAction,
}

enum ConfigAction {
    SessionKeyPath,
}
```

Behavior:
- Respects global `--ledger` flag for per-ledger resolution
- Outputs the absolute path to the resolved key file
- Does NOT output the key contents
- Exits with error if the resolved key file doesn't exist

## Feature 3: Authenticated Event Recording

### New Command: `sahjhan authed-event`

```rust
AuthedEvent {
    #[arg(value_name = "TYPE")]
    event_type: String,
    #[arg(long = "field", value_name = "KEY=VALUE")]
    fields: Vec<String>,
    #[arg(long)]
    proof: String,
}
```

### Validation Flow

1. Load config, parse `--field` pairs (same validation as `cmd_event`)
2. Verify event type IS restricted. Error if not: `error: event type '<type>' is not restricted. Use 'sahjhan event' instead.` (prevents using authed-event as a general bypass)
3. Resolve session key using resolution order from Feature 2
4. Build canonical payload: `event_type\0field1_name=field1_value\0field2_name=field2_value` with fields sorted lexicographically by name, null byte separator
5. Compute `HMAC-SHA256(canonical_payload, session_key)`, hex-encode
6. Compare to `--proof` value
7. Match: record event normally (shared helper with `cmd_event`)
8. Mismatch: `error: invalid proof for event '<type>'`, exit `EXIT_INTEGRITY_ERROR`

### Canonical Payload Example

```
quiz_answered\0auditor=holtz\0pass=true\0perspective=component\0project=holtz\0run=25\0score=5/5
```

### Shared Recording Helper

Extract post-validation recording + render triggering from `cmd_event` into a shared function:

```rust
fn record_event_and_render(
    config: &ProtocolConfig,
    config_path: &Path,
    machine: &mut StateMachine,
    manifest: &mut Manifest,
    data_dir: &Path,
    event_type: &str,
    fields: HashMap<String, String>,
    targeting: &LedgerTargeting,
) -> i32
```

Both `cmd_event` and `cmd_authed_event` call this after their respective validation passes.

### New Dependency

`hmac = "0.12"` in Cargo.toml. `sha2` is already present.

## Feature 4: Read-Guard Manifest

### Config Change: `protocol.toml`

```rust
// In ProtocolFile:
#[serde(default)]
pub guards: Option<GuardsConfig>,

#[derive(Debug, Deserialize, Clone, Default)]
pub struct GuardsConfig {
    #[serde(default)]
    pub read_blocked: Vec<String>,
}
```

TOML usage:
```toml
[guards]
read_blocked = [
    ".sahjhan/session.key",
    "enforcement/quiz-bank.json",
]
```

Optional section. Backward compatible.

### New Command: `sahjhan guards`

```rust
Guards,
```

Behavior:
1. Load config
2. Collect `guards.read_blocked` (empty if no `[guards]` section)
3. Auto-include `<data_dir>/session.key` (defense in depth)
4. Deduplicate
5. Output JSON to stdout:

```json
{
  "read_blocked": [
    ".sahjhan/session.key",
    "enforcement/quiz-bank.json"
  ]
}
```

No ledger interaction needed.

### `ProtocolConfig` Propagation

Add `guards: Option<GuardsConfig>` to `ProtocolConfig`. Loaded from `ProtocolFile.guards` in `ProtocolConfig::load()`.

## Feature 5: Negation Gate — `ledger_lacks_event`

### Gate Implementation

New function in `gates/ledger.rs`:

```rust
// [eval-ledger-lacks-event]
pub(super) fn eval_ledger_lacks_event(gate: &GateConfig, ctx: &GateContext) -> GateResult
```

Parameters (mirror `ledger_has_event`):
- `event` (required): event type to check
- `filter` (optional): key/value map to narrow matching

Logic: count matching events (same filter logic as `ledger_has_event`). Pass if count == 0. Fail with reason listing count.

### Wiring

- `gates/types.rs` `eval()`: add `"ledger_lacks_event" => super::ledger::eval_ledger_lacks_event(gate, ctx)`
- `gates/evaluator.rs` `default_intent()`: add `"ledger_lacks_event" => "prohibited events must not exist"`
- `config/mod.rs` `validate_deep()`: add `("ledger_lacks_event", vec!["event"])` to `known_gates`

### TOML Usage

```toml
gates = [
    { type = "ledger_lacks_event", event = "finding", filter = { phase = "audit" }, intent = "no audit findings should exist before recon_complete" },
]
```

## File Changes Summary

### New Files

| File | Purpose |
|------|---------|
| `src/cli/guards.rs` | `cmd_guards()` |
| `src/cli/config_cmd.rs` | `cmd_session_key_path()` |
| `src/cli/authed_event.rs` | `cmd_authed_event()` |
| `tests/auth_tests.rs` | All auth/restriction/guard/negation-gate tests |

### Modified Files

| File | Changes |
|------|---------|
| `Cargo.toml` | Add `hmac = "0.12"` |
| `src/config/events.rs` | Add `restricted` field |
| `src/config/protocol.rs` | Add `GuardsConfig`, `guards` to `ProtocolFile` |
| `src/config/mod.rs` | Propagate `guards`, add `ledger_lacks_event` to validation |
| `src/gates/types.rs` | Add `ledger_lacks_event` dispatch |
| `src/gates/evaluator.rs` | Add default intent |
| `src/gates/ledger.rs` | Add `eval_ledger_lacks_event()` |
| `src/cli/mod.rs` | Add `guards`, `config_cmd`, `authed_event` modules |
| `src/cli/transition.rs` | Add restriction check, extract shared recording helper |
| `src/cli/init.rs` | Add session key generation |
| `src/cli/ledger.rs` | Add per-ledger session key generation |
| `src/main.rs` | Add `AuthedEvent`, `Guards`, `Config` variants + dispatch |
| `CLAUDE.md` | Update module tables, flow maps, gate tables |

## Test Plan

All in `tests/auth_tests.rs`:

1. **Restricted event rejection:** `sahjhan event` on restricted type returns exit 4 with correct error message
2. **Authed-event valid proof:** `sahjhan authed-event` with correct HMAC succeeds, event appears in ledger
3. **Authed-event invalid proof:** wrong proof returns exit 2
4. **Session key creation:** `sahjhan init` creates `<data_dir>/session.key` with exactly 32 bytes
5. **Session key path:** `sahjhan config session-key-path` returns correct absolute path; with `--ledger` returns per-ledger path
6. **Guards output:** `sahjhan guards` returns JSON with configured paths plus auto-included session key
7. **Negation gate — pass:** `ledger_lacks_event` passes when no matching events exist
8. **Negation gate — fail:** fails when matching events exist, reason includes count
9. **Negation gate — filter:** filter narrows the check correctly

## Non-Goals

- Sahjhan does NOT enforce read-blocking (hooks do that)
- Sahjhan does NOT know about Claude Code or agents
- No ledger format changes — authenticated events stored identically to regular events
- Proof is verified at recording time, not stored
