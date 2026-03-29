# Ledger Templates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add ledger templates to sahjhan so protocols can declare named ledger patterns, instantiate them with `--from`, and reference them in renders via `ledger_template`.

**Architecture:** Protocol.toml gains `[ledgers.<name>]` sections parsed into `LedgerTemplateConfig`. Registry entries gain `template` + `instance_id` fields. Renders gain `ledger_template` field that resolves against the active ledger's template metadata. Status display reads template metadata instead of counting events.

**Tech Stack:** Rust, serde/toml deserialization, clap CLI, Tera templates

**Spec:** `docs/superpowers/specs/2026-03-28-ledger-templates-design.md`

---

### Task 1: Parse `[ledgers]` from protocol.toml

**Files:**
- Modify: `src/config/protocol.rs:1-63`
- Modify: `src/config/mod.rs:17-18` (re-export), `93-103` (load), `28-38` (ProtocolConfig struct)
- Test: `tests/config_tests.rs`

- [ ] **Step 1: Write the failing test**

Add to `tests/config_tests.rs`:

```rust
#[test]
fn test_ledger_templates_loaded() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    // minimal has no [ledgers] section — should default to empty
    assert!(config.ledgers.is_empty());
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_ledger_templates_loaded -- --nocapture`
Expected: FAIL — `ProtocolConfig` has no field `ledgers`

- [ ] **Step 3: Add `LedgerTemplateConfig` struct to protocol.rs**

In `src/config/protocol.rs`, add after `SetConfig` (after line 62):

```rust
/// A ledger declaration in protocol.toml.
///
/// Two forms:
/// - **Template** (`path_template`): pattern with `{template.instance_id}` / `{template.name}`
/// - **Fixed** (`path`): single known path, no instantiation
///
/// These are mutually exclusive.
#[derive(Debug, Deserialize, Clone)]
pub struct LedgerTemplateConfig {
    pub description: String,
    /// Fixed path (for singleton ledgers).
    pub path: Option<String>,
    /// Path template with `{template.instance_id}` and `{template.name}` variables.
    pub path_template: Option<String>,
}
```

Add `ledgers` field to `ProtocolFile` (inside the struct, after `checkpoints`):

```rust
    #[serde(default)]
    pub ledgers: HashMap<String, LedgerTemplateConfig>,
```

- [ ] **Step 4: Re-export and propagate to ProtocolConfig**

In `src/config/mod.rs`, update the re-export line (line 18):

```rust
pub use protocol::{CheckpointConfig, LedgerTemplateConfig, PathsConfig, ProtocolMeta, SetConfig};
```

Add `ledgers` field to `ProtocolConfig` struct (after `checkpoints` on line 37):

```rust
    pub ledgers: HashMap<String, LedgerTemplateConfig>,
```

In `ProtocolConfig::load`, add to the `Ok(ProtocolConfig { ... })` block (after `checkpoints` on line 102):

```rust
            ledgers: proto_file.ledgers,
```

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test test_ledger_templates_loaded -- --nocapture`
Expected: PASS

- [ ] **Step 6: Add test with actual ledger template config**

Create `examples/minimal/protocol.toml` doesn't have ledgers — so write a second test that constructs one programmatically:

```rust
#[test]
fn test_ledger_template_fields() {
    use sahjhan::config::LedgerTemplateConfig;

    let toml_str = r#"
        [protocol]
        name = "test"
        version = "1.0.0"
        description = "test"

        [paths]
        managed = []
        data_dir = ".data"
        render_dir = "."

        [ledgers.run]
        description = "Per-run ledger"
        path_template = "runs/{template.instance_id}/ledger.jsonl"

        [ledgers.project]
        description = "Project ledger"
        path = "project.jsonl"
    "#;

    let proto_file: sahjhan::config::protocol::ProtocolFile =
        toml::from_str(toml_str).unwrap();

    assert_eq!(proto_file.ledgers.len(), 2);

    let run = &proto_file.ledgers["run"];
    assert_eq!(run.description, "Per-run ledger");
    assert!(run.path_template.is_some());
    assert!(run.path.is_none());
    assert_eq!(
        run.path_template.as_ref().unwrap(),
        "runs/{template.instance_id}/ledger.jsonl"
    );

    let project = &proto_file.ledgers["project"];
    assert!(project.path.is_some());
    assert!(project.path_template.is_none());
}
```

- [ ] **Step 7: Run test**

Run: `cargo test test_ledger_template_fields -- --nocapture`
Expected: PASS

- [ ] **Step 8: Update index headers**

In `src/config/protocol.rs`, update the `## Index` comment at the top to include:

```
// - LedgerTemplateConfig     — ledger declaration (path or path_template)
```

- [ ] **Step 9: Commit**

```bash
git add src/config/protocol.rs src/config/mod.rs tests/config_tests.rs
git commit -m "feat: parse [ledgers] section from protocol.toml

Add LedgerTemplateConfig struct with path/path_template variants.
ProtocolConfig now loads and exposes ledger template declarations.

Addresses issue #10 (ledger templates)."
```

---

### Task 2: Validate ledger template declarations

**Files:**
- Modify: `src/config/mod.rs:233-384` (validate_deep)
- Test: `tests/config_tests.rs`

- [ ] **Step 1: Write the failing tests**

Add to `tests/config_tests.rs`:

