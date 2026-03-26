use sahjhan::ledger::registry::{LedgerMode, LedgerRegistry};
use tempfile::TempDir;

fn temp_registry_path(dir: &TempDir) -> std::path::PathBuf {
    dir.path().join(".sahjhan").join("ledgers.toml")
}

// ---------------------------------------------------------------------------
// 1. create two entries, list returns both in insertion order
// ---------------------------------------------------------------------------
#[test]
fn test_create_and_list_ledgers() {
    let dir = TempDir::new().unwrap();
    let path = temp_registry_path(&dir);

    let mut reg = LedgerRegistry::new(&path).unwrap();
    reg.create(
        "run-21",
        "docs/holtz/runs/21/ledger.jsonl",
        LedgerMode::Stateful,
    )
    .unwrap();
    reg.create("project", "docs/holtz/project.jsonl", LedgerMode::EventOnly)
        .unwrap();

    let entries = reg.list();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].name, "run-21");
    assert_eq!(entries[1].name, "project");
}

// ---------------------------------------------------------------------------
// 2. remove entry, list is now empty
// ---------------------------------------------------------------------------
#[test]
fn test_remove_from_registry() {
    let dir = TempDir::new().unwrap();
    let path = temp_registry_path(&dir);

    let mut reg = LedgerRegistry::new(&path).unwrap();
    reg.create(
        "run-21",
        "docs/holtz/runs/21/ledger.jsonl",
        LedgerMode::Stateful,
    )
    .unwrap();
    reg.remove("run-21").unwrap();

    assert!(reg.list().is_empty());
}

// ---------------------------------------------------------------------------
// 3. resolve(None) returns the first (and only) entry
// ---------------------------------------------------------------------------
#[test]
fn test_resolve_default_ledger() {
    let dir = TempDir::new().unwrap();
    let path = temp_registry_path(&dir);

    let mut reg = LedgerRegistry::new(&path).unwrap();
    reg.create(
        "run-21",
        "docs/holtz/runs/21/ledger.jsonl",
        LedgerMode::Stateful,
    )
    .unwrap();

    let entry = reg.resolve(None).unwrap();
    assert_eq!(entry.name, "run-21");
}

// ---------------------------------------------------------------------------
// 4. resolve(Some("name")) returns the named entry, not the first
// ---------------------------------------------------------------------------
#[test]
fn test_resolve_named_ledger() {
    let dir = TempDir::new().unwrap();
    let path = temp_registry_path(&dir);

    let mut reg = LedgerRegistry::new(&path).unwrap();
    reg.create(
        "run-21",
        "docs/holtz/runs/21/ledger.jsonl",
        LedgerMode::Stateful,
    )
    .unwrap();
    reg.create("project", "docs/holtz/project.jsonl", LedgerMode::EventOnly)
        .unwrap();

    let entry = reg.resolve(Some("project")).unwrap();
    assert_eq!(entry.name, "project");
    assert_eq!(entry.path, "docs/holtz/project.jsonl");
}

// ---------------------------------------------------------------------------
// 5. event-only mode survives round-trip through TOML
// ---------------------------------------------------------------------------
#[test]
fn test_event_only_mode_stored() {
    let dir = TempDir::new().unwrap();
    let path = temp_registry_path(&dir);

    let mut reg = LedgerRegistry::new(&path).unwrap();
    reg.create("project", "docs/holtz/project.jsonl", LedgerMode::EventOnly)
        .unwrap();

    let entry = &reg.list()[0];
    assert_eq!(entry.mode, LedgerMode::EventOnly);
}

// ---------------------------------------------------------------------------
// 6. duplicate name is rejected
// ---------------------------------------------------------------------------
#[test]
fn test_duplicate_name_rejected() {
    let dir = TempDir::new().unwrap();
    let path = temp_registry_path(&dir);

    let mut reg = LedgerRegistry::new(&path).unwrap();
    reg.create(
        "run-21",
        "docs/holtz/runs/21/ledger.jsonl",
        LedgerMode::Stateful,
    )
    .unwrap();

    let result = reg.create("run-21", "other/path.jsonl", LedgerMode::Stateful);
    assert!(result.is_err());
    // Error message should mention the duplicate name
    let msg = result.unwrap_err();
    assert!(
        msg.contains("run-21"),
        "error should name the duplicate: {msg}"
    );
}

// ---------------------------------------------------------------------------
// 7. resolve(None) on an empty registry is an error
// ---------------------------------------------------------------------------
#[test]
fn test_resolve_empty_registry_errors() {
    let dir = TempDir::new().unwrap();
    let path = temp_registry_path(&dir);

    let reg = LedgerRegistry::new(&path).unwrap();
    let result = reg.resolve(None);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// 8. persistence: entries survive drop + reload from disk
// ---------------------------------------------------------------------------
#[test]
fn test_persistence() {
    let dir = TempDir::new().unwrap();
    let path = temp_registry_path(&dir);

    {
        let mut reg = LedgerRegistry::new(&path).unwrap();
        reg.create(
            "run-21",
            "docs/holtz/runs/21/ledger.jsonl",
            LedgerMode::Stateful,
        )
        .unwrap();
        reg.create("project", "docs/holtz/project.jsonl", LedgerMode::EventOnly)
            .unwrap();
    } // reg is dropped here

    // Reload from disk
    let reg2 = LedgerRegistry::new(&path).unwrap();
    let entries = reg2.list();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].name, "run-21");
    assert_eq!(entries[0].path, "docs/holtz/runs/21/ledger.jsonl");
    assert_eq!(entries[0].mode, LedgerMode::Stateful);
    assert_eq!(entries[1].name, "project");
    assert_eq!(entries[1].mode, LedgerMode::EventOnly);
    // created timestamps should be non-empty ISO 8601
    assert!(!entries[0].created.is_empty());
    assert!(entries[0].created.contains('T'));
}
