# Terse Output Overhaul

**Date:** 2026-03-28
**Status:** Approved
**Goal:** Minimize tokens emitted by all CLI commands while maximizing agent usability — state clarity, self-documentation, and compliance-aware rejections.

## Principle

Every byte Sahjhan emits must either (1) tell the agent what state they're in, (2) tell them what to do next, or (3) tell them why they can't do something and what the rule actually demands. No decoration, no bars, no blank lines, no ceremony.

## Design Decisions

- **Approach:** Unified terse rewrite (no `--verbose`/`--quiet` flags, no `--json` mode)
- **Primary consumer:** AI agents (personality bleeds through in word choice, not structure)
- **No decoration:** No separator bars, no headers, no blank-line padding
- **One-line successes:** Every successful mutation emits exactly one line
- **Self-documenting status:** `status` shows full decision tree (state + transitions + gate results) in one call
- **Intent-aware rejections:** Gates carry an optional `intent` field; fallback to generated defaults

## Schema Change: GateConfig.intent

Add optional `intent` field to `GateConfig` in `src/config/transitions.rs`:

```rust
#[derive(Debug, Deserialize, Clone)]
pub struct GateConfig {
    #[serde(rename = "type")]
    pub gate_type: String,
    /// Why this gate exists — shown to agents on rejection.
    /// If absent, a default intent is generated from the gate type.
    pub intent: Option<String>,
    #[serde(flatten)]
    pub params: HashMap<String, toml::Value>,
}
```

TOML usage:

```toml
[[gates]]
type = "file_exists"
path = "spec.md"
intent = "review must contain substantive analysis"
```

Backward compatible — existing configs without `intent` continue to work.

## Default Intents

A function `default_intent(gate_type: &str) -> &str` in `src/gates/evaluator.rs`:

| Gate type | Default intent |
|-----------|---------------|
| `file_exists` / `files_exist` | "required files must exist before proceeding" |
| `command_succeeds` | "command must pass before proceeding" |
| `command_output` | "command output must match expected value" |
| `ledger_has_event` | "required events must be recorded first" |
| `ledger_has_event_since` | "required events must occur since last transition" |
| `set_covered` | "all set members must be completed" |
| `min_elapsed` | "minimum time must elapse before proceeding" |
| `no_violations` | "all protocol violations must be resolved" |
| `field_not_empty` | "required field must have a value" |
| `snapshot_compare` | "snapshot must match expected state" |
| `query` | "query condition must be satisfied" |
| (unknown) | "gate condition must be met" |

## Output Formats by Command

### status

```
state: reviewing (5 events, chain valid)
sets:
  perspectives: 2/4 [✓ security, ✓ perf, · scale, · maint]
next:
  approve: blocked
    ✗ file_exists: review.md — review must contain substantive analysis
    ✓ set_covered: perspectives
  reject: ready
```

- Line 1: state name, event count, chain status
- Sets: one line each, inline member markers
- Transitions: from current state only, with full gate results
- `ready` = all gates pass (or no gates). `blocked` = at least one fails.

Event-only ledger variant:

```
event-only: 12 events, chain valid
```

### transition (success)

```
drafting → reviewing (2 rendered)
```

One line. Render count only if >0. No rendered file list.

### transition (gate blocked)

```
✗ file_exists: spec.md missing — review must contain substantive analysis
```

One line to stderr. Exit code 1.

### transition (no such transition)

```
error: no transition 'submit' from state 'idle'
```

One line to stderr. Exit code 4.

### gate-check (dry-run)

```
gate-check: submit_review
  ✓ ledger_has_event: draft_complete
  ✗ file_exists: spec.md — review must contain substantive analysis
result: blocked
```

Same gate format as `status`. Final line is `result: ready` or `result: blocked`. No gates = `result: ready (no gates)`.

### event

```
recorded: review_note (1 rendered)
```

One line. Render count only if >0.

### set complete

```
set perspectives: security done (1/4, 1 rendered)
```

One line. Render count only if >0.

### set status