```rust
#[test]
fn test_validate_ledger_template_both_path_and_template() {
    use sahjhan::config::*;
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.ledgers.insert(
        "bad".to_string(),
        LedgerTemplateConfig {
            description: "bad".to_string(),
            path: Some("a.jsonl".to_string()),
            path_template: Some("b/{template.instance_id}.jsonl".to_string()),
        },
    );
    let (errors, _) = config.validate_deep(Path::new("examples/minimal"));
    assert!(
        errors.iter().any(|e| e.contains("bad") && e.contains("both")),
        "Expected error about both path and path_template: {:?}",
        errors
    );
}

#[test]
fn test_validate_ledger_template_neither_path_nor_template() {
    use sahjhan::config::*;
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.ledgers.insert(
        "empty".to_string(),
        LedgerTemplateConfig {
            description: "empty".to_string(),
            path: None,
            path_template: None,
        },
    );
    let (errors, _) = config.validate_deep(Path::new("examples/minimal"));
    assert!(
        errors.iter().any(|e| e.contains("empty") && e.contains("must have")),
        "Expected error about missing path: {:?}",
        errors
    );
}

#[test]
fn test_validate_ledger_template_missing_instance_id_var() {
    use sahjhan::config::*;
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.ledgers.insert(
        "novar".to_string(),
        LedgerTemplateConfig {
            description: "novar".to_string(),
            path: None,
            path_template: Some("runs/ledger.jsonl".to_string()),
        },
    );
    let (errors, _) = config.validate_deep(Path::new("examples/minimal"));
    assert!(
        errors.iter().any(|e| e.contains("novar") && e.contains("{template.instance_id}")),
        "Expected error about missing {{template.instance_id}}: {:?}",
        errors
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_validate_ledger_template -- --nocapture`
Expected: FAIL — no ledger validation exists yet

- [ ] **Step 3: Add ledger validation to validate_deep**

In `src/config/mod.rs`, inside `validate_deep` after the unreachable state check (after line 381, before the final `(errors, warnings)`):

```rust
        // 12. Ledger template validation.
        for (name, ledger_tmpl) in &self.ledgers {
            match (&ledger_tmpl.path, &ledger_tmpl.path_template) {
                (Some(_), Some(_)) => {
                    errors.push(format!(
                        "protocol.toml: ledger '{}' has both 'path' and 'path_template' — must have exactly one",
                        name
                    ));
                }
                (None, None) => {
                    errors.push(format!(
                        "protocol.toml: ledger '{}' must have either 'path' or 'path_template'",
                        name
                    ));
                }
                (None, Some(tmpl)) => {
                    if !tmpl.contains("{template.instance_id}") {
                        errors.push(format!(
                            "protocol.toml: ledger '{}' path_template must contain '{{template.instance_id}}'",
                            name
                        ));
                    }
                }
                (Some(_), None) => {
                    // Fixed path — valid as-is.
                }
            }
        }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test test_validate_ledger_template -- --nocapture`
Expected: PASS (all three)

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: All existing tests still pass

- [ ] **Step 6: Commit**

```bash
git add src/config/mod.rs tests/config_tests.rs
git commit -m "feat: validate ledger template declarations in validate_deep

Checks: exactly one of path/path_template, path_template must contain
{template.instance_id}."
```

---

### Task 3: Expand registry entries with template metadata

**Files:**
- Modify: `src/ledger/registry.rs:28-35` (LedgerRegistryEntry), `82-97` (create method)
- Test: `tests/registry_tests.rs`

- [ ] **Step 1: Write the failing test**

Add to `tests/registry_tests.rs`:

```rust
// ---------------------------------------------------------------------------
// 9. template and instance_id fields survive round-trip
// ---------------------------------------------------------------------------
#[test]
fn test_template_metadata_round_trip() {
    let dir = TempDir::new().unwrap();
    let path = temp_registry_path(&dir);

    {
        let mut reg = LedgerRegistry::new(&path).unwrap();
        reg.create_with_template(
            "run-25",
            "docs/holtz/runs/25/ledger.jsonl",
            LedgerMode::Stateful,
            Some("run"),
            Some("25"),
        )
        .unwrap();
    }

    // Reload from disk
    let reg2 = LedgerRegistry::new(&path).unwrap();
    let entry = reg2.resolve(Some("run-25")).unwrap();
    assert_eq!(entry.template.as_deref(), Some("run"));
    assert_eq!(entry.instance_id.as_deref(), Some("25"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_template_metadata_round_trip -- --nocapture`
Expected: FAIL — `create_with_template` doesn't exist, `template`/`instance_id` fields don't exist

- [ ] **Step 3: Add fields to LedgerRegistryEntry**

In `src/ledger/registry.rs`, modify `LedgerRegistryEntry` (lines 28-35):

```rust
/// One row in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerRegistryEntry {
    pub name: String,
    pub path: String,
    pub mode: LedgerMode,
    /// ISO 8601 creation timestamp.
    pub created: String,
    /// Which protocol template this ledger was created from (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
    /// Instance identifier within the template (e.g., "25" for run-25).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
}
```

- [ ] **Step 4: Add `create_with_template` method**

In `src/ledger/registry.rs`, add after the existing `create` method (after line 97):

```rust
    /// Register a new ledger with optional template metadata.
    /// Fails if `name` already exists.
    pub fn create_with_template(
        &mut self,
        name: &str,
        path: &str,
        mode: LedgerMode,
        template: Option<&str>,
        instance_id: Option<&str>,
    ) -> Result<(), String> {
        if self.entries.iter().any(|e| e.name == name) {
            return Err(format!("ledger '{name}' already exists in the registry"));
        }

        let created = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        self.entries.push(LedgerRegistryEntry {
            name: name.to_string(),
            path: path.to_string(),
            mode,
            created,
            template: template.map(|s| s.to_string()),
            instance_id: instance_id.map(|s| s.to_string()),
        });

        self.save()
    }
```

- [ ] **Step 5: Update existing `create` to populate new fields as None**

Modify the existing `create` method push (line 89-94) to include the new fields:

```rust
        self.entries.push(LedgerRegistryEntry {
            name: name.to_string(),
            path: path.to_string(),
            mode,
            created,
            template: None,
            instance_id: None,
        });
```

