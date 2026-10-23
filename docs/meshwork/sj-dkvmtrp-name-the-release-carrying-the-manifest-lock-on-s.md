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
  Waiting on the owner to cut the release. Main, local and **unpushed**,
  carries
  after v0.28.2: 49277b9 (the manifest lock) and ba9f236 (a transition
  refused
  when its state moved under it), with their docs commits 45eaf1d and
  03a8f4a.
  All `fix:`/`docs:`, so v0.28.3 by the release process. Gate green at
  2c44673:
  fmt, clippy `-D warnings`, 792 tests, and the 28 `#[ignore]` daemon
  tests in
  daemon_record_event / config_integrity / daemon_signing.
  
  Once tagged and published:
  1. Re-run the holtz-shaped stress against the *published* darwin binary.
  The
  script is not in the repo; the scenario is in the answer's close comment
  (the task in `relates:`): a 2,000-entry ledger, 21 parallel `finding`s
  each
  re-rendering three files, 120 `ping`s, two `manifest verify` loops.
  Those
  numbers came from a local release build.
  2. Comment on the answer with the tag, the commits, and that re-run as a
  before/after against 0.28.2. holtz#ho-0mvwg8q's own verify wants
  `answered by sahjhan vX.Y.Z` in holtz's task file, so the version is the
  thing holtz is waiting on.
  3. Release notes in the v0.25.0 style. Behavior a consumer can notice:
  the 30s
  lock timeout (exit 2, `Cannot lock manifest: timed out…`); verify
  waits for
  writers; renders and manifest.json replaced by rename (hidden
  `.<name>.<pid>.tmp`, new inode, umask mode); a hook-eval auto-record
  with an
  unloadable manifest skipped with a warning; a transition whose manifest
  will
  not load fails after its gates; a transition whose state moved under it
  refuses with exit 1. The answer's close comment has the full list.
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
