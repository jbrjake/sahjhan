use sahjhan::ledger::entry::{LedgerEntry, SCHEMA_VERSION};
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Core round-trip and hashing
// ---------------------------------------------------------------------------

#[test]
fn test_jsonl_round_trip() {
    let mut fields = BTreeMap::new();
    fields.insert("id".to_string(), "BH-001".to_string());
    fields.insert("severity".to_string(), "HIGH".to_string());

    let entry = LedgerEntry::new(
        0,
        "abc123".to_string(),
        "finding",
        "sahjhan/0.2.0",
        "holtz/1.0.0",
        fields.clone(),
    );

    let jsonl_line = entry.to_jsonl();
    let parsed = LedgerEntry::from_jsonl(&jsonl_line).unwrap();

    assert_eq!(parsed.seq, 0);
    assert_eq!(parsed.event_type, "finding");
    assert_eq!(parsed.fields, fields);
    assert_eq!(parsed.hash, entry.hash);
    assert_eq!(parsed.prev, entry.prev);
}

#[test]
fn test_hash_excludes_hash_field() {
    let mut fields = BTreeMap::new();
    fields.insert("x".to_string(), "1".to_string());

    let entry = LedgerEntry::new(
        0,
        "0000".to_string(),
        "test",
        "sahjhan/0.2.0",
        "test/1.0.0",
        fields,
    );

    let recomputed = LedgerEntry::compute_hash(
        entry.schema,
        entry.seq,
        &entry.prev,
        &entry.ts,
        &entry.event_type,
        &entry.engine,
        &entry.protocol,
        &entry.fields,
    );
    assert_eq!(entry.hash, recomputed);
}

#[test]
fn test_canonical_json_key_ordering() {
    let mut fields = BTreeMap::new();
    fields.insert("z_last".to_string(), "1".to_string());
    fields.insert("a_first".to_string(), "2".to_string());

    let entry = LedgerEntry::new(
        0,
        "nonce".to_string(),
        "test",
        "sahjhan/0.2.0",
        "test/1.0.0",
        fields,
    );

    let line = entry.to_jsonl();
    let a_pos = line.find("\"a_first\"").unwrap();
    let z_pos = line.find("\"z_last\"").unwrap();
    assert!(a_pos < z_pos, "keys must be sorted alphabetically");
}

#[test]
fn test_hash_chain_linkage() {
    let mut fields = BTreeMap::new();
    fields.insert("x".to_string(), "1".to_string());

    let entry0 = LedgerEntry::new(
        0,
        "genesis_nonce".to_string(),
        "init",
        "sahjhan/0.2.0",
        "test/1.0.0",
        fields.clone(),
    );
    let entry1 = LedgerEntry::new(
        1,
        entry0.hash.clone(),
        "step",
        "sahjhan/0.2.0",
        "test/1.0.0",
        fields,
    );

    assert_eq!(entry1.prev, entry0.hash);
}

// ---------------------------------------------------------------------------
// Top-level key ordering in to_jsonl()
// ---------------------------------------------------------------------------

#[test]
fn test_top_level_key_ordering() {
    let entry = LedgerEntry::new(
        0,
        "prev".to_string(),
        "test",
        "eng",
        "proto",
        BTreeMap::new(),
    );
    let line = entry.to_jsonl();

    // All top-level keys must appear in alphabetical order:
    // engine, fields, hash, prev, protocol, schema, seq, ts, type
    let positions: Vec<usize> = [
        "\"engine\"",
        "\"fields\"",
        "\"hash\"",
        "\"prev\"",
        "\"protocol\"",
        "\"schema\"",
        "\"seq\"",
        "\"ts\"",
        "\"type\"",
    ]
    .iter()
    .map(|k| line.find(k).expect(&format!("key {} missing from JSONL", k)))
    .collect();

    for i in 1..positions.len() {
        assert!(
            positions[i] > positions[i - 1],
            "top-level keys are not in alphabetical order"
        );
    }
}

// ---------------------------------------------------------------------------
// from_jsonl validation
// ---------------------------------------------------------------------------

#[test]
fn test_from_jsonl_rejects_unsupported_schema() {
    let entry = LedgerEntry::new(
        0,
        "prev".to_string(),
        "test",
        "eng",
        "proto",
        BTreeMap::new(),
    );
    // Manually craft JSON with schema > SCHEMA_VERSION
    let line = entry.to_jsonl().replace(
        &format!("\"schema\":{}", SCHEMA_VERSION),
        &format!("\"schema\":{}", SCHEMA_VERSION + 1),
    );
    let err = LedgerEntry::from_jsonl(&line).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("unsupported schema version"),
        "expected UnsupportedVersion, got: {}",
        msg
    );
}

#[test]
fn test_from_jsonl_rejects_tampered_hash() {
    let entry = LedgerEntry::new(
        0,
        "prev".to_string(),
        "test",
        "eng",
        "proto",
        BTreeMap::new(),
    );
    let line = entry.to_jsonl();
    // Tamper: flip last hex char in hash
    let tampered = line.replace(&entry.hash, &format!("{}ff", &entry.hash[..entry.hash.len() - 2]));
    let err = LedgerEntry::from_jsonl(&tampered).unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("hash mismatch"),
        "expected HashMismatch, got: {}",
        msg
    );
}

#[test]
fn test_from_jsonl_rejects_malformed_json() {
    let err = LedgerEntry::from_jsonl("not json at all").unwrap_err();
    let msg = err.to_string();
    assert!(
        msg.contains("JSON parse error"),
        "expected ParseError, got: {}",
        msg
    );
}

// ---------------------------------------------------------------------------
// RFC 8785 edge cases
// ---------------------------------------------------------------------------