```
perspectives: 2/4 [✓ security, ✓ perf, · scale, · maint]
```

One line, same format as sets in `status`.

### init

```
initialized. good luck.
```

### validate

```
valid.
```

On failure, errors to stderr:

```
error: unknown gate type 'foo' in transition 'submit'
error: state 'reviewing' referenced but not defined
```

Warnings:

```
warning: render template 'status.md.tera' not found
```

### reset

Token prompt:

```
reset requires --token abc123
```

Success:

```
reset. prior run archived.
```

### ledger create

```
created: run-25
```

### ledger list

```
default (stateful) ledger.jsonl
run-25 (stateful) runs/25/ledger.jsonl
```

No header line. One line per entry. No timestamps (agent doesn't need them).

### ledger remove

```
removed: run-25 (file kept)
```

### ledger verify

```
chain valid (5 entries)
```

Failure:

```
error: chain invalid at seq 3 — tampering detected
```

### ledger checkpoint

```
checkpoint: seq 5 scope=full
```

### ledger import

```
imported: audit-log
```

### manifest verify

Clean:

```
manifest clean (3 tracked)
```

Failure:

```
manifest: 2 modified
  spec.md — expected a1b2c3, got d4e5f6
  review.md — missing
```

### manifest list

```
a1b2c3 spec.md (render)
d4e5f6 ledger.jsonl (ledger_append)
```

No header. One line per file.

### manifest restore

```
restore: re-render spec.md (last tracked seq 3)
```

or:

```
restore: git checkout -- spec.md
```

### Error format (all commands)

```
error: {what}: {detail}
```

To stderr. No personality in errors — clarity only.

## Unchanged Commands

These already emit appropriate output:

- `log dump` / `log tail` — structured ledger entries, already terse
- `query` — has table/json/jsonl/csv modes, already format-aware
- `render --dump-context` — JSON debug tool, verbosity is the point
- `hooks generate` — outputs script content, verbosity inherent

## Implementation Scope

### Files to modify

| File | Changes |
|------|---------|
| `src/config/transitions.rs` | Add `intent: Option<String>` to `GateConfig` |
| `src/gates/evaluator.rs` | Add `default_intent()`, add `intent` field to `GateResult` |
| `src/gates/types.rs` | Pass intent through from `GateConfig` to `GateResult` |
| `src/cli/status.rs` | Rewrite `cmd_status`, `cmd_set_status`, `cmd_set_complete` output |
| `src/cli/transition.rs` | Rewrite `cmd_transition`, `cmd_gate_check`, `cmd_event` output |
| `src/cli/init.rs` | Rewrite `cmd_init`, `cmd_validate`, `cmd_reset` output |
| `src/cli/ledger.rs` | Rewrite all ledger subcommand output |
| `src/cli/manifest_cmd.rs` | Rewrite manifest subcommand output |
| `src/cli/render.rs` | Rewrite render success output |

### Files unchanged

| File | Reason |
|------|--------|
| `src/cli/log.rs` | Already terse structured output |
| `src/cli/query.rs` | Already format-aware |
| `src/cli/hooks_cmd.rs` | Outputs script content |
| `src/gates/command.rs` | Internal gate logic, no user-facing output |
| `src/gates/file.rs` | Internal gate logic |
| `src/gates/ledger.rs` | Internal gate logic |
| `src/gates/query.rs` | Internal gate logic |
| `src/gates/snapshot.rs` | Internal gate logic |
| `src/gates/template.rs` | Internal gate logic |
| `src/state/machine.rs` | No user-facing output |
| `src/render/engine.rs` | No user-facing output |

### Test impact

Integration tests in `tests/integration_tests.rs` that assert on CLI output strings will need updating. Gate tests that check `GateResult` fields need updating for the new `intent` field. No logic changes — only output format changes.

## Validation plan

Deep-validation (`cmd_validate`) must still check that `intent` fields, when present, are strings. The `#[serde(flatten)]` captures unknown fields into `params` — since `intent` is now an explicit field, it will be deserialized directly and not leak into `params`.
