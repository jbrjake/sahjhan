// tests/template_tests.rs
//
// Integration tests for template-based ledger creation via cmd_ledger_create.

use std::fs;
use std::path::Path;
use tempfile::TempDir;

use sahjhan::ledger::registry::{LedgerMode, LedgerRegistry};

fn write_template_protocol(dir: &Path) {
    let data_dir = dir.join(".sahjhan");
    let data_dir_str = data_dir.to_string_lossy();
    let runs_dir = dir.join("runs");
    let runs_dir_str = runs_dir.to_string_lossy();

    fs::write(
        dir.join("protocol.toml"),
        format!(
            r#"
[protocol]
name = "test"
version = "1.0.0"
description = "test"

[paths]
managed = []
data_dir = "{}"
render_dir = "."

[ledgers.run]
description = "Per-run ledger"
path_template = "{}/{{template.instance_id}}/ledger.jsonl"
"#,
            data_dir_str, runs_dir_str
        ),
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

    fs::write(
        dir.join("transitions.toml"),
        "[[transitions]]\nfrom = \"idle\"\nto = \"idle\"\ncommand = \"noop\"\ngates = []\n",
    )
    .unwrap();
}

/// Template-based creation resolves path, derives name, stores metadata.
#[test]
fn test_template_create_full_stack() {
    let dir = TempDir::new().unwrap();
    write_template_protocol(dir.path());

    let data_dir = dir.path().join(".sahjhan");
    fs::create_dir_all(&data_dir).unwrap();

    let config_dir = dir.path().to_string_lossy().to_string();

    let result = sahjhan::cli::ledger::cmd_ledger_create(
        &config_dir,
        None,          // name (not used in template mode)
        None,          // path (not used in template mode)
        Some("run"),   // from_template
        Some("25"),    // instance_id
        "stateful",    // mode
    );

    assert_eq!(result, 0, "cmd_ledger_create should succeed");

    // Verify ledger file was created at the resolved path.
    // The path_template is relative so it resolves relative to cwd at runtime.
    // We look it up from the registry to confirm what was stored.
    let reg_path = data_dir.join("ledgers.toml");
    let registry = LedgerRegistry::new(&reg_path).unwrap();
    let entry = registry.resolve(Some("run-25")).unwrap();
    assert_eq!(entry.name, "run-25");
    assert_eq!(entry.template.as_deref(), Some("run"));
    assert_eq!(entry.instance_id.as_deref(), Some("25"));
    assert_eq!(entry.mode, LedgerMode::Stateful);
}

/// Template-based creation fails for unknown template.
#[test]
fn test_template_create_unknown_template() {
    let dir = TempDir::new().unwrap();
    write_template_protocol(dir.path());

    let data_dir = dir.path().join(".sahjhan");
    fs::create_dir_all(&data_dir).unwrap();

    let config_dir = dir.path().to_string_lossy().to_string();

    let result = sahjhan::cli::ledger::cmd_ledger_create(
        &config_dir,
        None,
        None,
        Some("nonexistent"),
        Some("1"),
        "stateful",
    );

    assert_ne!(result, 0, "Should fail for unknown template");
}

/// Template-based creation without instance_id fails with a clear error.
#[test]
fn test_template_create_missing_instance_id() {
    let dir = TempDir::new().unwrap();
    write_template_protocol(dir.path());

    let data_dir = dir.path().join(".sahjhan");
    fs::create_dir_all(&data_dir).unwrap();

    let config_dir = dir.path().to_string_lossy().to_string();

    let result = sahjhan::cli::ledger::cmd_ledger_create(
        &config_dir,
        None,
        None,
        Some("run"),
        None, // no instance_id
        "stateful",
    );

    assert_ne!(result, 0, "Should fail when instance_id is missing");
}
