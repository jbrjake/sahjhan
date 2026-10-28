---
id: sj-7cn3a1m
title: Make the tampered-hash ledger test tamper on every run
status: done
category: tests/flake
relates: [sj-dkvmtrp]
verify: "all(lacks tests/ledger_tests.rs /\\{\\}ff/, run cargo test target=ledger_tests test_from_jsonl_rejects_tampered_hash)"
docs:
  - CLAUDE.md#ledger--append-only-hash-chained-event-log
seq: 15
created: 2026-10-28T16:26Z
---

The v0.28.3 pre-flight at a316509 saw one `cargo test` come back 617 passed,
1 failed, 65 ignored, with the output lost. Accumulating per-binary counts in
run order from a clean run (cargo stops after the first failing binary without
`--no-fail-fast`) puts exactly 618 run and 65 ignored at the end of
`tests/ledger_tests.rs`, and at no other binary.

In it, `test_from_jsonl_rejects_tampered_hash` says it flips the hash's last
hex char, but overwrites the last two with a fixed `ff`. The entry comes from
`LedgerEntry::new`, which stamps the wall clock, so the hash differs every
run; when it already ends in `ff` (1 run in 256) the "tampered" line is the
original, `from_jsonl` accepts it, and `unwrap_err()` panics. CI runs the
suite on the tag push, so this can fail a release.

Fix: flip the last char to one it is not, and assert the tamper applied (the
idiom `test_recorded_by_is_covered_by_the_hash` already uses). Red check: an
entry whose hash ends in `ff`, found by `new_with_ts`, passes the old tamper
straight through.

Swept: the other tamper sites (`chain_integrity_tests.rs` a→b or a grown
hash, `recorded_by` rewrite with its applied-assert, config files appended to)
all differ by construction.

## log
- 2026-10-28T16:26Z created
- 2026-10-28T16:26Z open→doing — claimed by claude (5b99787e-3e5f-4cac-892c-faec3ba0c708)
- 2026-10-28T16:27Z doing→done — verify exit 0 @ 2323a9a+7

## comments
- 2026-10-28T16:27Z [claude (5b99787e-3e5f-4cac-892c-faec3ba0c708)] Fixed in 2323a9a, riding in v0.28.3. Red check: a probe built an entry with new_with_ts at 2026-10-28T12:00:00.224Z (hash …9d24ff); the old tamper left the line unchanged and unwrap_err panicked on Ok, the lost pre-flight failure. With the flip, the same entry's line changes and is rejected; a hash ending in 0 (…f7e420 at 12:00:00.000Z) flips to 1 and is rejected too. Probe removed. Before the fix, six cargo test --all --no-fail-fast runs this session were 799 passed, 0 failed, 67 ignored each, consistent with a 1-in-256 flake.
