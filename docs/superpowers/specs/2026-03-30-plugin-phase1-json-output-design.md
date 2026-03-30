# Phase 1: Stable `--json` Output

**Date:** 2026-03-30
**Issue:** #18 (Plugin System — Phase 1 of 7)
**Status:** Approved

## Goal

Add `--json` flag to 6 CLI commands with versioned, enveloped output. Refactor command handlers to data-first architecture (build struct, then format). Create HORIZONS-1 example protocol config for integration testing across all plugin phases.

## Decisions

- **Envelope style**: Standard envelope wrapping all responses
- **Error handling**: Structured JSON errors to stdout (with exit codes preserved)
- **Output architecture**: Data-first — each command builds a typed result struct, single serialization point formats as text or JSON (Approach C)
- **Command scope**: 6 commands — `status`, `log dump`, `log tail`, `gate check`, `manifest verify`, `set status`
- **HORIZONS-1 location**: `examples/horizons1/` (parallel to `examples/minimal/`)
- **HORIZONS-1 scope**: Full protocol config (states, transitions, events, sets). No plugin-specific config or data files yet — those arrive in their respective phases.

## JSON Envelope

Every JSON response wraps in:

```json
{"schema_version": 1, "ok": true, "command": "status", "data": {...}}
```

Error responses:

```json
{"schema_version": 1, "ok": false, "command": "status", "error": {"code": "integrity_error", "message": "chain invalid", "details": null}}
```

Schema versioning: `schema_version: 1` in every response. Additive changes don't bump version. Removals or renames of existing fields do.

Error codes (machine-readable): `gate_blocked`, `integrity_error`, `usage_error`, `config_error`.

## Core Types

### `src/cli/output.rs` (new file)

```rust
pub const SCHEMA_VERSION: u64 = 1;

/// Trait for type-erased command dispatch.
pub trait CommandOutput {
    fn to_json(&self) -> String;
    fn to_text(&self) -> String;
    fn exit_code(&self) -> i32;
}

/// Typed command result with envelope.
pub struct CommandResult<T: Serialize + Display> {
    pub ok: bool,
    pub command: String,
    pub data: Option<T>,
    pub error: Option<ErrorData>,
    pub exit_code: i32,
}

#[derive(Serialize)]
pub struct ErrorData {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

// Constructors:
// CommandResult::ok(command, data) -> Self
// CommandResult::err(command, exit_code, code, message) -> Self
// CommandResult::err_with_details(command, exit_code, code, message, details) -> Self

// Blanket impl: CommandResult<T: Serialize + Display> implements CommandOutput
// to_json() serializes envelope {schema_version, ok, command, data/error}
// to_text() calls data.to_string() (Display) or formats error to stderr-style string
```

### Legacy shim

Commands not yet converted return `i32`. A `LegacyResult` struct implements `CommandOutput` with empty data, preserving exit code. This allows incremental migration — unconverted commands still work through the same dispatch path.

## Per-Command Data Structs

All in `src/cli/output.rs`, all `#[derive(Serialize)]` + `impl Display`.

### StatusData

```rust
pub struct StatusData {
    pub state: String,
    pub event_count: u64,
    pub chain_valid: bool,
    pub chain_error: Option<String>,
    pub sets: Vec<SetSummaryData>,
    pub transitions: Vec<TransitionSummaryData>,
}

pub struct SetSummaryData {
    pub name: String,
    pub completed: usize,
    pub total: usize,
    pub members: Vec<MemberData>,
}

pub struct MemberData {
    pub name: String,
    pub done: bool,
}

pub struct TransitionSummaryData {
    pub command: String,
    pub from: String,
    pub to: String,
    pub ready: bool,
    pub gates: Vec<GateResultData>,
}

pub struct GateResultData {
    pub gate_type: String,
    pub passed: bool,
    pub evaluable: bool,
    pub description: String,
    pub reason: Option<String>,
    pub intent: Option<String>,
}
```

### LogData (shared by log dump + log tail)

```rust
pub struct LogData {
    pub entries: Vec<EntryData>,
}

pub struct EntryData {
    pub seq: u64,
    pub timestamp: String,
    pub event_type: String,
    pub hash: String,
    pub fields: BTreeMap<String, String>,
}
```

JSON gets full hashes. Display impl truncates to 12 chars (matching current behavior).

### GateCheckData

```rust
pub struct GateCheckData {
    pub transition: String,
    pub current_state: String,
    pub candidates: Vec<CandidateData>,
    pub result: String,
    pub would_take: Option<String>,
}

pub struct CandidateData {
    pub from: String,
    pub to: String,
    pub gates: Vec<GateResultData>,
    pub all_passed: bool,
}
```

### ManifestVerifyData

```rust
pub struct ManifestVerifyData {
    pub clean: bool,
    pub tracked_count: usize,
    pub mismatches: Vec<MismatchData>,
}

pub struct MismatchData {
    pub path: String,
    pub expected: String,
    pub actual: Option<String>,
}
```

### SetStatusData

