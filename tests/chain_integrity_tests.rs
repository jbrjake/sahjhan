// chain_integrity_tests.rs
//
// All tests in this file used the v0.1.x binary ledger API (to_bytes, from_bytes, Vec<u8> payloads).
// They are temporarily stubbed out while the JSONL migration is in progress.
// Task 3 will rewrite chain.rs and these tests for the JSONL format.

use sahjhan::ledger::chain::Ledger;
use tempfile::tempdir;

#[test]
#[ignore = "binary format removed — rewrite in Task 3"]
fn test_genesis_creates_valid_chain() {}

#[test]
#[ignore = "binary format removed — rewrite in Task 3"]
fn test_append_maintains_chain() {}

#[test]
#[ignore = "binary format removed — rewrite in Task 3"]
fn test_verify_detects_tampering() {}

#[test]
#[ignore = "binary format removed — rewrite in Task 3"]
fn test_verify_detects_deletion() {}

#[test]
#[ignore = "binary format removed — rewrite in Task 3"]
fn test_open_existing_ledger() {}

#[test]
#[ignore = "binary format removed — rewrite in Task 3"]
fn test_verify_checks_timestamp_monotonicity() {}

#[test]
#[ignore = "binary format removed — rewrite in Task 3"]
fn test_events_of_type() {}

#[test]
#[ignore = "binary format removed — rewrite in Task 3"]
fn test_external_append_causes_sequence_gap() {}

#[test]
#[ignore = "binary format removed — rewrite in Task 3"]
fn test_reload_fixes_external_append() {}

#[test]
#[ignore = "binary format removed — rewrite in Task 3"]
fn test_tail() {}
