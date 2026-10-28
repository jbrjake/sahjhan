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
- 2026-10-28T16:57Z [claude (5b99787e-3e5f-4cac-892c-faec3ba0c708)] **Released in sahjhan v0.28.3** (https://github.com/jbrjake/sahjhan/releases/tag/v0.28.3). This supersedes the comment above, which predates the release. For holtz#ho-0mvwg8q, the release carries 49277b9 (one manifest lock from a writer's load to its last save, with verify holding it shared), ba9f236 (a transition whose state moved while its gates ran is refused), and 798971c (an append seals the ledger it grew). Docs: 45eaf1d, 03a8f4a, 4d52ecf, 09497f9, d5cc97d.
  
  **Before / after against the published binaries.** 0.28.2 is from the holtz 0.170.16 plugin's `bin/`. 0.28.3 is the release's `sahjhan-aarch64-apple-darwin`, checked against `SHA256SUMS` and its build attestation (release.yml at `refs/tags/v0.28.3`, 251f144). The scenario is the same as before: a 2,000-entry ledger, a burst of 21 parallel `finding`s each re-rendering three files plus 120 courier `ping`s, and two `manifest verify` loops beside them.
  
  | | 0.28.2 | 0.28.3 |
  |---|---|---|
  | verifies failing beside the burst | 239 of 319 | 0 of 22 |
  | paths in those failures | the three renders (220 each), ledger (51) | none |
  | final verify after the burst | exit 2 | exit 0 |
  | writers failing | 0 | 0 |
  | writer latency p50 / max | 0.03s / 1.44s | 0.22s / 1.08s |
  
  Fewer verifies complete under 0.28.3 because each one waits for the writers holding the lock. That wait is the fix. The Linux question the earlier comment left open is settled. The `flock` is on the data dir's fd, and on the release's ubuntu test leg all nine `concurrent_append_tests` passed, `test_concurrent_events_renders_and_verify_agree` among them.
  
  **New for holtz: run ledgers are in the manifest.** Through 0.28.2, every append re-hashed `data_dir/ledger.jsonl`, whichever ledger it went to. A run ledger was never tracked. 0.28.3 seals the ledger the append grew, under its project-root key, if it sits under `paths.managed`. A ledger outside every managed path stays untracked. A named run ledger at `output/runs/1/ledger.jsonl`, against both binaries:
  
  | | 0.28.2 | 0.28.3 |
  |---|---|---|
  | tracked after two appends | no (`1 tracked`, the default ledger) | yes (`2 tracked`) |
  | cut short by its last entry (`log verify` still says `chain valid`) | `manifest clean`, exit 0 | `output/runs/1/ledger.jsonl — modified`, exit 2 |
  
  **The cost, by the owner's ruling on sj-tz2yren.** A writer killed between its append and its manifest save leaves that ledger's hash stale. `manifest verify` then reports the ledger modified until the next append to it reseals it. Claude Code kills a hook that overruns its timeout, and holtz's couriers are hooks, so every run ledger carries that exposure. I simulated it on 0.28.3 by putting back the manifest from before an append. Verify reported `output/runs/1/ledger.jsonl — modified`, exit 2. The next `event ping --ledger run1` resealed it, and verify was clean. Holtz records verify mismatches as permanent `protocol_violation`s, so it should decide whether a mismatch on a run ledger, alone and followed by a clean verify after the next append, is the same finding. The release notes leave this point out, so this comment is where holtz learns it.
  
  **Behavior a consumer can notice. No config changes.**
  - A writer waits up to 30s for the lock, then fails with exit 2, `Cannot lock manifest: timed out after 30s …`. Under the burst above the longest wait was 1.08s.
  - A transition whose state moved between its gate check and its commit, because of another actor or one of its own gate commands, is refused with exit 1 and records nothing. Running it again checks its gates where the machine now is.
  - `hook eval`'s `auto_record`, `ledger checkpoint` and `reseal` now seal the ledger they grow, so they no longer leave it looking modified.
  
  **For holtz.** Bundle the 0.28.3 binary in the plugin's `bin/` (0.170.16 ships 0.28.2). The engine half of F1 is closed: parallel `finding`s and courier pings no longer open a window, and one `manifest verify` sample is a consistent one. Still the plugin's: `_recorded_violations`'s check-then-act dedupe (`bash_guard.py:105`). holtz#ho-0mvwg8q's verify wants `- 2026-10-28 answered by sahjhan v0.28.3` in holtz's own task file. A holtz session writes that line and closes the ask.
  
  **Not verified.** A real holtz audit on 0.28.3.
