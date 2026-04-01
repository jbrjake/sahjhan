// chain_integrity_tests.rs — Tests for JSONL ledger chain operations (Task 3)

use sahjhan::ledger::chain::Ledger;
use std::collections::BTreeMap;
use std::fs;
use std::io::Write;
use tempfile::tempdir;

fn fields(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

// ---- 1. Init creates JSONL file with genesis ----

#[test]
fn test_init_creates_jsonl_file_with_genesis() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.jsonl");

    let ledger = Ledger::init(&path, "test-proto", "1.0.0").unwrap();

    // File exists and is readable text
    let contents = fs::read_to_string(&path).unwrap();
    assert!(!contents.is_empty(), "file should not be empty");

    // Exactly one line (plus trailing newline)
    let lines: Vec<&str> = contents.lines().collect();
    assert_eq!(lines.len(), 1, "genesis should be exactly one line");

    // The line is valid JSON
    let parsed: serde_json::Value = serde_json::from_str(lines[0]).unwrap();
    assert_eq!(parsed["seq"], 0);
    assert_eq!(parsed["type"], "genesis");

    // In-memory state
    assert_eq!(ledger.len(), 1);
    let genesis = &ledger.entries()[0];
    assert_eq!(genesis.seq, 0);
    assert_eq!(genesis.event_type, "genesis");

    // Fields contain protocol info
    assert_eq!(genesis.fields.get("protocol_name").unwrap(), "test-proto");
    assert_eq!(genesis.fields.get("protocol_version").unwrap(), "1.0.0");

    // prev is a 64-char hex nonce (32 bytes)
    assert_eq!(genesis.prev.len(), 64);
    assert!(genesis.prev.chars().all(|c| c.is_ascii_hexdigit()));
}

// ---- 2. Append and reload ----

#[test]
fn test_append_and_reload() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.jsonl");

    let mut ledger = Ledger::init(&path, "test-proto", "1.0.0").unwrap();
    ledger
        .append(
            "state_change",
            fields(&[("from", "start"), ("to", "running")]),
        )
        .unwrap();

    assert_eq!(ledger.len(), 2);

    // Reload from disk
    let reopened = Ledger::open(&path).unwrap();
    assert_eq!(reopened.len(), 2);
    assert_eq!(reopened.entries()[1].event_type, "state_change");
    assert_eq!(reopened.entries()[1].seq, 1);
    assert_eq!(reopened.entries()[1].fields.get("from").unwrap(), "start");
}

// ---- 3. Verify detects tampered hash ----

#[test]
fn test_verify_detects_tampered_hash() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.jsonl");

    let mut ledger = Ledger::init(&path, "test-proto", "1.0.0").unwrap();
    ledger.append("event_a", fields(&[("key", "val")])).unwrap();
    drop(ledger);

    // Tamper with the hash of the second entry
    let contents = fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = contents.lines().collect();
    assert_eq!(lines.len(), 2);

    // Replace a character in the hash field of the second line
    // Replace a hex char in the hash value to create invalid JSON hash.
    let second_line = lines[1].to_string();
    let tampered = if second_line.contains("\"hash\":\"a") {
        second_line.replacen("\"hash\":\"a", "\"hash\":\"b", 1)
    } else {
        second_line.replacen("\"hash\":\"", "\"hash\":\"ff", 1)
    };

    let new_contents = format!("{}\n{}\n", lines[0], tampered);
    fs::write(&path, new_contents).unwrap();

    // Opening should fail due to hash mismatch
    let result = Ledger::open(&path);
    assert!(result.is_err(), "opening tampered file should fail");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("hash mismatch"),
        "error should mention hash mismatch, got: {}",
        err
    );
}

// ---- 4. Verify detects sequence gap ----

#[test]
fn test_verify_detects_sequence_gap() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.jsonl");

    // Create a ledger with 3 entries
    let mut ledger = Ledger::init(&path, "test-proto", "1.0.0").unwrap();
    ledger.append("event_a", BTreeMap::new()).unwrap();
    ledger.append("event_b", BTreeMap::new()).unwrap();
    drop(ledger);

    // Remove the middle line (seq=1) to create a gap: 0, 2
    let contents = fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = contents.lines().collect();
    assert_eq!(lines.len(), 3);

    let new_contents = format!("{}\n{}\n", lines[0], lines[2]);
    fs::write(&path, new_contents).unwrap();

    let result = Ledger::open(&path);
    assert!(result.is_err(), "should detect sequence gap");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("sequence gap") || err.contains("chain break"),
        "error should mention sequence gap or chain break, got: {}",
        err
    );
}

// ---- 5. Blank lines skipped ----

#[test]
fn test_blank_lines_skipped() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.jsonl");

    let mut ledger = Ledger::init(&path, "test-proto", "1.0.0").unwrap();
    ledger.append("event_a", BTreeMap::new()).unwrap();
    drop(ledger);

    // Insert blank lines into the file
    let contents = fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = contents.lines().collect();
    let new_contents = format!("{}\n\n\n{}\n\n", lines[0], lines[1]);
    fs::write(&path, new_contents).unwrap();

    // Should still open fine
    let reopened = Ledger::open(&path).unwrap();
    assert_eq!(reopened.len(), 2);
    reopened.verify().unwrap();
}

