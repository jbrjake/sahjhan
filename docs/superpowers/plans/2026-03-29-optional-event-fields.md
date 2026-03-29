# Optional Event Fields Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `optional = true` support to event field definitions so fields can be omitted without error, while still being validated when provided.

**Architecture:** `EventFieldConfig` gains `optional: bool` (default false). A new shared `validate_event_fields()` function replaces the duplicated validation in `cmd_event` and `cmd_authed_event`. The shared function skips the "missing field" check for optional fields but validates pattern/values when the field IS provided.

**Tech Stack:** Rust, serde/toml deserialization, clap CLI

**Spec:** `docs/superpowers/specs/2026-03-29-optional-event-fields-design.md`

---

### Task 1: Add `optional` field to `EventFieldConfig`

**Files:**
- Modify: `src/config/events.rs:29-36`
- Test: `tests/config_tests.rs`

- [ ] **Step 1: Write the failing test**

Add to `tests/config_tests.rs`:

```rust
#[test]
fn test_event_field_optional_defaults_false() {
    let toml_str = r#"
[events.test_event]
description = "Test"
fields = [
    { name = "required_field", type = "string" },
]
"#;
    let events_file: sahjhan::config::events::EventsFile = toml::from_str(toml_str).unwrap();
    let event = &events_file.events["test_event"];
    assert!(!event.fields[0].optional);
}

#[test]
fn test_event_field_optional_true() {
    let toml_str = r#"
[events.test_event]
description = "Test"
fields = [
    { name = "opt_field", type = "string", optional = true },
]
"#;
    let events_file: sahjhan::config::events::EventsFile = toml::from_str(toml_str).unwrap();
    let event = &events_file.events["test_event"];
    assert!(event.fields[0].optional);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_event_field_optional -- --nocapture`
Expected: Compilation error — `EventFieldConfig` has no field `optional`

- [ ] **Step 3: Add the `optional` field**

In `src/config/events.rs`, add to `EventFieldConfig`:

```rust
#[derive(Debug, Deserialize, Clone)]
pub struct EventFieldConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub pattern: Option<String>,
    pub values: Option<Vec<String>>,
    #[serde(default)]
    pub optional: bool,
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test test_event_field_optional -- --nocapture`
Expected: Both `test_event_field_optional_defaults_false` and `test_event_field_optional_true` PASS

- [ ] **Step 5: Commit**

```bash
git add src/config/events.rs tests/config_tests.rs
git commit -m "feat: add optional field to EventFieldConfig"
```

---

### Task 2: Extract shared `validate_event_fields` function

**Files:**
- Modify: `src/cli/transition.rs:1-12, 404-444`
- Modify: `src/cli/authed_event.rs:126-162`
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Write a test that exercises existing required-field validation**

This test confirms the shared function works — it should pass both before and after the extraction. Add to `tests/integration_tests.rs`:

```rust
#[test]
fn test_event_missing_required_field_rejected() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin"])
        .current_dir(dir.path())
        .assert()
        .success();

    // set_member_complete requires "set" and "member" fields — omit "member"
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "event",
            "set_member_complete",
            "--field",
            "set=check",
        ])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("missing field 'member'"));
}
```

- [ ] **Step 2: Run the test to verify it passes (existing behavior)**

Run: `cargo test test_event_missing_required_field_rejected -- --nocapture`
Expected: PASS

- [ ] **Step 3: Extract the shared function**

In `src/cli/transition.rs`, add this function above the `cmd_event` function (around line 349):

```rust
use crate::config::events::EventConfig;

/// Validate event fields against an event definition.
///
/// Checks that required fields are present, and validates pattern/values
/// constraints on all provided fields (including optional ones).
pub fn validate_event_fields(
    event_config: &EventConfig,
    fields: &HashMap<String, String>,
    event_type: &str,
) -> Result<(), (i32, String)> {
    // Check required fields are present (skip optional fields)
    for field_def in &event_config.fields {
        if !field_def.optional && !fields.contains_key(&field_def.name) {
            return Err((
                EXIT_USAGE_ERROR,
                format!(
                    "error: missing field '{}' for event '{}'",
                    field_def.name, event_type
                ),
            ));
        }
    }

    // Validate provided field values against patterns and allowed values
    for field_def in &event_config.fields {
        if let Some(value) = fields.get(&field_def.name) {
            if let Some(pattern) = &field_def.pattern {
                if let Ok(re) = regex::Regex::new(pattern) {
                    if !re.is_match(value) {
                        return Err((
                            EXIT_USAGE_ERROR,
                            format!(
                                "error: field '{}' value '{}' doesn't match pattern '{}'",
                                field_def.name, value, pattern
                            ),
                        ));
                    }
                }
            }
            if let Some(allowed) = &field_def.values {
                if !allowed.contains(value) {
                    return Err((
                        EXIT_USAGE_ERROR,
                        format!(
                            "error: field '{}' value '{}' not in allowed values {:?}",
                            field_def.name, value, allowed
                        ),
                    ));
                }
            }
        }
    }

    Ok(())
}
```

- [ ] **Step 4: Replace inline validation in `cmd_event`**

In `src/cli/transition.rs`, replace the inline validation block in `cmd_event` (the block starting with `// Validate fields against events.toml definitions (E11)`) with:

```rust
    // Validate fields against events.toml definitions (E11)
    if let Some(event_config) = config.events.get(event_type) {
        if let Err((code, msg)) = validate_event_fields(event_config, &fields, event_type) {
            eprintln!("{}", msg);
            return code;
        }
    }
```