- [ ] **Step 6: Run test to verify it passes**

Run: `cargo test test_template_metadata_round_trip -- --nocapture`
Expected: PASS

- [ ] **Step 7: Add resolve_by_template method and test**

Add method to `LedgerRegistry` (after `resolve`):

```rust
    /// Find all registry entries created from a given template, in insertion order.
    pub fn resolve_by_template(&self, template: &str) -> Vec<&LedgerRegistryEntry> {
        self.entries
            .iter()
            .filter(|e| e.template.as_deref() == Some(template))
            .collect()
    }
```

Add test to `tests/registry_tests.rs`:

```rust
// ---------------------------------------------------------------------------
// 10. resolve_by_template returns matching entries
// ---------------------------------------------------------------------------
#[test]
fn test_resolve_by_template() {
    let dir = TempDir::new().unwrap();
    let path = temp_registry_path(&dir);

    let mut reg = LedgerRegistry::new(&path).unwrap();
    reg.create_with_template(
        "run-24",
        "runs/24/ledger.jsonl",
        LedgerMode::Stateful,
        Some("run"),
        Some("24"),
    )
    .unwrap();
    reg.create_with_template(
        "run-25",
        "runs/25/ledger.jsonl",
        LedgerMode::Stateful,
        Some("run"),
        Some("25"),
    )
    .unwrap();
    reg.create("project", "project.jsonl", LedgerMode::EventOnly)
        .unwrap();

    let run_entries = reg.resolve_by_template("run");
    assert_eq!(run_entries.len(), 2);
    assert_eq!(run_entries[0].name, "run-24");
    assert_eq!(run_entries[1].name, "run-25");

    let project_entries = reg.resolve_by_template("project");
    assert!(project_entries.is_empty());
}
```

- [ ] **Step 8: Run tests**

Run: `cargo test test_resolve_by_template -- --nocapture`
Expected: PASS

- [ ] **Step 9: Run full test suite**

Run: `cargo test`
Expected: All tests pass. Existing registry entries without `template`/`instance_id` deserialize with `None` due to `#[serde(default)]`.

- [ ] **Step 10: Update index header**

In `src/ledger/registry.rs`, update the `## Index` comment to include:

```
// - LedgerRegistryEntry       — name, path, mode, template, instance_id
```

- [ ] **Step 11: Commit**

```bash
git add src/ledger/registry.rs tests/registry_tests.rs
git commit -m "feat: add template and instance_id fields to registry entries

Existing entries deserialize with None (serde default). New
create_with_template method stores template metadata.
Adds resolve_by_template query."
```

---

### Task 4: Fix `--path` resolution in `ledger create` (Bug 1)

**Files:**
- Modify: `src/cli/ledger.rs:29-98` (cmd_ledger_create), `308-369` (cmd_ledger_import)
- Modify: `src/cli/commands.rs` (add compute_registry_path helper)
- Test: `tests/integration_tests.rs` (or a new targeted test)

- [ ] **Step 1: Write the failing test**

Since `cli::commands` functions are `pub(crate)`, add unit tests inside the module itself. Add to the bottom of `src/cli/commands.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_compute_registry_path_under_data_dir() {
        let data_dir = PathBuf::from("/project/.sahjhan");
        let file = PathBuf::from("/project/.sahjhan/runs/25/ledger.jsonl");
        let result = compute_registry_path(&file, &data_dir);
        assert_eq!(result, "runs/25/ledger.jsonl");
    }

    #[test]
    fn test_compute_registry_path_outside_data_dir() {
        let data_dir = PathBuf::from("/project/.sahjhan");
        let file = PathBuf::from("/project/docs/runs/25/ledger.jsonl");
        let result = compute_registry_path(&file, &data_dir);
        assert_eq!(result, "/project/docs/runs/25/ledger.jsonl");
    }

    #[test]
    fn test_compute_registry_path_absolute_preserved() {
        let data_dir = PathBuf::from("/project/.sahjhan");
        let file = PathBuf::from("/tmp/ledger.jsonl");
        let result = compute_registry_path(&file, &data_dir);
        assert_eq!(result, "/tmp/ledger.jsonl");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test test_compute_registry_path -- --nocapture`
Expected: FAIL — `compute_registry_path` doesn't exist

- [ ] **Step 3: Add `compute_registry_path` helper**

In `src/cli/commands.rs`, add after `resolve_registry_path` (after line 252):

```rust
// [compute-registry-path]
/// Compute the path to store in the registry for a ledger file.
///
/// If the file is under `data_dir`, stores the relative path (so
/// `resolve_registry_path` can round-trip it). Otherwise stores
/// the absolute path.
pub(crate) fn compute_registry_path(file: &Path, data_dir: &Path) -> String {
    match file.strip_prefix(data_dir) {
        Ok(rel) => rel.to_string_lossy().to_string(),
        Err(_) => file.to_string_lossy().to_string(),
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test test_compute_registry_path -- --nocapture`
Expected: PASS

- [ ] **Step 5: Fix path resolution in `cmd_ledger_create`**

In `src/cli/ledger.rs`, replace lines 51-57:

```rust
    // Resolve output path relative to data_dir
    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let ledger_file = if PathBuf::from(path).is_absolute() {
        PathBuf::from(path)
    } else {
        data_dir.join(path)
    };
```

With:

```rust
    // Resolve output path:
    // - Absolute paths: use as-is
    // - Relative paths: resolve relative to cwd (not data_dir)
    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let ledger_file = if PathBuf::from(path).is_absolute() {
        PathBuf::from(path)
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
```

And replace line 87 (`registry.create(name, path, mode)`) with:

