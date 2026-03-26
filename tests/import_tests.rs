// import_tests.rs — Tests for ledger JSONL import (Task 9)

use sahjhan::ledger::chain::Ledger;
use sahjhan::ledger::import::import_jsonl;
use std::io::BufReader;
use tempfile::tempdir;

// ---- 1. import_bare_jsonl: 2 events → 3 entries (genesis + 2), chain valid ----

#[test]
fn test_import_bare_jsonl() {
    let dir = tempdir().unwrap();
    let output = dir.path().join("imported.jsonl");

    let input = r#"{"type":"finding","fields":{"id":"BH-001","severity":"HIGH"}}
{"type":"note","fields":{"text":"all clear"}}
"#;
    let mut reader = BufReader::new(input.as_bytes());

    import_jsonl(&mut reader, &output, "test-proto", "1.0.0").unwrap();

    let ledger = Ledger::open(&output).unwrap();
    assert_eq!(ledger.len(), 3, "genesis + 2 events = 3 entries");

    // Entry 0 is genesis
    assert_eq!(ledger.entries()[0].event_type, "genesis");

    // Entries 1 and 2 are the imported events
    assert_eq!(ledger.entries()[1].event_type, "finding");
    assert_eq!(ledger.entries()[2].event_type, "note");

    // Chain must be valid
    ledger.verify().unwrap();
}

// ---- 2. import_with_existing_timestamps: ts field preserved ----

#[test]
fn test_import_with_existing_timestamps() {
    let dir = tempdir().unwrap();
    let output = dir.path().join("imported.jsonl");

    let ts = "2026-01-15T10:00:00.000Z";
    let input = format!(
        r#"{{"type":"finding","ts":"{ts}","fields":{{"id":"BH-002"}}}}"#,
        ts = ts
    );

    let mut reader = BufReader::new(input.as_bytes());
    import_jsonl(&mut reader, &output, "test-proto", "1.0.0").unwrap();

    let ledger = Ledger::open(&output).unwrap();
    assert_eq!(ledger.len(), 2);

    let entry = &ledger.entries()[1];
    assert_eq!(entry.ts, ts, "timestamp should be preserved from input");
    ledger.verify().unwrap();
}

// ---- 3. import_preserves_fields: all field key-values present ----

#[test]
fn test_import_preserves_fields() {
    let dir = tempdir().unwrap();
    let output = dir.path().join("imported.jsonl");

    let input = r#"{"type":"finding","fields":{"id":"BH-003","severity":"CRITICAL","cvss":"9.8","affected":"login-service"}}
"#;
    let mut reader = BufReader::new(input.as_bytes());
    import_jsonl(&mut reader, &output, "test-proto", "1.0.0").unwrap();

    let ledger = Ledger::open(&output).unwrap();
    assert_eq!(ledger.len(), 2);

    let entry = &ledger.entries()[1];
    assert_eq!(entry.fields.get("id").unwrap(), "BH-003");
    assert_eq!(entry.fields.get("severity").unwrap(), "CRITICAL");
    assert_eq!(entry.fields.get("cvss").unwrap(), "9.8");
    assert_eq!(entry.fields.get("affected").unwrap(), "login-service");
}

// ---- 4. import_empty_input: empty reader produces just genesis ----

#[test]
fn test_import_empty_input() {
    let dir = tempdir().unwrap();
    let output = dir.path().join("imported.jsonl");

    let input = "";
    let mut reader = BufReader::new(input.as_bytes());
    import_jsonl(&mut reader, &output, "test-proto", "1.0.0").unwrap();

    let ledger = Ledger::open(&output).unwrap();
    assert_eq!(ledger.len(), 1, "only genesis entry expected");
    assert_eq!(ledger.entries()[0].event_type, "genesis");
    ledger.verify().unwrap();
}

// ---- 5. import skips blank lines ----

#[test]
fn test_import_skips_blank_lines() {
    let dir = tempdir().unwrap();
    let output = dir.path().join("imported.jsonl");

    let input = r#"{"type":"finding","fields":{"id":"BH-004"}}

{"type":"note","fields":{"text":"done"}}

"#;
    let mut reader = BufReader::new(input.as_bytes());
    import_jsonl(&mut reader, &output, "test-proto", "1.0.0").unwrap();

    let ledger = Ledger::open(&output).unwrap();
    assert_eq!(ledger.len(), 3, "genesis + 2 events = 3 entries");
    ledger.verify().unwrap();
}

// ---- 6. import with missing fields key treats as empty fields ----

#[test]
fn test_import_missing_fields_key() {
    let dir = tempdir().unwrap();
    let output = dir.path().join("imported.jsonl");

    // An event with no "fields" key
    let input = r#"{"type":"heartbeat"}
"#;
    let mut reader = BufReader::new(input.as_bytes());
    import_jsonl(&mut reader, &output, "test-proto", "1.0.0").unwrap();

    let ledger = Ledger::open(&output).unwrap();
    assert_eq!(ledger.len(), 2);
    let entry = &ledger.entries()[1];
    assert_eq!(entry.event_type, "heartbeat");
    assert!(entry.fields.is_empty());
    ledger.verify().unwrap();
}
