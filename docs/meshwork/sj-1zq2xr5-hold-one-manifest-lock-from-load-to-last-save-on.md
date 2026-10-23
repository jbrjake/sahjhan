---
id: sj-1zq2xr5
title: "Hold one manifest lock from load to last save on every writer, and take it shared in manifest verify"
status: doing
category: manifest/concurrency
answers: holtz#ho-0mvwg8q
verify: run cargo test target=concurrent_append_tests test_concurrent_events_renders_and_verify_agree
docs:
  - CLAUDE.md#manifest--file-integrity-tracking
  - holtz#docs/superpowers/plans/2026-10-22-tqdm-run-1-postmortem.md#f1-concurrent-finding-writes-record-permanent-violations
seq: 10
created: 2026-10-23T19:44Z
claimed-by: claude (458e1a62-9f03-498d-ab4d-d00dde1c6d95)
---

holtz#ho-0mvwg8q (tqdm run-1 postmortem §F1): every manifest load-modify-save in
sahjhan is unlocked, and so is `manifest verify`. The ledger `flock` covers the
append only. Two failures follow, both recorded by holtz as permanent
`protocol_violation`s:

- a verify beside a writer reads between a render's `std::fs::write` and its seal;
- an `event` that renders nothing, but loaded the manifest before another process
  sealed, saves the old hashes back over that seal.

The fix: one exclusive lock, held from a writer's manifest load to its last save
(renders included), and `manifest verify` takes the same lock shared. Every
load-modify-save site the ask lists goes through it: `cli/transition.rs`
(`cmd_event`, `record_and_render`, `cmd_transition`), `cli/authed_event.rs`,
`cli/batch.rs`, `cli/init.rs`, `cli/render.rs`, `cli/status.rs`,
`daemon/mod.rs` `handle_record_event`. A writer whose manifest copy was loaded
outside the lock is the second failure, so the load moves inside it.

Test (in `tests/concurrent_append_tests.rs`, one test target): N concurrent
`sahjhan event` processes against the built binary, some with `on_event` renders
and some without, with `manifest verify` looping beside them. Asserts verify
never reports a mismatch and the final manifest matches every rendered file.
Watch it fail on the unfixed code first.

Docs: CLAUDE.md lookup tables + flow maps for the lock; README only if a claim
there becomes false, and README edits stay uncommitted for the owner's review.
The answer comment on the ask names the release.

## log
- 2026-10-23T19:44Z created
- 2026-10-23T19:46Z open→doing — claimed by claude (458e1a62-9f03-498d-ab4d-d00dde1c6d95)