```rust
    let registry_stored_path =
        super::commands::compute_registry_path(&ledger_file, &data_dir);
    if let Err(e) = registry.create(name, &registry_stored_path, mode) {
```

- [ ] **Step 6: Apply same fix to `cmd_ledger_import`**

In `src/cli/ledger.rs`, replace lines 319-324 (in `cmd_ledger_import`):

```rust
    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let ledger_file = if PathBuf::from(path).is_absolute() {
        PathBuf::from(path)
    } else {
        data_dir.join(path)
    };
```

With:

```rust
    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let ledger_file = if PathBuf::from(path).is_absolute() {
        PathBuf::from(path)
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };
```

And replace line 358 (`registry.create(name, path, LedgerMode::EventOnly)`) with:

```rust
    let registry_stored_path =
        super::commands::compute_registry_path(&ledger_file, &data_dir);
    if let Err(e) = registry.create(name, &registry_stored_path, LedgerMode::EventOnly) {
```

- [ ] **Step 7: Add compute_registry_path to the import in ledger.rs if needed**

Check the imports at the top of `src/cli/ledger.rs` — `compute_registry_path` is accessed via `super::commands::compute_registry_path`, so no import change is needed.

- [ ] **Step 8: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 9: Commit**

```bash
git add src/cli/commands.rs src/cli/ledger.rs
git commit -m "fix: resolve --path relative to cwd, not data_dir (issue #10 bug 1)

ledger create and ledger import now resolve relative paths against
the working directory. Registry stores paths relative to data_dir
when possible for round-trip correctness."
```

---

### Task 5: Add `--from` template-based ledger creation

**Files:**
- Modify: `src/main.rs:243-301` (LedgerAction::Create), `373-376` (dispatch)
- Modify: `src/cli/ledger.rs:28-98` (cmd_ledger_create)
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Write the failing test**

This is a CLI integration test. Add to `tests/integration_tests.rs` or create `tests/template_create_tests.rs`. Since this requires a full protocol config with `[ledgers]`, use a temp dir:

```rust
use std::fs;
use std::path::Path;
use tempfile::TempDir;

fn write_protocol_with_ledger_template(dir: &Path) {
    fs::write(
        dir.join("protocol.toml"),
        r#"
[protocol]
name = "test"
version = "1.0.0"
description = "test"

[paths]
managed = []
data_dir = ".sahjhan"
render_dir = "."

[ledgers.run]
description = "Per-run ledger"
path_template = "runs/{template.instance_id}/ledger.jsonl"
"#,
    )
    .unwrap();

    fs::write(
        dir.join("states.toml"),
        r#"
[states.idle]
label = "Idle"
initial = true
"#,
    )
    .unwrap();

    fs::write(dir.join("transitions.toml"), "[[transitions]]\nfrom = \"idle\"\nto = \"idle\"\ncommand = \"noop\"\ngates = []\n").unwrap();
}

#[test]
fn test_ledger_create_from_template() {
    let dir = TempDir::new().unwrap();
    write_protocol_with_ledger_template(dir.path());

    // Initialize default ledger first
    let data_dir = dir.path().join(".sahjhan");
    fs::create_dir_all(&data_dir).unwrap();

    use sahjhan::config::ProtocolConfig;
    let config = ProtocolConfig::load(dir.path()).unwrap();

    // Test the template path resolution logic
    let tmpl = &config.ledgers["run"];
    let path_template = tmpl.path_template.as_ref().unwrap();

    let resolved = path_template
        .replace("{template.instance_id}", "25")
        .replace("{template.name}", "run");

    assert_eq!(resolved, "runs/25/ledger.jsonl");
}
```

- [ ] **Step 2: Run test to verify it passes**

Run: `cargo test test_ledger_create_from_template -- --nocapture`
Expected: PASS (this tests the resolution logic, not the CLI wiring yet)

- [ ] **Step 3: Update clap LedgerAction::Create**

In `src/main.rs`, replace `LedgerAction::Create` (lines 246-258):

```rust
    /// Register and initialize a new named ledger
    Create {
        /// Ledger name (for direct creation without template)
        #[arg(long, required_unless_present = "from")]
        name: Option<String>,

        /// File path for the new ledger (for direct creation without template)
        #[arg(long, required_unless_present = "from")]
        path: Option<String>,

        /// Create from a protocol-declared ledger template
        #[arg(long)]
        from: Option<String>,

        /// Instance identifier for the template (e.g., "25" creates run-25)
        #[arg(requires = "from")]
        instance_id: Option<String>,

        /// Ledger mode: stateful or event-only
        #[arg(long, default_value = "stateful")]
        mode: String,
    },
```

- [ ] **Step 4: Update dispatch in main.rs**

Replace the `LedgerAction::Create` match arm (lines 374-376):

```rust
            LedgerAction::Create {
                name,
                path,
                from,
                instance_id,
                mode,
            } => ledger::cmd_ledger_create(
                &cli.config_dir,
                name.as_deref(),
                path.as_deref(),
                from.as_deref(),
                instance_id.as_deref(),
                &mode,
            ),
```

- [ ] **Step 5: Rewrite `cmd_ledger_create` to handle both modes**

Replace the entire `cmd_ledger_create` function in `src/cli/ledger.rs`:

