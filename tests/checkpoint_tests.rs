// checkpoint_tests.rs — Tests for Ledger checkpoint methods (Task 8)

use sahjhan::ledger::chain::Ledger;
use std::collections::BTreeMap;
use tempfile::tempdir;

fn fields(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

// ---- 1. write_checkpoint produces a _checkpoint entry ----

#[test]
fn test_explicit_checkpoint() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ledger.jsonl");

    let mut ledger = Ledger::init(&path, "test-proto", "1.0.0").unwrap();
    ledger.write_checkpoint("phase-1", "after-init").unwrap();

    // Should now have genesis + checkpoint = 2 entries
    assert_eq!(ledger.len(), 2);

    let cp = &ledger.entries()[1];
    assert_eq!(cp.event_type, "_checkpoint");
    assert_eq!(cp.fields.get("scope").unwrap(), "phase-1");
    assert_eq!(cp.fields.get("snapshot").unwrap(), "after-init");

    // Chain integrity must hold
    ledger.verify().unwrap();
}

// ---- 2. find_latest_checkpoint returns post-checkpoint slice ----

#[test]
fn test_find_latest_checkpoint() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ledger.jsonl");

    let mut ledger = Ledger::init(&path, "test-proto", "1.0.0").unwrap();

    // Events before checkpoint
    ledger
        .append("finding", fields(&[("id", "PRE-1")]))
        .unwrap();
    ledger
        .append("finding", fields(&[("id", "PRE-2")]))
        .unwrap();

    // Write checkpoint
    let cp_seq = {
        ledger.write_checkpoint("audit", "v1").unwrap();
        ledger.entries().last().unwrap().seq
    };

    // Events after checkpoint
    ledger
        .append("finding", fields(&[("id", "POST-1")]))
        .unwrap();
    ledger
        .append("finding", fields(&[("id", "POST-2")]))
        .unwrap();

    let result = ledger.find_latest_checkpoint("audit");
    assert!(result.is_some(), "should find the checkpoint");

    let (seq, after) = result.unwrap();
    assert_eq!(
        seq, cp_seq,
        "returned seq should match the checkpoint entry"
    );

    // The slice should only contain the two post-checkpoint events
    assert_eq!(after.len(), 2);
    assert_eq!(after[0].fields.get("id").unwrap(), "POST-1");
    assert_eq!(after[1].fields.get("id").unwrap(), "POST-2");
}

// ---- 3. find_latest_checkpoint returns None when no checkpoints exist ----

#[test]
fn test_find_checkpoint_none() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ledger.jsonl");

    let mut ledger = Ledger::init(&path, "test-proto", "1.0.0").unwrap();
    ledger.append("finding", fields(&[("id", "X-1")])).unwrap();

    let result = ledger.find_latest_checkpoint("any-scope");
    assert!(
        result.is_none(),
        "should return None when no checkpoints exist"
    );
}

// ---- 4. find_latest_checkpoint is scoped — different scopes don't interfere ----

#[test]
fn test_checkpoint_scope_isolation() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ledger.jsonl");

    let mut ledger = Ledger::init(&path, "test-proto", "1.0.0").unwrap();

    ledger.write_checkpoint("scope-a", "snap-a").unwrap();
    ledger.append("event_x", BTreeMap::new()).unwrap();
    ledger.write_checkpoint("scope-b", "snap-b").unwrap();
    ledger.append("event_y", BTreeMap::new()).unwrap();

    // scope-a checkpoint comes before scope-b, so after it there are 3 entries
    // (event_x, checkpoint-scope-b, event_y)
    let (_, after_a) = ledger.find_latest_checkpoint("scope-a").unwrap();
    assert_eq!(after_a.len(), 3);

    // scope-b checkpoint comes after scope-a, so after it there is 1 entry (event_y)
    let (_, after_b) = ledger.find_latest_checkpoint("scope-b").unwrap();
    assert_eq!(after_b.len(), 1);
    assert_eq!(after_b[0].event_type, "event_y");

    // A scope with no checkpoint returns None
    let result = ledger.find_latest_checkpoint("scope-c");
    assert!(result.is_none());
}

// ---- 5. find_latest_checkpoint returns the LATEST when multiple exist ----

#[test]
fn test_find_latest_checkpoint_picks_last() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ledger.jsonl");

    let mut ledger = Ledger::init(&path, "test-proto", "1.0.0").unwrap();

    // First checkpoint
    ledger.write_checkpoint("phase", "v1").unwrap();
    ledger.append("event_a", BTreeMap::new()).unwrap();

    // Second checkpoint — same scope
    ledger.write_checkpoint("phase", "v2").unwrap();
    ledger.append("event_b", BTreeMap::new()).unwrap();
    ledger.append("event_c", BTreeMap::new()).unwrap();

    // Should return the v2 checkpoint, so only event_b and event_c follow it
    let (_, after) = ledger.find_latest_checkpoint("phase").unwrap();
    assert_eq!(after.len(), 2);
    assert_eq!(after[0].event_type, "event_b");
    assert_eq!(after[1].event_type, "event_c");
}
