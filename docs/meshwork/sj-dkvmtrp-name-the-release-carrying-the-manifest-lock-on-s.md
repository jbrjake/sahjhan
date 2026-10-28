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
status: doing
created: 2026-10-23T20:24Z
handoff: |
  **v0.28.3 is tagged locally and not pushed.** Pre-flight on 2026-10-28
  was
  clean: fmt, clippy `-D warnings`, and seven `cargo test --all` runs at
  799 passed, 0 failed, 67 ignored. The earlier 617-passed/1-failed run
  was
  `ledger_tests.rs`'s tampered-hash test, a 1-in-256 flake, fixed in
  2323a9a.
  Then `251f144 chore: bump version to 0.28.3` and the annotated
  tag `v0.28.3` on it ("v0.28.3 — manifest verify agrees with concurrent
  writers, and a since anchor on a checkpoint").
  
  `git push origin main && git push origin v0.28.3` was refused by the
  auto-mode
  classifier (creates a public surface), so the owner pushes or allows it.
  If
  main has moved past 251f144 since then, the tag still names the bump
  commit,
  which is the one to release.
  
  After the tag push, in order:
  1. Watch release.yml (`gh run watch`) until the release exists.
  2. `gh release edit v0.28.3 --notes-file RELEASE-NOTES-v0.28.3.md`. The
  file
  is final as the owner left it; do not edit it.
  3. #33: re-run `ISSUE-33-COMMENT.md`'s table against the published
  darwin
  binary, then show it to the owner before posting (not yet reviewed).
  4. holtz: re-run the stress against the published binary. The driver is
  recovered at the session scratchpad's `stress.py`
  (`python3 -I stress.py <bin> <fresh dir> 2000`), and 0.28.2 is in the
  holtz 0.170.16 plugin's `bin/`. Comment on the manifest-lock task in
  `relates:` with the tag, the
  commits, and the before/after. Say that run ledgers under
  `paths.managed`
  are now sealed at their next append, and that a courier killed between
  its
  append and its manifest save leaves that ledger reported modified until
  the next append to it. holtz's verify wants `answered by sahjhan
  v0.28.3`.
  5. Close this.
claimed-by: claude (5b99787e-3e5f-4cac-892c-faec3ba0c708)
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
- 2026-10-28T16:23Z open→doing — claimed by claude (5b99787e-3e5f-4cac-892c-faec3ba0c708)
- 2026-10-28T16:29Z handoff by claude (5b99787e-3e5f-4cac-892c-faec3ba0c708)
- 2026-10-28T16:29Z handoff by claude (5b99787e-3e5f-4cac-892c-faec3ba0c708)