#[test]
fn test_empty_fields_map() {
    let entry = LedgerEntry::new(
        0,
        "prev".to_string(),
        "test",
        "eng",
        "proto",
        BTreeMap::new(),
    );

    let line = entry.to_jsonl();
    assert!(
        line.contains("\"fields\":{}"),
        "empty fields should serialize as {{}}; got: {}",
        line
    );

    let parsed = LedgerEntry::from_jsonl(&line).unwrap();
    assert_eq!(parsed.fields.len(), 0);
    assert_eq!(parsed.hash, entry.hash);
}

#[test]
fn test_fields_with_quotes_and_backslashes() {
    let mut fields = BTreeMap::new();
    fields.insert("msg".to_string(), r#"He said "hello\world""#.to_string());

    let entry = LedgerEntry::new(
        0,
        "prev".to_string(),
        "test",
        "eng",
        "proto",
        fields.clone(),
    );

    let line = entry.to_jsonl();
    let parsed = LedgerEntry::from_jsonl(&line).unwrap();
    assert_eq!(parsed.fields, fields);
    assert_eq!(parsed.hash, entry.hash);
}

#[test]
fn test_fields_with_control_characters() {
    let mut fields = BTreeMap::new();
    fields.insert("tab".to_string(), "a\tb".to_string());
    fields.insert("newline".to_string(), "line1\nline2".to_string());
    fields.insert("null_byte".to_string(), "before\0after".to_string());

    let entry = LedgerEntry::new(
        0,
        "prev".to_string(),
        "test",
        "eng",
        "proto",
        fields.clone(),
    );

    let line = entry.to_jsonl();
    // Control chars must be escaped, not raw
    assert!(!line.contains('\t'), "raw tab should be escaped");
    assert!(!line.contains('\n'), "raw newline should be escaped");
    assert!(!line.contains('\0'), "raw null should be escaped");

    let parsed = LedgerEntry::from_jsonl(&line).unwrap();
    assert_eq!(parsed.fields, fields);
    assert_eq!(parsed.hash, entry.hash);
}

#[test]
fn test_empty_string_values() {
    let mut fields = BTreeMap::new();
    fields.insert("empty".to_string(), "".to_string());
    fields.insert("also_empty".to_string(), "".to_string());

    let entry = LedgerEntry::new(
        0,
        "prev".to_string(),
        "test",
        "eng",
        "proto",
        fields.clone(),
    );

    let line = entry.to_jsonl();
    let parsed = LedgerEntry::from_jsonl(&line).unwrap();
    assert_eq!(parsed.fields, fields);
    assert_eq!(parsed.hash, entry.hash);
}

#[test]
fn test_seq_zero_no_leading_zeros() {
    let entry = LedgerEntry::new(
        0,
        "prev".to_string(),
        "test",
        "eng",
        "proto",
        BTreeMap::new(),
    );

    let line = entry.to_jsonl();
    // "seq":0 — not "seq":00 or "seq":"0"
    assert!(
        line.contains("\"seq\":0,") || line.contains("\"seq\":0}"),
        "seq=0 should be bare integer 0, got: {}",
        line
    );
}

#[test]
fn test_forward_slashes_not_escaped() {
    let mut fields = BTreeMap::new();
    fields.insert("path".to_string(), "/usr/local/bin".to_string());

    let entry = LedgerEntry::new(
        0,
        "prev".to_string(),
        "test",
        "sahjhan/0.2.0",
        "test/1.0.0",
        fields,
    );

    let line = entry.to_jsonl();
    // RFC 8785: forward slashes must NOT be escaped
    assert!(
        !line.contains("\\/"),
        "forward slashes should not be escaped; got: {}",
        line
    );
    assert!(line.contains("/usr/local/bin"));
}

// ---------------------------------------------------------------------------
// new_with_ts deterministic construction
// ---------------------------------------------------------------------------

#[test]
fn test_new_with_ts_deterministic() {
    let fields = BTreeMap::new();
    let ts = "2025-01-15T12:00:00.000Z".to_string();

    let a = LedgerEntry::new_with_ts(
        0,
        "prev".to_string(),
        "test",
        "eng",
        "proto",
        fields.clone(),
        ts.clone(),
    );
    let b = LedgerEntry::new_with_ts(
        0,
        "prev".to_string(),
        "test",
        "eng",
        "proto",
        fields,
        ts,
    );

    assert_eq!(a.hash, b.hash, "same inputs must produce same hash");
    assert_eq!(a.to_jsonl(), b.to_jsonl());
}

// ---------------------------------------------------------------------------
// Schema version field
// ---------------------------------------------------------------------------

#[test]
fn test_schema_version_in_output() {
    let entry = LedgerEntry::new(
        0,
        "prev".to_string(),
        "test",
        "eng",
        "proto",
        BTreeMap::new(),
    );
    let line = entry.to_jsonl();
    assert!(
        line.contains(&format!("\"schema\":{}", SCHEMA_VERSION)),
        "schema version {} should appear in JSONL; got: {}",
        SCHEMA_VERSION,
        line
    );
}

// ---------------------------------------------------------------------------
// serde "type" rename
// ---------------------------------------------------------------------------

#[test]
fn test_type_field_rename() {
    let entry = LedgerEntry::new(
        0,
        "prev".to_string(),
        "finding",
        "eng",
        "proto",
        BTreeMap::new(),
    );
    let line = entry.to_jsonl();
    // JSON key should be "type", not "event_type"
    assert!(
        line.contains("\"type\":\"finding\""),
        "event_type should serialize as 'type'; got: {}",
        line
    );
    assert!(
        !line.contains("\"event_type\""),
        "raw field name 'event_type' should not appear in JSON"
    );
}
