# Hooks, Guards, and Monitors — Design Spec

**Issue:** #19
**Date:** 2026-03-31

## Problem

Sahjhan's current enforcement is static: generated hook scripts bake in managed paths at generation time, guards only block reads, and there's no mechanism for state-aware runtime enforcement. An agent can edit source files without a failing test, claim completion in non-terminal states, accumulate edits without committing, and stall in a state indefinitely — all without Sahjhan intervening.

## Architecture Decision

**Runtime evaluation command.** Rather than generating logic-heavy scripts, all enforcement intelligence lives in the Rust binary. A single CLI command (`sahjhan hook eval`) evaluates hooks, guards, and monitors against live ledger state. Generated scripts become thin wrappers that delegate to this command.

Monitors piggyback on `hook eval` calls rather than requiring a separate command or timer.

## Config: `hooks.toml`

New optional config file alongside the existing five. Contains `[[hooks]]` and `[[monitors]]`.

### Hooks

```toml
[[hooks]]
event = "PreToolUse"           # Required: PreToolUse | PostToolUse | Stop
tools = ["Edit", "Write"]      # Optional: tool filter (all tools if omitted)
states = ["fix_loop"]          # Optional: only active in these states
states_not = ["finalized"]     # Optional: inactive in these states
action = "block"               # Required (unless auto_record): "block" or "warn"
message = "Explain why..."     # Required (unless auto_record): supports {current_state} interpolation

# Exactly one of: gate, check, auto_record

# --- Option A: gate (reuses existing gate evaluation system) ---
[hooks.gate]
type = "ledger_has_event_since"
event = "failing_test"
since = "fix_commit"

# --- Option B: check (threshold or pattern evaluation) ---
[hooks.check]
type = "query"                 # "query" | "output_contains_any" | "event_count_since_last_transition"
sql = "SELECT count(*) as cnt FROM events WHERE ..."
compare = "gte"                # gte | lte | eq | gt | lt
threshold = 3

# --- Option C: auto_record (PostToolUse only, no action/message) ---
[hooks.auto_record]
event_type = "source_edit"
fields = { file_path = "{tool.file_path}" }

# Optional: filter on tool arguments
[hooks.filter]
path_matches = "src/**"        # glob the file path MUST match
path_not_matches = "tests/*"   # glob the file path must NOT match
```

**Hook types by condition mechanism:**

| Mechanism | Use case | Fields |
|-----------|----------|--------|
| `gate` | Reuse any existing gate type as a precondition | Full gate config (type + params) |
| `check` | Threshold/pattern checks not expressible as gates | `type`, plus type-specific fields (see below) |
| `auto_record` | Automatically append events to the ledger | `event_type`, `fields` with `{tool.*}` interpolation |

**Check types:**

| Type | Fields | Behavior |
|------|--------|----------|
| `query` | `sql`, `compare` (gte/lte/eq/gt/lt), `threshold` | Run SQL via DataFusion, compare first numeric result to threshold |
| `output_contains_any` | `patterns` (string array) | Pass if agent output contains any pattern (Stop hooks only) |
| `event_count_since_last_transition` | `threshold` | Count all events since last `state_transition`, compare to threshold |

### Monitors

Evaluated on every `hook eval` call. Monitors only warn, never block.

```toml
[[monitors]]
name = "stall_detector"        # Required: unique name
states = ["fix_loop"]          # Optional: state filter
action = "warn"                # Required: must be "warn"
message = "20 events since last state transition."

[monitors.trigger]
type = "event_count_since_last_transition"
threshold = 20
```

## Extended Guards in `protocol.toml`

The existing `[guards]` section gains `[[guards.write_gated]]` for state-conditional write protection:

```toml
[guards]
read_blocked = [".sahjhan/session.key"]

[[guards.write_gated]]
path = "docs/SUMMARY.md"          # Supports globs
writable_in = ["finalized"]       # Whitelist: blocked in all other states
message = "SUMMARY.md can only be written in state: finalized. Current state: {current_state}."
```