// ---- 6. Verify detects deletion ----

#[test]
fn test_verify_detects_deletion() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.jsonl");

    let mut ledger = Ledger::init(&path, "test-proto", "1.0.0").unwrap();
    ledger.append("event_a", BTreeMap::new()).unwrap();
    ledger.append("event_b", BTreeMap::new()).unwrap();
    drop(ledger);

    // Remove the last entry
    let contents = fs::read_to_string(&path).unwrap();
    let lines: Vec<&str> = contents.lines().collect();
    assert_eq!(lines.len(), 3);

    // Keep only first two entries, removing third — chain is valid but now
    // remove the MIDDLE entry so the chain breaks
    let new_contents = format!("{}\n{}\n", lines[0], lines[2]);
    fs::write(&path, new_contents).unwrap();

    let result = Ledger::open(&path);
    assert!(
        result.is_err(),
        "should detect chain break from deleted entry"
    );
}

// ---- 7. Events of type ----

#[test]
fn test_events_of_type() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.jsonl");

    let mut ledger = Ledger::init(&path, "test-proto", "1.0.0").unwrap();
    ledger
        .append("state_change", fields(&[("to", "a")]))
        .unwrap();
    ledger
        .append("gate_eval", fields(&[("gate", "g1")]))
        .unwrap();
    ledger
        .append("state_change", fields(&[("to", "b")]))
        .unwrap();
    ledger
        .append("gate_eval", fields(&[("gate", "g2")]))
        .unwrap();

    let state_changes = ledger.events_of_type("state_change");
    assert_eq!(state_changes.len(), 2);
    assert_eq!(state_changes[0].fields.get("to").unwrap(), "a");
    assert_eq!(state_changes[1].fields.get("to").unwrap(), "b");

    let genesis_events = ledger.events_of_type("genesis");
    assert_eq!(genesis_events.len(), 1);
}

// ---- 8. Tail ----

#[test]
fn test_tail() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.jsonl");

    let mut ledger = Ledger::init(&path, "test-proto", "1.0.0").unwrap();
    ledger.append("event_a", BTreeMap::new()).unwrap();
    ledger.append("event_b", BTreeMap::new()).unwrap();
    ledger.append("event_c", BTreeMap::new()).unwrap();

    // Tail 2 should give last 2 entries
    let last2 = ledger.tail(2);
    assert_eq!(last2.len(), 2);
    assert_eq!(last2[0].event_type, "event_b");
    assert_eq!(last2[1].event_type, "event_c");

    // Tail larger than ledger returns all
    let all = ledger.tail(100);
    assert_eq!(all.len(), 4);

    // Tail 0
    let none = ledger.tail(0);
    assert_eq!(none.len(), 0);
}

// ---- 9. Reload fixes external append ----

#[test]
fn test_reload_fixes_external_append() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.jsonl");

    let mut ledger = Ledger::init(&path, "test-proto", "1.0.0").unwrap();
    ledger.append("event_a", BTreeMap::new()).unwrap();

    // Simulate an external process appending a valid entry
    let last_hash = ledger.last_hash();
    let external_entry = sahjhan::ledger::entry::LedgerEntry::new(
        2,
        last_hash,
        "external_event",
        &ledger.entries()[0].engine,
        &ledger.entries()[0].protocol,
        fields(&[("source", "external")]),
    );

    // Append directly to file
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(file, "{}", external_entry.to_jsonl()).unwrap();
    drop(file);

    // Before reload, ledger only knows about 2 entries
    assert_eq!(ledger.len(), 2);

    // After reload, it picks up the external entry
    ledger.reload().unwrap();
    assert_eq!(ledger.len(), 3);
    assert_eq!(ledger.entries()[2].event_type, "external_event");
    ledger.verify().unwrap();
}

// ---- 10. External append causes stale chain ----

#[test]
fn test_external_append_handled_by_lock_and_reread() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("test.jsonl");

    let mut ledger = Ledger::init(&path, "test-proto", "1.0.0").unwrap();

    // Simulate external process appending a valid entry
    let last_hash = ledger.last_hash();
    let external_entry = sahjhan::ledger::entry::LedgerEntry::new(
        1,
        last_hash.clone(),
        "external_event",
        &ledger.entries()[0].engine,
        &ledger.entries()[0].protocol,
        BTreeMap::new(),
    );

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(file, "{}", external_entry.to_jsonl()).unwrap();
    drop(file);

    // Ledger's in-memory state is stale (doesn't know about external entry).
    // append() re-reads the file under the lock, so it correctly discovers
    // the external entry and chains after it (issue #21 fix).
    ledger.append("our_event", BTreeMap::new()).unwrap();

    // In-memory state should now reflect all 3 entries
    assert_eq!(ledger.len(), 3, "genesis + external + our_event");
    assert_eq!(ledger.entries()[1].event_type, "external_event");
    assert_eq!(ledger.entries()[2].event_type, "our_event");
    assert_eq!(ledger.entries()[2].seq, 2);

    // Chain should verify cleanly
    ledger.verify().unwrap();
}