- [ ] **Step 5: Replace inline validation in `cmd_authed_event`**

In `src/cli/authed_event.rs`, add the import:

```rust
use super::transition::validate_event_fields;
```

Then replace the inline validation block (the block starting with `// Validate fields against events.toml definitions`) with:

```rust
    // Validate fields against events.toml definitions
    if let Some(event_config) = config.events.get(event_type) {
        if let Err((code, msg)) = validate_event_fields(event_config, &fields, event_type) {
            eprintln!("{}", msg);
            return code;
        }
    }
```

- [ ] **Step 6: Run tests to verify nothing broke**

Run: `cargo test test_event_missing_required_field_rejected test_event_recording -- --nocapture`
Expected: Both PASS

- [ ] **Step 7: Commit**

```bash
git add src/cli/transition.rs src/cli/authed_event.rs tests/integration_tests.rs
git commit -m "refactor: extract shared validate_event_fields function"
```

---

### Task 3: Add tests for optional field behavior

**Files:**
- Test: `tests/integration_tests.rs`

This task adds the four test cases specified in the issue, using an inline events.toml that declares an optional field.

- [ ] **Step 1: Add a setup helper for optional-field tests**

Add to `tests/integration_tests.rs`:

```rust
/// Create a temp directory with a config that has an optional field, then run `init`.
fn setup_optional_field_dir() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        r#"
[protocol]
name = "test-optional"
version = "1.0.0"
description = "Optional field test"

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
"#,
    )
    .unwrap();

    std::fs::write(
        config_dir.join("transitions.toml"),
        "[[transitions]]\nfrom = \"idle\"\nto = \"idle\"\ncommand = \"noop\"\ngates = []\n",
    )
    .unwrap();

    std::fs::write(
        config_dir.join("events.toml"),
        r#"
[events.finding_resolved]
description = "A finding was resolved"
fields = [
    { name = "id", type = "string", pattern = "^F-\\d{3}$" },
    { name = "commit_hash", type = "string" },
    { name = "evidence_path", type = "string", optional = true, pattern = "^evidence/" },
]
"#,
    )
    .unwrap();

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

- [ ] **Step 2: Write test — all fields provided, all validated**

```rust
#[test]
fn test_optional_field_provided_and_validated() {
    let dir = setup_optional_field_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "event",
            "finding_resolved",
            "--field", "id=F-001",
            "--field", "commit_hash=abc1234",
            "--field", "evidence_path=evidence/justification.md",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("recorded: finding_resolved"));
}
```

- [ ] **Step 3: Write test — optional field omitted, accepted**

```rust
#[test]
fn test_optional_field_omitted_accepted() {
    let dir = setup_optional_field_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "event",
            "finding_resolved",
            "--field", "id=F-002",
            "--field", "commit_hash=def5678",
        ])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("recorded: finding_resolved"));
}
```

- [ ] **Step 4: Write test — optional field provided but failing pattern, rejected**

```rust
#[test]
fn test_optional_field_bad_pattern_rejected() {
    let dir = setup_optional_field_dir();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "event",
            "finding_resolved",
            "--field", "id=F-003",
            "--field", "commit_hash=abc1234",
            "--field", "evidence_path=wrong/path.md",
        ])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("doesn't match pattern"));
}
```

- [ ] **Step 5: Write test — required field still rejected when missing**

```rust
#[test]
fn test_required_field_still_required_with_optional_present() {
    let dir = setup_optional_field_dir();
    // Omit required "id" field — should fail even though optional field exists
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args([
            "--config-dir",
            "enforcement",
            "event",
            "finding_resolved",
            "--field", "commit_hash=abc1234",
        ])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("missing field 'id'"));
}
```

- [ ] **Step 6: Run all four tests**

Run: `cargo test test_optional_field test_required_field_still_required -- --nocapture`
Expected: All FAIL (optional field support not yet wired in — `test_optional_field_omitted_accepted` will fail with "missing field 'evidence_path'")

- [ ] **Step 7: Verify the tests pass now that Task 2 already added the `optional` check**

Since `validate_event_fields` from Task 2 already includes `!field_def.optional` in the required-field check, these tests should actually pass. Run:

Run: `cargo test test_optional_field test_required_field_still_required -- --nocapture`
Expected: All 4 PASS

- [ ] **Step 8: Commit**

```bash
git add tests/integration_tests.rs
git commit -m "test: add integration tests for optional event fields (#12)"
```

---

### Task 4: Update documentation

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update `EventFieldConfig` description in CLAUDE.md**

In the `config/` module lookup table, update the `EventFieldConfig` row:

From:
```
| Event definitions | `config/events.rs` | `EventConfig`, `EventFieldConfig` | events.toml; field patterns for validation; `restricted` marks HMAC-only events |
```

To:
```
| Event definitions | `config/events.rs` | `EventConfig`, `EventFieldConfig` | events.toml; field patterns for validation; `restricted` marks HMAC-only events; `optional` marks fields that can be omitted |
```

- [ ] **Step 2: Add `validate_event_fields` to the CLI lookup table**

In the `cli/` module lookup table, add a new row after the existing `Transition/gate/event` row:

```
| Event field validation | `cli/transition.rs` | `validate_event_fields` | Shared required-field + pattern/values validation for events |
```

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: All tests pass (275+ tests)

Run: `cargo clippy -- -D warnings`
Expected: No warnings

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md for optional event fields and shared validation"
```