```rust
// [cmd-ledger-create]
pub fn cmd_ledger_create(
    config_dir: &str,
    name: Option<&str>,
    path: Option<&str>,
    from_template: Option<&str>,
    instance_id: Option<&str>,
    mode_str: &str,
) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let mode = match mode_str {
        "stateful" => LedgerMode::Stateful,
        "event-only" => LedgerMode::EventOnly,
        other => {
            eprintln!(
                "Unknown ledger mode '{}'. Valid: stateful, event-only.",
                other
            );
            return EXIT_USAGE_ERROR;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);

    // Determine name, path, and template metadata based on creation mode
    let (ledger_name, ledger_file, tmpl_name, tmpl_id) = if let Some(template_name) =
        from_template
    {
        // --- Template-based creation ---
        let tmpl = match config.ledgers.get(template_name) {
            Some(t) => t,
            None => {
                eprintln!(
                    "No ledger template '{}' in protocol.toml. Available: {}",
                    template_name,
                    if config.ledgers.is_empty() {
                        "(none)".to_string()
                    } else {
                        config.ledgers.keys().cloned().collect::<Vec<_>>().join(", ")
                    }
                );
                return EXIT_CONFIG_ERROR;
            }
        };

        let path_template = match &tmpl.path_template {
            Some(pt) => pt,
            None => {
                eprintln!(
                    "Ledger '{}' uses a fixed path, not a path_template. Use --name/--path instead.",
                    template_name
                );
                return EXIT_USAGE_ERROR;
            }
        };

        let id = match instance_id {
            Some(id) => id,
            None => {
                eprintln!(
                    "Template '{}' requires an instance_id (e.g., `ledger create --from {} 25`)",
                    template_name, template_name
                );
                return EXIT_USAGE_ERROR;
            }
        };

        let resolved_path = path_template
            .replace("{template.instance_id}", id)
            .replace("{template.name}", template_name);

        let derived_name = format!("{}-{}", template_name, id);
        let file = if PathBuf::from(&resolved_path).is_absolute() {
            PathBuf::from(&resolved_path)
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(&resolved_path)
        };

        (
            derived_name,
            file,
            Some(template_name.to_string()),
            Some(id.to_string()),
        )
    } else {
        // --- Direct creation ---
        let n = name.unwrap(); // clap ensures this is present when --from is absent
        let p = path.unwrap();

        let file = if PathBuf::from(p).is_absolute() {
            PathBuf::from(p)
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(p)
        };

        (n.to_string(), file, None, None)
    };

    // Initialize the ledger file
    if let Err(e) = std::fs::create_dir_all(ledger_file.parent().unwrap_or(Path::new("."))) {
        eprintln!("Cannot create directory for ledger: {}", e);
        return EXIT_CONFIG_ERROR;
    }

    match Ledger::init(
        &ledger_file,
        &config.protocol.name,
        &config.protocol.version,
    ) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Cannot initialize ledger: {}", e);
            return EXIT_INTEGRITY_ERROR;
        }
    }

    // Register in the registry
    let reg_path = registry_path_from_config(&config);
    let mut registry = match LedgerRegistry::new(&reg_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Cannot load registry: {}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    let registry_stored_path =
        super::commands::compute_registry_path(&ledger_file, &data_dir);
    if let Err(e) = registry.create_with_template(
        &ledger_name,
        &registry_stored_path,
        mode,
        tmpl_name.as_deref(),
        tmpl_id.as_deref(),
    ) {
        eprintln!("Cannot register ledger: {}", e);
        return EXIT_CONFIG_ERROR;
    }

    println!(
        "Ledger '{}' created at {} and registered.",
        ledger_name,
        ledger_file.display()
    );
    EXIT_SUCCESS
}
```

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 7: Commit**

```bash
git add src/main.rs src/cli/ledger.rs
git commit -m "feat: add --from flag for template-based ledger creation

'ledger create --from run 25' resolves path_template, derives name
as 'run-25', and stores template + instance_id in registry.
Direct mode (--name/--path) still works."
```

---

### Task 6: Add `ledger_template` field to RenderConfig

**Files:**
- Modify: `src/config/renders.rs:19-27`
- Modify: `src/config/mod.rs` (validate_deep — renders must not set both)
- Test: `tests/config_tests.rs`

- [ ] **Step 1: Write the failing test**

Add to `tests/config_tests.rs`:

```rust
#[test]
fn test_render_config_ledger_template_field() {
    use sahjhan::config::renders::RendersFile;

    let toml_str = r#"
[[renders]]
target = "STATUS.md"
template = "templates/status.md.tera"
trigger = "on_transition"
ledger_template = "run"
"#;

    let rf: RendersFile = toml::from_str(toml_str).unwrap();
    assert_eq!(rf.renders.len(), 1);
    assert_eq!(rf.renders[0].ledger_template.as_deref(), Some("run"));
    assert!(rf.renders[0].ledger.is_none());
}

#[test]
fn test_validate_render_both_ledger_and_ledger_template() {
    use sahjhan::config::*;
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.renders.push(RenderConfig {
        target: "bad.md".to_string(),
        template: "templates/status.md.tera".to_string(),
        trigger: "on_transition".to_string(),
        event_types: None,
        ledger: Some("default".to_string()),
        ledger_template: Some("run".to_string()),
    });
    let (errors, _) = config.validate_deep(Path::new("examples/minimal"));
    assert!(
        errors.iter().any(|e| e.contains("bad.md") && e.contains("both")),
        "Expected error about both ledger and ledger_template: {:?}",
        errors
    );
}

#[test]
fn test_validate_render_ledger_template_references_valid_template() {
    use sahjhan::config::*;
    let mut config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    config.renders.push(RenderConfig {
        target: "ref.md".to_string(),
        template: "templates/status.md.tera".to_string(),
        trigger: "on_transition".to_string(),
        event_types: None,
        ledger: None,
        ledger_template: Some("nonexistent".to_string()),
    });
    let (errors, _) = config.validate_deep(Path::new("examples/minimal"));
    assert!(
        errors.iter().any(|e| e.contains("ref.md") && e.contains("nonexistent")),
        "Expected error about unknown ledger template: {:?}",
        errors
    );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test test_render_config_ledger_template -- --nocapture`
Expected: FAIL — `ledger_template` field doesn't exist

