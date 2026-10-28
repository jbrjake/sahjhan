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
  **Cut v0.28.3 now.** On 2026-10-28 the owner said "ok i'm good with the
  release
  notes finally". `RELEASE-NOTES-v0.28.3.md` at the root is final as they
  left
  it; do not edit it. Follow the release process memory: bump to 0.28.3
  with
  `cargo check` so `Cargo.lock` follows, commit `chore: bump version to
  0.28.3`
  using `git commit -- Cargo.toml Cargo.lock`, create an annotated tag,
  push
  main, then push the tag. Watch release.yml until it is green.
  
  Pre-flight at a316509 (2026-10-28): fmt and clippy `-D warnings` clean.
  One
  `cargo test` run came back 617 passed, 1 failed. A binary stopped early,
  and
  that run's output was not captured, so the failing test is unknown. Six
  reruns
  with `--no-fail-fast` all came back 799 passed, 0 failed. CI runs the
  suite on
  the tag push, so a flake there fails the release. Run the suite a few
  more
  times before tagging. If it fails again, read the failing test's name
  and
  whether it is concurrent_append_tests or another binary.
  
  Main is local and **unpushed**: 36 commits after v0.28.2, with no
  `feat:`.
  They are the manifest lock (49277b9) and the stale-state refusal
  (ba9f236) for
  holtz#ho-0mvwg8q; #33 (f9a017d, 9d9df66, 46636de, which says `Closes
  #33`);
  c4e4ebe (the checkpoint's seal refusal); 798971c (an append seals the
  ledger it
  grew, by the owner's ruling); and 908506d (a since gate's default
  intent).
  
  Once tagged and published:
  1. `gh release edit v0.28.3 --notes-file RELEASE-NOTES-v0.28.3.md`.
  2. ISSUE CLOSURE RULE for #33: `ISSUE-33-COMMENT.md` at the root has not
  been
  reviewed by the owner. Re-run its table against the published darwin
  binary.
  The shapes are the issue's template, plus a `since` gate on `check_done`
  with a checkpoint between two events. Show it to the owner, then post
  it.
  3. Re-run the holtz-shaped stress against the published binary. The
  scenario
  is in the close comment of the manifest-lock task in `relates:`. Then
  comment on that task with the tag, the commits, and the before/after
  against 0.28.2. Tell holtz that run ledgers under `paths.managed` are
  now
  sealed at their next append, and that a courier hook killed between its
  append and its manifest save leaves that ledger reported modified until
  the
  next append to it. The owner cut that point from the notes, so this
  comment
  is where holtz learns it. holtz's verify wants `answered by sahjhan
  v0.28.3`.
  4. Close this.
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
- 2026-10-28T16:20Z handoff by claude (91a2d877-ce17-4ab5-9ccb-a31a561b6f73)