- Distinct from `paths.managed` (always-blocked). `write_gated` files are writable, but only in specified states.
- Checked during `hook eval` for PreToolUse Edit/Write.

## CLI: `sahjhan hook eval`

Single entry point for all runtime enforcement.

```
sahjhan hook eval \
  --event PreToolUse|PostToolUse|Stop \
  [--tool Edit|Write|Bash|...] \
  [--file path/to/file.rs] \
  [--output-text "agent's final message..."]
```

- `--event` required
- `--tool` required for PreToolUse/PostToolUse, ignored for Stop
- `--file` optional, for path filter matching on Edit/Write
- `--output-text` for Stop hooks that pattern-match agent output

**Output** (always JSON):

```json
{
  "decision": "block",
  "messages": [
    {
      "source": "hook",
      "rule_index": 0,
      "action": "block",
      "message": "TDD violation: write a failing test first."
    }
  ],
  "auto_records": [
    {
      "event_type": "source_edit",
      "fields": { "file_path": "src/main.rs" }
    }
  ],
  "monitor_warnings": [
    {
      "name": "stall_detector",
      "message": "20 events since last state transition."
    }
  ]
}
```

- `decision`: most restrictive result (block > warn > allow)
- If no hooks.toml exists: `{"decision": "allow", "messages": [], "auto_records": [], "monitor_warnings": []}`

**Evaluation order:**
1. State-gated write guards (`guards.write_gated`)
2. Hooks matching event/tool/state/filter
3. Auto-record hooks (side effect: appends to ledger)
4. Monitors

## Gate Enhancement: `ledger_has_event_since`

The `since` parameter is now **required** (breaking change).

```toml
# Since last state transition (previously implicit default):
type = "ledger_has_event_since"
event = "failing_test"
since = "last_transition"

# Since last event of a specific type:
type = "ledger_has_event_since"
event = "failing_test"
since = "fix_commit"
```

- `"last_transition"` — since most recent `state_transition` event
- Any other value — interpreted as an event type name; uses most recent event of that type as boundary, falls back to last transition if none exists
- Validated during `validate_deep`: non-keyword values must be defined event types
- Existing configs using `ledger_has_event_since` without `since` fail validation with a clear migration message

## Hook Generator Updates

Generated scripts become thin wrappers delegating to `sahjhan hook eval`:

| Script | Event | Replaces |
|--------|-------|----------|
| `pre_tool_hook.py` | PreToolUse | `write_guard.py` |
| `post_tool_hook.py` | PostToolUse | `bash_guard.py` |
| `stop_hook.py` | Stop | (new) |
| `_sahjhan_bootstrap.py` | PreToolUse | stays as-is |

- Static managed path checks (`paths.managed`) move into `hook eval`
- `_sahjhan_bootstrap.py` remains self-contained (can't delegate to the system it protects)
- `suggested_hooks_json()` updated for new script names and Stop hook entry
- Config changes take effect immediately without regenerating scripts

## Config Integration

**Loading:** `hooks.toml` is optional. When absent, `hook eval` returns allow.

**Validation (`validate_deep`):**
- Hook `event` ∈ {PreToolUse, PostToolUse, Stop}
- Hook `action` ∈ {block, warn} (not required for auto_record)
- Hook `gate` validated through existing recursive gate validator
- Hook `check.type` must be a known check type
- `auto_record` requires `event = "PostToolUse"`
- `auto_record.event_type` must be a defined event type
- `since` required on all `ledger_has_event_since` gates
- `states` / `states_not` reference existing states
- `guards.write_gated` `writable_in` references existing states
- Monitor names are unique
- Monitor `action` must be `warn`

**Config seals:** `hooks.toml` added as 6th hash in `compute_config_seals()`. Absent file hashes as empty string. No legacy ledger support needed.