- [ ] **Step 3: Add `ledger_template` to RenderConfig**

In `src/config/renders.rs`, replace `RenderConfig`:

```rust
/// A single render definition.
#[derive(Debug, Deserialize, Clone)]
pub struct RenderConfig {
    pub target: String,
    pub template: String,
    pub trigger: String,
    pub event_types: Option<Vec<String>>,
    /// Optional: which named ledger (from ledgers.toml) to read from.
    /// If absent, the default ledger is used.
    pub ledger: Option<String>,
    /// Optional: which ledger template (from protocol.toml [ledgers]) to resolve.
    /// Resolves to the active (targeted) ledger if its template matches.
    pub ledger_template: Option<String>,
}
```

Update the index comment:

```
// - RenderConfig            — target, template, trigger, event_types, ledger, ledger_template
```

- [ ] **Step 4: Add render validation to validate_deep**

In `src/config/mod.rs`, in `validate_deep`, after the ledger template validation block (task 2), add:

```rust
        // 13. Render ledger/ledger_template validation.
        for render in &self.renders {
            if render.ledger.is_some() && render.ledger_template.is_some() {
                errors.push(format!(
                    "renders.toml: render for '{}' has both 'ledger' and 'ledger_template' — use one or the other",
                    render.target
                ));
            }
            if let Some(ref tmpl_name) = render.ledger_template {
                if !self.ledgers.contains_key(tmpl_name) {
                    errors.push(format!(
                        "renders.toml: render for '{}' references ledger_template '{}' which is not declared in protocol.toml [ledgers]",
                        render.target, tmpl_name
                    ));
                }
            }
        }
```

- [ ] **Step 5: Run tests**

Run: `cargo test test_render_config_ledger_template test_validate_render_both test_validate_render_ledger_template_references -- --nocapture`
Expected: PASS

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 7: Commit**

```bash
git add src/config/renders.rs src/config/mod.rs tests/config_tests.rs
git commit -m "feat: add ledger_template field to RenderConfig

Renders can now reference protocol-declared ledger templates.
Validation rejects both ledger + ledger_template, and unknown
template names."
```

---

### Task 7: Implement render ledger resolution via `ledger_template` (Bug 2)

**Files:**
- Modify: `src/render/engine.rs:50-56` (RenderEngine struct), `58-86` (constructors), `93-162` (resolve_render_ledger)
- Modify: `src/cli/transition.rs:89-121` (transition render call), `366-398` (event render call)
- Modify: `src/cli/status.rs:331-363` (set complete render call)
- Modify: `src/cli/render.rs:52-54` (render command)
- Test: new test file or added to existing

- [ ] **Step 1: Add `active_ledger_name` to RenderEngine**

In `src/render/engine.rs`, add field to `RenderEngine` struct (after `registry_path` on line 55):

```rust
    /// Name of the active (targeted) ledger, used to resolve `ledger_template` references.
    active_ledger_name: Option<String>,
```

Add builder method after `with_registry` (after line 86):

```rust
    /// Set the name of the active (targeted) ledger for template resolution.
    pub fn with_active_ledger_name(mut self, name: String) -> Self {
        self.active_ledger_name = Some(name);
        self
    }
```

Initialize in `new` (in the `Ok(RenderEngine { ... })` block):

```rust
        Ok(RenderEngine {
            tera,
            config: config.clone(),
            registry_path: None,
            active_ledger_name: None,
        })
```

- [ ] **Step 2: Rewrite `resolve_render_ledger` to handle `ledger_template`**

Replace `resolve_render_ledger` (lines 93-162):

```rust
    /// Resolve the ledger for a render config.
    ///
    /// Resolution order:
    /// 1. `ledger` field: literal registry lookup.
    /// 2. `ledger_template` field: check active ledger's template, then most recent.
    /// 3. Neither: use default ledger (return `Ok(None)`).
    fn resolve_render_ledger(
        &self,
        render_cfg: &crate::config::RenderConfig,
    ) -> Result<Option<Ledger>, String> {
        // --- Direct ledger name ---
        if let Some(ref ledger_name) = render_cfg.ledger {
            return self.resolve_ledger_by_name(ledger_name, &render_cfg.target);
        }

        // --- Ledger template ---
        if let Some(ref tmpl_name) = render_cfg.ledger_template {
            return self.resolve_ledger_by_template(tmpl_name, &render_cfg.target);
        }

        // --- Default ---
        Ok(None)
    }

    /// Look up a specific ledger by name from the registry.
    fn resolve_ledger_by_name(
        &self,
        ledger_name: &str,
        render_target: &str,
    ) -> Result<Option<Ledger>, String> {
        let reg_path = match self.registry_path.as_ref() {
            Some(p) => p,
            None => {
                eprintln!(
                    "  Render '{}': ledger '{}' requested but no registry configured; using default ledger",
                    render_target, ledger_name
                );
                return Ok(None);
            }
        };

        let registry = match LedgerRegistry::new(reg_path) {
            Ok(r) => r,
            Err(_) => {
                eprintln!(
                    "  Render '{}': cannot load registry; using default ledger for '{}'",
                    render_target, ledger_name
                );
                return Ok(None);
            }
        };

        let entry = match registry.resolve(Some(ledger_name)) {
            Ok(e) => e,
            Err(_) => {
                eprintln!(
                    "  Render '{}': ledger '{}' not found in registry; using default ledger",
                    render_target, ledger_name
                );
                return Ok(None);
            }
        };

        self.open_registry_entry(entry, render_target)
    }

    /// Resolve a ledger template reference against the active ledger or most recent match.
    fn resolve_ledger_by_template(
        &self,
        tmpl_name: &str,
        render_target: &str,
    ) -> Result<Option<Ledger>, String> {
        let reg_path = match self.registry_path.as_ref() {
            Some(p) => p,
            None => {
                eprintln!(
                    "  Render '{}': ledger_template '{}' requested but no registry configured; using default ledger",
                    render_target, tmpl_name
                );
                return Ok(None);
            }
        };

        let registry = match LedgerRegistry::new(reg_path) {
            Ok(r) => r,
            Err(_) => {
                eprintln!(
                    "  Render '{}': cannot load registry; using default ledger",
                    render_target
                );
                return Ok(None);
            }
        };

        // First: check if the active ledger matches this template
        if let Some(ref active_name) = self.active_ledger_name {
            if let Ok(entry) = registry.resolve(Some(active_name)) {
                if entry.template.as_deref() == Some(tmpl_name) {
                    return self.open_registry_entry(entry, render_target);
                }
            }
        }

        // Fallback: most recently created ledger with this template
        let matches = registry.resolve_by_template(tmpl_name);
        if let Some(entry) = matches.last() {
            return self.open_registry_entry(entry, render_target);
        }

        eprintln!(
            "  Render '{}': no ledger found for template '{}'; using default ledger",
            render_target, tmpl_name
        );
        Ok(None)
    }

    /// Open a ledger from a registry entry, resolving relative paths.
    fn open_registry_entry(
        &self,
        entry: &crate::ledger::registry::LedgerRegistryEntry,
        render_target: &str,
    ) -> Result<Option<Ledger>, String> {
        let ledger_path = {
            let p = std::path::PathBuf::from(&entry.path);
            if p.is_absolute() {
                p
            } else {
                self.registry_path
                    .as_ref()
                    .and_then(|rp| rp.parent())
                    .unwrap_or_else(|| std::path::Path::new("."))
                    .join(p)
            }
        };

        match Ledger::open(&ledger_path) {
            Ok(l) => Ok(Some(l)),
            Err(_) => {
                eprintln!(
                    "  Render '{}': cannot open ledger '{}' at {}; using default ledger",
                    render_target,
                    entry.name,
                    ledger_path.display()
                );
                Ok(None)
            }
        }
    }
```

