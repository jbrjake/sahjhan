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
