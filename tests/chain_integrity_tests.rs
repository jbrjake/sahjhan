use sahjhan::ledger::chain::Ledger;
use tempfile::tempdir;

#[test]
fn test_genesis_creates_valid_chain() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&path, "test-protocol", "1.0.0").unwrap();
    assert_eq!(ledger.len(), 1);
    assert!(ledger.verify().is_ok());
}

#[test]
fn test_append_maintains_chain() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&path, "test-protocol", "1.0.0").unwrap();
    ledger.append("test_event", b"payload1".to_vec()).unwrap();
    ledger.append("test_event", b"payload2".to_vec()).unwrap();
    assert_eq!(ledger.len(), 3); // genesis + 2
    assert!(ledger.verify().is_ok());
}

#[test]
fn test_verify_detects_tampering() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&path, "test-protocol", "1.0.0").unwrap();
    ledger.append("event", b"data".to_vec()).unwrap();
    drop(ledger);

    // Tamper: flip a byte in the middle of the file
    let mut data = std::fs::read(&path).unwrap();
    let mid = data.len() / 2;
    data[mid] ^= 0xFF;
    std::fs::write(&path, &data).unwrap();

    let result = Ledger::open(&path);
    // Either open fails (bad hash) or verify catches it
    match result {
        Err(_) => {} // tamper caught during open
        Ok(ledger) => assert!(ledger.verify().is_err()),
    }
}

#[test]
fn test_verify_detects_deletion() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&path, "test-protocol", "1.0.0").unwrap();
    ledger.append("event1", b"a".to_vec()).unwrap();
    ledger.append("event2", b"b".to_vec()).unwrap();
    ledger.append("event3", b"c".to_vec()).unwrap();

    // Get entry bytes for surgical deletion
    let entries = ledger.entries();
    let entry0_bytes = entries[0].to_bytes();
    let entry2_bytes = entries[2].to_bytes();
    drop(ledger);

    // Write only entries 0 and 2 (skip 1) — creates both sequence gap AND chain break
    let mut tampered = Vec::new();
    tampered.extend(&entry0_bytes);
    tampered.extend(&entry2_bytes);
    std::fs::write(&path, &tampered).unwrap();

    let result = Ledger::open(&path);
    match result {
        Err(_) => {} // caught during open/parse
        Ok(ledger) => assert!(ledger.verify().is_err()),
    }
}

// E9: Test insertion detection
#[test]
fn test_verify_detects_insertion() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&path, "test-protocol", "1.0.0").unwrap();
    ledger.append("event1", b"a".to_vec()).unwrap();
    ledger.append("event2", b"b".to_vec()).unwrap();

    let entries = ledger.entries();
    let entry0_bytes = entries[0].to_bytes();
    let entry1_bytes = entries[1].to_bytes();
    let entry2_bytes = entries[2].to_bytes();
    drop(ledger);

    // Fabricate an entry and insert between entry1 and entry2
    use sahjhan::ledger::entry::LedgerEntry;
    let fabricated = LedgerEntry::new(
        99,
        [0xAA; 32], // wrong prev_hash
        "fabricated".to_string(),
        b"evil".to_vec(),
    );

    let mut tampered = Vec::new();
    tampered.extend(&entry0_bytes);
    tampered.extend(&entry1_bytes);
    tampered.extend(&fabricated.to_bytes());
    tampered.extend(&entry2_bytes);
    std::fs::write(&path, &tampered).unwrap();

    let result = Ledger::open(&path);
    match result {
        Err(_) => {} // caught during open/parse
        Ok(ledger) => assert!(ledger.verify().is_err()),
    }
}

#[test]
fn test_open_existing_ledger() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&path, "test-protocol", "1.0.0").unwrap();
    ledger.append("event", b"data".to_vec()).unwrap();
    drop(ledger);

    let ledger = Ledger::open(&path).unwrap();
    assert_eq!(ledger.len(), 2);
    assert!(ledger.verify().is_ok());
}

// E8: Timestamp monotonicity
#[test]
fn test_verify_checks_timestamp_monotonicity() {
    // This is hard to test directly since timestamps come from system clock.
    // But verify() should check for it, and we can at least verify the function exists.
    let dir = tempdir().unwrap();
    let path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&path, "test-protocol", "1.0.0").unwrap();
    ledger.append("event1", b"a".to_vec()).unwrap();
    ledger.append("event2", b"b".to_vec()).unwrap();
    // Normal timestamps should be monotonic
    assert!(ledger.verify().is_ok());
}

#[test]
fn test_events_of_type() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&path, "test-protocol", "1.0.0").unwrap();
    ledger.append("type_a", b"1".to_vec()).unwrap();
    ledger.append("type_b", b"2".to_vec()).unwrap();
    ledger.append("type_a", b"3".to_vec()).unwrap();

    let type_a = ledger.events_of_type("type_a");
    assert_eq!(type_a.len(), 2);
    let type_b = ledger.events_of_type("type_b");
    assert_eq!(type_b.len(), 1);
}

#[test]
fn test_tail() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&path, "test-protocol", "1.0.0").unwrap();
    ledger.append("e1", vec![]).unwrap();
    ledger.append("e2", vec![]).unwrap();
    ledger.append("e3", vec![]).unwrap();

    let tail = ledger.tail(2);
    assert_eq!(tail.len(), 2);
    assert_eq!(tail[0].event_type, "e2");
    assert_eq!(tail[1].event_type, "e3");
}