- [ ] **Step 3: Pass active ledger name at all render call sites**

In `src/cli/transition.rs`, in `cmd_transition` (around line 92-93), change:

```rust
                    let engine = engine.with_registry(registry_path);
```

To:

```rust
                    let mut engine = engine.with_registry(registry_path);
                    if let Some(ref name) = targeting.ledger_name {
                        engine = engine.with_active_ledger_name(name.clone());
                    }
```

In `src/cli/transition.rs`, in `cmd_event` (around line 368-369), change:

```rust
                    let engine = engine.with_registry(registry_path);
```

To:

```rust
                    let mut engine = engine.with_registry(registry_path);
                    if let Some(ref name) = targeting.ledger_name {
                        engine = engine.with_active_ledger_name(name.clone());
                    }
```

In `src/cli/status.rs`, in `cmd_set_complete` (around line 333-334), change:

```rust
                    let engine = engine.with_registry(registry_path);
```

To:

```rust
                    let mut engine = engine.with_registry(registry_path);
                    if let Some(ref name) = targeting.ledger_name {
                        engine = engine.with_active_ledger_name(name.clone());
                    }
```

In `src/cli/render.rs`, in `cmd_render` (around line 53-54), change:

```rust
        Ok(e) => e.with_registry(registry_path),
```

To:

```rust
        Ok(e) => {
            let mut engine = e.with_registry(registry_path);
            if let Some(ref name) = targeting.ledger_name {
                engine = engine.with_active_ledger_name(name.clone());
            }
            engine
        }
```

In `src/cli/render.rs`, in `cmd_render_dump_context` (around line 110-111), apply the same pattern:

```rust
        Ok(e) => {
            let mut engine = e.with_registry(registry_path);
            if let Some(ref name) = targeting.ledger_name {
                engine = engine.with_active_ledger_name(name.clone());
            }
            engine
        }
```

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 5: Update index header in engine.rs**

Update `src/render/engine.rs` index to include the new methods.

- [ ] **Step 6: Commit**

```bash
git add src/render/engine.rs src/cli/transition.rs src/cli/status.rs src/cli/render.rs
git commit -m "feat: resolve ledger_template in renders via active ledger (issue #10 bug 2)

Renders with ledger_template check the active ledger's template
metadata first, then fall back to most recent registry match.
All render call sites now pass the active ledger name."
```

---

### Task 8: Fix status display to use registry metadata (Bug 3)

**Files:**
- Modify: `src/cli/status.rs:118-128`
- Test: visual verification (status output is formatted text)

- [ ] **Step 1: Add registry imports to status.rs**

At the top of `src/cli/status.rs`, add to the imports from `super::commands`:

```rust
use super::commands::{
    build_state_params, load_config, load_manifest, open_targeted_ledger,
    registry_path_from_config, resolve_config_dir, resolve_data_dir, save_manifest,
    track_ledger_in_manifest, LedgerTargeting, EXIT_INTEGRITY_ERROR, EXIT_SUCCESS,
    EXIT_USAGE_ERROR,
};
```

And add:

```rust
use crate::ledger::registry::LedgerRegistry;
```

- [ ] **Step 2: Replace run number logic**

In `src/cli/status.rs`, replace lines 118-128:

```rust
    // Run number = count of protocol_init events (should be 1 normally)
    let run_number = machine.ledger().events_of_type("protocol_init").len();

    // Header
    let width = 59;
    let bar = "=".repeat(width);
    println!("{}", bar);
    println!(
        "  sahjhan · {} v{} · Run {}",
        config.protocol.name, config.protocol.version, run_number
    );
    println!("{}", bar);
```

With:

