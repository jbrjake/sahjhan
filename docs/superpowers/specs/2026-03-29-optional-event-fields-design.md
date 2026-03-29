# Optional Event Fields — Design Spec

**Issue:** #12
**Date:** 2026-03-29

## Summary

Add `optional = true` support to event field definitions in `events.toml`. Optional fields can be omitted from `--field` arguments without error. If provided, they are still validated against `type`, `pattern`, and `values` constraints. Default is `optional = false` (backward compatible).

## Changes

### 1. `EventFieldConfig` — add `optional` field

**File:** `src/config/events.rs`

Add `#[serde(default)] pub optional: bool` to `EventFieldConfig`. Serde `default` gives `false` when omitted from TOML, preserving current behavior for all existing event definitions.

### 2. Extract shared validation function

**File:** `src/cli/transition.rs` (new public function)

`cmd_event` (transition.rs:404-444) and `cmd_authed_event` (authed_event.rs:126-162) have identical field validation logic. Extract into:

```rust
pub fn validate_event_fields(
    event_config: &EventConfig,
    fields: &HashMap<String, String>,
    event_type: &str,
) -> Result<(), (i32, String)>
```

Logic:
- For each field definition in `event_config.fields`:
  - If `!optional && !fields.contains_key(name)` -> error (exit code 4, "missing field")
  - If field IS present:
    - Validate against `pattern` (if defined)
    - Validate against `values` (if defined)
- Returns `Ok(())` on success, `Err((exit_code, message))` on failure

Both `cmd_event` and `cmd_authed_event` replace their inline validation with a call to this function.

### 3. No changes to `validate_deep`

`validate_deep` in `config/mod.rs` only checks field types (`string`, `number`, `boolean`). The `optional` flag doesn't affect type validation, so no change needed.

## TOML syntax

```toml
[events.finding_resolved]
description = "A finding was resolved"
fields = [
    { name = "id", type = "string", pattern = "^B[HJ]-\\d{3}$" },
    { name = "commit_hash", type = "string", pattern = "^[0-9a-f]{7,40}$" },
    { name = "evidence_path", type = "string", optional = true },
]
```

## Test cases

1. **All fields provided (including optional):** Accepted, all fields validated against patterns/values
2. **Optional field omitted:** Accepted, no error
3. **Optional field provided but failing pattern:** Rejected with pattern error
4. **Existing events with no optional fields:** Behavior unchanged (backward compatible)

## Files touched

| File | Change |
|------|--------|
| `src/config/events.rs` | Add `optional: bool` to `EventFieldConfig` |
| `src/cli/transition.rs` | Extract `validate_event_fields()`, use in `cmd_event` |
| `src/cli/authed_event.rs` | Replace inline validation with `validate_event_fields()` call |
| `tests/integration_tests.rs` | Add test cases for optional field behavior |
| `CLAUDE.md` | Update `EventFieldConfig` description in events.rs index entry |
