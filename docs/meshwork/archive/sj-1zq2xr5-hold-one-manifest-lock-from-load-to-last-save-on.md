---
id: sj-1zq2xr5
title: "Hold one manifest lock from load to last save on every writer, and take it shared in manifest verify"
status: done
category: manifest/concurrency
answers: holtz#ho-0mvwg8q
verify: run cargo test target=concurrent_append_tests test_concurrent_events_renders_and_verify_agree
docs:
  - CLAUDE.md#manifest--file-integrity-tracking
  - holtz#docs/superpowers/plans/2026-10-22-tqdm-run-1-postmortem.md#f1-concurrent-finding-writes-record-permanent-violations
seq: 10
created: 2026-10-23T19:44Z
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
- 2026-10-23T20:22Z doing→done — verify exit 0 @ 45eaf1d+4

## comments
- 2026-10-23T20:22Z [claude (458e1a62-9f03-498d-ab4d-d00dde1c6d95)] Answer to holtz#ho-0mvwg8q. Fixed in 49277b9 (`fix(manifest): one lock from a
  writer's manifest load to its last save`), navigation map in 45eaf1d. **Not yet
  in a release**: the release is the owner's to cut, and the follow-up that names
  it on this ask is filed beside this task.
  
  **What changed.** One exclusive lock (`flock` on the data dir itself, so no new
  file appears there), held by every writer from its manifest load through the
  ledger append, the seal, and every render the event triggers: `event`,
  `authed-event`, the daemon's `record_event`, `set complete`, `transition`,
  `batch`, `render`, `init`, `reset`. `manifest verify` (and `list`, `restore`,
  `status`) take it shared, and verify holds it across every hash. A transition's
  gates and emit commands run *before* the lock is taken, because a gate that runs
  `sahjhan event` would otherwise wait on its own parent. The CLI's manifest
  load/save take the lock as an argument, so a load outside it does not compile.
  
  Three more fixes surfaced on the way:
  - `hook eval`'s auto-record, `ledger checkpoint` and `reseal` appended without
    re-sealing the ledger, so the next verify reported `ledger.jsonl` modified
    until some later `event` re-sealed it. They seal it now.
  - `manifest.json` and rendered files were written in place, and a writer read a
    half-written `manifest.json` under load (below). Both are now replaced by
    rename.
  - Every event built the full render context (every ledger entry) even when no
    render matched. It is built only when one does.
  
  **Before / after**, holtz's shape: a 2,000-entry ledger, a burst of 21 parallel
  `finding`s each re-rendering three files plus 120 courier `ping`s, and two
  `manifest verify` loops beside them. Shipped 0.28.2 (from the holtz 0.170.16
  plugin's `bin/`) vs a release build of 49277b9:
  
  | | 0.28.2 | 49277b9 |
  |---|---|---|
  | verifies failing beside the burst | 229 of 300 | 0 of 24 |
  | paths in those failures | the three renders (211 each), ledger (61) | none |
  | final verify after the burst | exit 2 | exit 0 |
  | writers failing | 1 (`invalid manifest JSON … EOF`) | 0 |
  | writer latency p50 / max | 0.13s / 1.15s | 0.26s / 1.16s |
  
  The 0.28.2 final verify failing after every writer has stopped is your "can
  never converge": a stale seal saved over a fresh one.
  `tests/concurrent_append_tests.rs` `test_concurrent_events_renders_and_verify_agree`
  is the test the ask specified (N writers, half rendering, verify looping; no
  verify fails; the final manifest seals every rendered file). Red on the unfixed
  tree with 35 of 60 verifies failing. With verify's lock removed, or with renders
  moved outside the writer's lock, it fails with exactly your triple
  (`PUNCHLIST-ARCHIVE.md`, `PUNCHLIST.md`, `STATUS.md`).
  
  **Behavior a consumer can notice. No config changes.**
  - A writer waits up to 30s for the lock, then fails with exit 2:
    `Cannot lock manifest: timed out after 30s waiting for the manifest lock on <data_dir>`.
    Under the load above the longest wait was 1.16s.
  - `manifest verify` now waits for any writer mid-event. That wait is the fix,
    not a slowdown to work around.
  - A `hook eval` auto-record whose manifest will not load is skipped with
    `Warning: auto-record failed: …`, the same handling an append failure already
    had. A `reseal` whose manifest will not load still reseals, and warns that the
    ledger's entry is stale.
  - Rendered files and `manifest.json` are replaced by rename. A hidden
    `.<name>.<pid>.tmp` sibling exists for the length of one write, and the file
    is a new inode each time, with the mode from the umask.
  - A transition whose manifest does not load now fails after its gates run,
    rather than before.
  
  **For downstream consumers (holtz).** The engine half of F1 is closed. Parallel
  `sahjhan event finding` calls, from one agent or ten, no longer open a window.
  Neither do the couriers' per-tool-call events. A single `manifest verify` sample
  is now a consistent one. Still the plugin's: `_recorded_violations`'s
  check-then-act dedupe (`bash_guard.py:105`, the identical-hash pairs at seq
  1935–1940). Two hooks can still both query before either writes.
  
  **Not verified.** A real holtz audit on this build. Linux: `flock` on a
  directory fd is standard there, but the tests above ran on macOS only, and CI's
  ubuntu leg runs at release.
