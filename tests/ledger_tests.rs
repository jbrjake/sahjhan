use sahjhan::ledger::entry::LedgerEntry;

#[test]
fn test_entry_roundtrip() {
    let entry = LedgerEntry::new(
        1,
        [0u8; 32],
        "test_event".to_string(),
        b"test payload".to_vec(),
    );
    let bytes = entry.to_bytes();
    let parsed = LedgerEntry::from_bytes(&bytes).unwrap();
    assert_eq!(parsed.seq, 1);
    assert_eq!(parsed.event_type, "test_event");
    assert_eq!(parsed.payload, b"test payload");
    assert_eq!(entry.entry_hash, parsed.entry_hash);
}

#[test]
fn test_entry_has_magic_bytes() {
    let entry = LedgerEntry::new(0, [0u8; 32], "init".to_string(), vec![]);
    let bytes = entry.to_bytes();
    assert_eq!(&bytes[0..4], b"SAHJ");
}

#[test]
fn test_entry_has_format_version() {
    let entry = LedgerEntry::new(0, [0u8; 32], "init".to_string(), vec![]);
    let bytes = entry.to_bytes();
    assert_eq!(bytes[4], 1); // format version 1
}

#[test]
fn test_from_bytes_rejects_truncated_data() {
    let entry = LedgerEntry::new(0, [0u8; 32], "init".to_string(), vec![]);
    let bytes = entry.to_bytes();
    // Truncate at various points
    assert!(LedgerEntry::from_bytes(&bytes[..3]).is_err()); // before magic complete
    assert!(LedgerEntry::from_bytes(&bytes[..10]).is_err()); // mid-header
    assert!(LedgerEntry::from_bytes(&bytes[..bytes.len() - 1]).is_err()); // missing last byte
}

#[test]
fn test_from_bytes_rejects_empty() {
    assert!(LedgerEntry::from_bytes(&[]).is_err());
}

#[test]
fn test_from_bytes_rejects_bad_magic() {
    let mut bytes = LedgerEntry::new(0, [0u8; 32], "x".to_string(), vec![]).to_bytes();
    bytes[0] = b'X';
    assert!(LedgerEntry::from_bytes(&bytes).is_err());
}

#[test]
fn test_from_bytes_rejects_wrong_version() {
    let mut bytes = LedgerEntry::new(0, [0u8; 32], "x".to_string(), vec![]).to_bytes();
    bytes[4] = 99;
    assert!(LedgerEntry::from_bytes(&bytes).is_err());
}

#[test]
fn test_tampered_payload_detected() {
    let entry = LedgerEntry::new(0, [0u8; 32], "test".to_string(), b"original".to_vec());
    let mut bytes = entry.to_bytes();
    // Tamper with a byte in the payload area
    let payload_start = 4 + 1 + 8 + 8 + 32 + 2 + 4 + 4; // magic+ver+seq+ts+prev_hash+et_len+"test"+pl_len
    if payload_start < bytes.len() - 32 {
        bytes[payload_start] ^= 0xFF;
    }
    assert!(LedgerEntry::from_bytes(&bytes).is_err());
}