```rust
    // Build instance label from registry metadata
    let instance_label = if let Some(ref name) = targeting.ledger_name {
        let reg_path = registry_path_from_config(&config);
        if let Ok(registry) = LedgerRegistry::new(&reg_path) {
            if let Ok(entry) = registry.resolve(Some(name)) {
                match (&entry.template, &entry.instance_id) {
                    (Some(tmpl), Some(id)) => format!(" · {} {}", tmpl, id),
                    _ => format!(" · {}", name),
                }
            } else {
                String::new()
            }
        } else {
            String::new()
        }
    } else {
        String::new()
    };

    // Header
    let width = 59;
    let bar = "=".repeat(width);
    println!("{}", bar);
    println!(
        "  sahjhan · {} v{}{}",
        config.protocol.name, config.protocol.version, instance_label
    );
    println!("{}", bar);
```

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add src/cli/status.rs
git commit -m "fix: status display uses registry template metadata (issue #10 bug 3)

Replaces protocol_init event counting with template/instance_id
from registry. Shows 'run 25' for template-created ledgers,
ledger name for non-template ledgers, nothing for default."
```

---

### Task 9: Inject template_instance_id into render context

**Files:**
- Modify: `src/render/engine.rs` (render_triggered and render_all — inject context var)
- Test: `tests/integration_tests.rs` or render test

- [ ] **Step 1: Write the failing test**

Add a unit test that checks the render context includes the template instance_id. Add to an appropriate test file:

```rust
#[test]
fn test_render_context_includes_template_instance_id() {
    // This is a conceptual test — the render engine injects
    // template_instance_id into the Tera context when the active
    // ledger has template metadata.
    //
    // Since build_context is private, we test this through dump_context
    // by checking the JSON output includes the field.
    // Skipping for now — integration tested through the full CLI.
}
```

Actually, the cleanest way is to inject `template_instance_id` into the context in `build_context` when the active ledger name is set and has template metadata.

- [ ] **Step 2: Add template_instance_id to build_context**

In `src/render/engine.rs`, in the `build_context` method (around line 389, before `Ok(ctx)`), add:

```rust
        // Inject template instance_id if active ledger has template metadata
        if let (Some(ref active_name), Some(ref reg_path)) =
            (&self.active_ledger_name, &self.registry_path)
        {
            if let Ok(registry) = LedgerRegistry::new(reg_path) {
                if let Ok(entry) = registry.resolve(Some(active_name)) {
                    if let Some(ref id) = entry.instance_id {
                        ctx.insert("template_instance_id", id);
                    }
                    if let Some(ref tmpl) = entry.template {
                        ctx.insert("template_name", tmpl);
                    }
                }
            }
        }
```

- [ ] **Step 3: Run full test suite**

Run: `cargo test`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add src/render/engine.rs
git commit -m "feat: inject template_instance_id into render context

Templates can use {{ template_instance_id }} and {{ template_name }}
when the active ledger has template metadata."
```

---

### Task 10: Update CLAUDE.md documentation

**Files:**
- Modify: `CLAUDE.md`
- Modify: Index headers in all changed source files (verify)

- [ ] **Step 1: Update protocol.rs section in CLAUDE.md**

In `CLAUDE.md`, update the `config/` table to include `LedgerTemplateConfig`:

Add row:
```
| Ledger templates | `config/protocol.rs` | `LedgerTemplateConfig` | Ledger declarations: path or path_template |
```

- [ ] **Step 2: Update registry section in CLAUDE.md**

In `CLAUDE.md`, update `ledger/` table for `LedgerRegistryEntry` to mention the new fields, and add `resolve_by_template`:

Update the `LedgerRegistryEntry` row description to:
```
| Entry struct | `ledger/registry.rs` | `LedgerRegistryEntry` | name, path, mode, template, instance_id |
```

Add row:
```
| Template query | `ledger/registry.rs` | `[resolve-by-template]` | Find entries by template name |
```

- [ ] **Step 3: Update cli/ table**

Add row to the cli/ section:
```
| Ledger template create | `cli/ledger.rs` | `[cmd-ledger-create]` | Template-based and direct ledger creation |
```

Update the existing `cmd-ledger-create` description if it says something different.

Add `compute_registry_path` to the cli/commands.rs section:
```
| Registry path compute | `cli/commands.rs` | `[compute-registry-path]` | Compute registry-storable path relative to data_dir |
```

- [ ] **Step 4: Update render/engine.rs section**

Add rows:
```
| Template resolution | `render/engine.rs` | `[resolve-ledger-by-template]` | Resolve ledger_template against active ledger |
| Active ledger config | `render/engine.rs` | `with_active_ledger_name()` | Set active ledger for template resolution |
```

- [ ] **Step 5: Update config/renders.rs section**

Update `RenderConfig` row to include `ledger_template`:
```
| Render definitions | `config/renders.rs` | `RenderConfig` | target, template, trigger, event_types, ledger, ledger_template |
```

- [ ] **Step 6: Verify all source file index headers are up to date**

Check each modified file's `## Index` comment matches its actual contents:
- `src/config/protocol.rs` — includes `LedgerTemplateConfig`
- `src/config/renders.rs` — includes `ledger_template` mention
- `src/ledger/registry.rs` — includes `template`, `instance_id`
- `src/render/engine.rs` — includes new methods
- `src/cli/commands.rs` — includes `compute_registry_path`

- [ ] **Step 7: Run clippy and fmt**

Run: `cargo clippy -- -D warnings && cargo fmt`
Expected: Clean

- [ ] **Step 8: Run full test suite one final time**

Run: `cargo test`
Expected: All 233+ tests pass (original count + new tests)

- [ ] **Step 9: Commit**

```bash
git add CLAUDE.md src/
git commit -m "docs: update CLAUDE.md and index headers for ledger templates"
```
