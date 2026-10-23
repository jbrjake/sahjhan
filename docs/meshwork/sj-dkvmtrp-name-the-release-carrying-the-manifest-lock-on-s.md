---
id: sj-dkvmtrp
title: Name the release carrying the manifest lock on sj-1zq2xr5 once it is cut
category: release
seq: 20
discovered-from: sj-1zq2xr5
relates: [sj-1zq2xr5, sj-9a7ret0]
docs:
  - CLAUDE.md#flow-manifest-seal-one-lock-from-load-to-last-save
verify: contains Cargo.toml /^version = "0\.(28\.[3-9]|29\.)/
status: open
created: 2026-10-23T20:24Z
handoff: |
  Waiting on the owner to cut the release. Main is local and **unpushed**.
  After
  v0.28.2 it carries:
  
  - 49277b9 (the manifest lock) and ba9f236 (a transition refused when its
  state
  moved under it). This is the work for holtz#ho-0mvwg8q.
  - f9a017d and 9d9df66: the engine's `_checkpoint` named as written, and
  the
  refusal for a `checkpoint` anchor naming it. 46636de: L5 counts render
  templates' reads, and says `Closes #33`.
  - c4e4ebe: `ledger checkpoint` returns the shared seal refusal.
  - 798971c: an append seals the ledger it grew rather than always the
  default
  one, by the owner's ruling.
  - 908506d: a since gate's default intent no longer says "last
  transition".
  - Their docs commits.
  
  There is no `feat:`, so v0.28.3. The gate at 908506d: fmt, clippy
  `-D warnings`, 799 passed, 0 failed. The 28 `#[ignore]` daemon tests
  (daemon_record_event, config_integrity, daemon_signing) passed at
  798971c.
  Neither of the two later commits touches a daemon path.
  
  Notes: `RELEASE-NOTES-v0.28.3.md` at the root, untracked. The owner
  revised it
  at 17:41 on 2026-10-23, cutting the console transcripts, the timing
  table, the
  Breaking changes table and the Rust API paragraph. Session 91a2d877 then
  inserted sections for the later fixes and touched one of the owner's
  lines (the
  Upgrading opener, which now names the `checkpoint` exception). Every new
  claim
  was driven against the downloaded 0.28.2 darwin binary and a HEAD
  release
  build. The bullet in "`manifest verify` beside other writers" that ends
  in a
  colon is the owner's edit in progress; leave it to them.
  
  Once tagged and published:
  1. Re-run the holtz-shaped stress against the *published* darwin binary.
  The
  scenario is in the close comment of the manifest-lock task in
  `relates:`.
  2. Comment on that task (the answer to holtz#ho-0mvwg8q) with the tag,
  the
  commits, and that re-run as a before/after against 0.28.2. Also tell
  holtz
  that run ledgers under `paths.managed` are now sealed at their next
  append.
  A courier hook killed between its append and its manifest save leaves
  that
  run ledger reported modified until the next append to it, and holtz
  records
  every mismatch as a permanent violation. holtz's verify wants `answered
  by
  sahjhan vX.Y.Z` in its task file.
  3. `ISSUE-33-COMMENT.md` at the root is the #33 resolution comment
  (ISSUE
  CLOSURE RULE). Re-run its table against the published binary, then post
  the
  version the owner leaves. The probe shapes are the issue's template plus
  a
  `since` gate on `check_done` with a checkpoint between two events.
  4. `gh release edit v0.28.3 --notes-file RELEASE-NOTES-v0.28.3.md` with
  the
  version the owner leaves.
  5. Close this.
---
49277b9 (the manifest lock, sj-1zq2xr5) is on main after v0.28.2 and rides in
the next release. Cutting it is the owner's. A range of `fix:` and `docs:`
commits is a patch, so v0.28.3 as the range stands.

holtz#ho-0mvwg8q's verify wants a line `answered by sahjhan vX.Y.Z` in holtz's
own task file, so the version is what holtz is waiting on. Once the release is
cut: `comment sj-1zq2xr5` with the tag, the commits it carries for the ask, and
the before/after re-run against the *published* binary (the scratch stress
script's numbers came from a local release build), then close this.

The verify checks only that the version was bumped past 0.28.2. The comment is
the act, and the store cannot check it.

## log
- 2026-10-23T20:24Z created
- 2026-10-23T20:29Z handoff by claude (458e1a62-9f03-498d-ab4d-d00dde1c6d95)
- 2026-10-23T20:29Z handoff by claude (458e1a62-9f03-498d-ab4d-d00dde1c6d95)
- 2026-10-23T21:00Z handoff by claude (458e1a62-9f03-498d-ab4d-d00dde1c6d95)
- 2026-10-23T22:22Z handoff by claude (91a2d877-ce17-4ab5-9ccb-a31a561b6f73)
- 2026-10-23T22:23Z handoff by claude (91a2d877-ce17-4ab5-9ccb-a31a561b6f73)