Reuses `SetSummaryData` directly (same struct, same Display — one set's worth of data).

### EventOnlyStatusData

For event-only ledgers in the `status` command:

```rust
pub struct EventOnlyStatusData {
    pub event_count: u64,
    pub chain_valid: bool,
    pub chain_error: Option<String>,
}
```

## CLI Plumbing

### Global `--json` flag

```rust
// main.rs Cli struct
#[arg(long, global = true)]
json: bool,
```

### Dispatch in main()

```rust
let result: Box<dyn CommandOutput> = match cli.command {
    Commands::Status => status::cmd_status(&cli.config_dir, &targeting),
    // ... converted commands return Box<dyn CommandOutput>
    // ... unconverted commands wrapped in LegacyResult
};

if cli.json {
    // JSON mode: everything to stdout (success and errors)
    println!("{}", result.to_json());
} else {
    // Text mode: success to stdout, errors to stderr (preserves current behavior)
    if result.exit_code() == 0 {
        print!("{}", result.to_text());
    } else {
        eprint!("{}", result.to_text());
    }
}
std::process::exit(result.exit_code());
```

### Command signature changes

Before: `pub fn cmd_status(config_dir: &str, targeting: &LedgerTargeting) -> i32`
After: `pub fn cmd_status(config_dir: &str, targeting: &LedgerTargeting) -> Box<dyn CommandOutput>`

The `eprintln!` + `return code` pattern becomes `return CommandResult::err(...)`. The `println!` at the end becomes building the data struct and returning `CommandResult::ok(...)`.

## HORIZONS-1 Protocol Config

### `examples/horizons1/protocol.toml`

```toml
[protocol]
name = "horizons1"
version = "1.0.0"
description = "Interplanetary probe mission control protocol"

[paths]
managed = ["output"]
data_dir = "output/.sahjhan"
render_dir = "output"

[sets.subsystems]
description = "Probe subsystem verification"
values = ["eps", "adcs", "telecom", "propulsion", "payload"]
```

### `examples/horizons1/states.toml`

9 states: `pre_launch` (initial) through `mission_complete` (terminal), plus `anomaly`.

### `examples/horizons1/transitions.toml`

Transitions between mission phases. Gates use built-in types (`file_exists`) as placeholders for Phase 1. Phase 3 replaces these with plugin gate types.

Key transitions:
- `complete_assembly`: pre_launch → assembly_complete
- `begin_testing`: assembly_complete → testing
- `clear_for_launch`: testing → launch_ready (gated on subsystem set coverage)
- `launch`: launch_ready → launched
- `begin_cruise`: launched → cruise
- `begin_encounter`: cruise → encounter
- `begin_science`: encounter → science_ops
- `complete_mission`: science_ops → mission_complete
- `declare_anomaly`: any state → anomaly

### `examples/horizons1/events.toml`

Event types: `telemetry_update`, `trajectory_update`, `anomaly_report`, `science_data_downlink`, `subsystem_checkout`.

### `examples/horizons1/renders.toml`

Empty array for Phase 1. Renderer plugin arrives in Phase 5.

## Testing Strategy

### Unit tests (`tests/json_output_tests.rs`)

- `CommandResult::ok` serializes correct envelope (schema_version, ok, command, data)
- `CommandResult::err` serializes error envelope (code, message, details)
- `LegacyResult` wraps exit codes correctly
- Each data struct's Display impl matches existing text output format

### Integration tests (added to `tests/integration_tests.rs` or new `tests/json_integration_tests.rs`)

- `status --json` with examples/minimal: parse JSON, verify state/sets/transitions fields
- `log dump --json`: verify entries array with seq/timestamp/hash/fields
- `log tail --json 3`: verify correct count of entries
- `gate check --json <transition>`: verify candidates/result/would_take
- `manifest verify --json`: verify clean/tracked_count
- `set status --json <set>`: verify completed/total/members

### HORIZONS-1 tests (`tests/horizons1_tests.rs`)

- Init horizons1 protocol, verify `status --json` shows `pre_launch`
- Transition through first few phases, verify JSON reflects state changes
- Gate check shows blocked transitions with structured gate results
- Set operations work and JSON reflects subsystem completion

### Error tests

- Unknown set → JSON error with code `usage_error`
- Tampered ledger → JSON error with code `integrity_error`
- Gate blocked → JSON error with code `gate_blocked` and per-gate details

### Backward compatibility

- Run existing integration tests unchanged — text output must not change
- `--json` flag must not affect commands that don't have it yet (they work normally)

## Files Changed

| File | Change |
|------|--------|
| `src/cli/output.rs` | **New** — CommandResult, CommandOutput trait, data structs, Display impls |
| `src/cli/mod.rs` | Export output module |
| `src/main.rs` | Add `--json` global flag, refactor dispatch to CommandOutput |
| `src/cli/status.rs` | Return CommandResult<StatusData/SetSummaryData> |
| `src/cli/log.rs` | Return CommandResult<LogData> |
| `src/cli/transition.rs` | `cmd_gate_check` returns CommandResult<GateCheckData> |
| `src/cli/manifest_cmd.rs` | `cmd_manifest_verify` returns CommandResult<ManifestVerifyData> |
| `examples/horizons1/protocol.toml` | **New** — mission control protocol config |
| `examples/horizons1/states.toml` | **New** — 9 mission phase states |
| `examples/horizons1/transitions.toml` | **New** — phase transitions with placeholder gates |
| `examples/horizons1/events.toml` | **New** — mission event types |
| `examples/horizons1/renders.toml` | **New** — empty (Phase 5) |
| `tests/json_output_tests.rs` | **New** — envelope/serialization unit tests |
| `tests/horizons1_tests.rs` | **New** — HORIZONS-1 integration tests |
| `CLAUDE.md` | Update test count, add output.rs to module lookup |

## Out of Scope

- `--json` on commands beyond the 6 listed (added as needed in later phases)
- Plugin config sections in protocol.toml (`[gate_plugins]`, `[event_middleware]`, etc.)
- Test data files (telemetry.json, trajectory.json) — Phase 3
- `sahjhan.toml` global config — Phase 2
- Schema version negotiation — Phase 2
