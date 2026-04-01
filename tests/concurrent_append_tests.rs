// concurrent_append_tests.rs — Stress tests for concurrent ledger writes (Issue #21)
//
// Validates that concurrent appends to the same ledger file produce contiguous
// sequence numbers and a valid hash chain.

use sahjhan::ledger::chain::Ledger;
use std::collections::BTreeMap;
use std::sync::{Arc, Barrier};
use std::thread;
use tempfile::tempdir;

fn fields(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Simulate the CLI code path: open ledger from disk, append one event.
/// This is what `sahjhan event` does — each invocation opens the file fresh.
fn open_and_append(path: &std::path::Path, event_type: &str, id: usize) -> Result<(), String> {
    let mut ledger = Ledger::open(path).map_err(|e| format!("open failed ({}): {}", id, e))?;
    ledger
        .append(
            event_type,
            fields(&[("worker", &id.to_string())]),
        )
        .map_err(|e| format!("append failed ({}): {}", id, e))?;
    Ok(())
}

/// Two threads each open the ledger and append one event concurrently.
/// This is the minimal reproduction case from the issue.
#[test]
fn test_concurrent_two_thread_append() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("concurrent.jsonl");

    // Create ledger with genesis
    Ledger::init(&path, "test-proto", "1.0.0").unwrap();

    let path1 = path.clone();
    let path2 = path.clone();

    let barrier = Arc::new(Barrier::new(2));
    let b1 = barrier.clone();
    let b2 = barrier.clone();

    let t1 = thread::spawn(move || {
        b1.wait(); // sync start
        open_and_append(&path1, "test_event", 1)
    });

    let t2 = thread::spawn(move || {
        b2.wait(); // sync start
        open_and_append(&path2, "test_event", 2)
    });

    let r1 = t1.join().unwrap();
    let r2 = t2.join().unwrap();

    // Both appends should succeed
    if let Err(e) = &r1 {
        eprintln!("Thread 1 error: {}", e);
    }
    if let Err(e) = &r2 {
        eprintln!("Thread 2 error: {}", e);
    }

    // The real check: can we open and verify the ledger?
    let result = Ledger::open(&path);
    match result {
        Ok(ledger) => {
            assert_eq!(
                ledger.len(),
                3,
                "Expected genesis + 2 events, got {}",
                ledger.len()
            );
            // Verify contiguous sequence numbers
            for (i, entry) in ledger.entries().iter().enumerate() {
                assert_eq!(
                    entry.seq,
                    i as u64,
                    "Sequence gap: expected {}, got {}",
                    i,
                    entry.seq
                );
            }
        }
        Err(e) => {
            panic!(
                "Ledger corrupted after concurrent writes: {}. \
                 This confirms issue #21 — concurrent appends produce \
                 sequence gaps or chain breaks.",
                e
            );
        }
    }
}

/// Stress test: N threads each open-and-append to the same ledger.
/// Runs multiple iterations to increase the chance of hitting the race window.
#[test]
fn test_concurrent_stress_many_threads() {
    let iterations = 20;
    let threads_per_iter = 5;
    let mut failures = 0;

    for iter in 0..iterations {
        let dir = tempdir().unwrap();
        let path = dir.path().join(format!("stress_{}.jsonl", iter));

        Ledger::init(&path, "test-proto", "1.0.0").unwrap();

        let barrier = Arc::new(Barrier::new(threads_per_iter));
        let handles: Vec<_> = (0..threads_per_iter)
            .map(|i| {
                let p = path.clone();
                let b = barrier.clone();
                thread::spawn(move || {
                    b.wait();
                    open_and_append(&p, "stress_event", i)
                })
            })
            .collect();

        let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
        let append_errors: Vec<_> = results.iter().filter(|r| r.is_err()).collect();

        match Ledger::open(&path) {
            Ok(ledger) => {
                let expected = 1 + threads_per_iter - append_errors.len();
                if ledger.len() != expected {
                    eprintln!(
                        "Iter {}: expected {} entries, got {} (append errors: {})",
                        iter,
                        expected,
                        ledger.len(),
                        append_errors.len()
                    );
                    failures += 1;
                }
                // Verify sequence contiguity
                for (i, entry) in ledger.entries().iter().enumerate() {
                    if entry.seq != i as u64 {
                        eprintln!(
                            "Iter {}: sequence gap at position {}: expected {}, got {}",
                            iter, i, i, entry.seq
                        );
                        failures += 1;
                        break;
                    }
                }
            }
            Err(e) => {
                eprintln!("Iter {}: ledger open failed: {}", iter, e);
                failures += 1;
            }
        }
    }

    if failures > 0 {
        panic!(
            "{} of {} iterations produced corrupted ledgers. \
             This confirms issue #21 — the lock critical section does not \
             cover seq assignment.",
            failures, iterations
        );
    }
}

/// Rapid sequential open-append cycles from a single thread (baseline sanity check).
/// This should always pass since there's no concurrency.
#[test]
fn test_sequential_rapid_appends_no_corruption() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("sequential.jsonl");

    Ledger::init(&path, "test-proto", "1.0.0").unwrap();

    for i in 0..50 {
        open_and_append(&path, "seq_event", i).unwrap();
    }

    let ledger = Ledger::open(&path).unwrap();
    assert_eq!(ledger.len(), 51); // genesis + 50 events
    for (i, entry) in ledger.entries().iter().enumerate() {
        assert_eq!(entry.seq, i as u64);
    }
}
