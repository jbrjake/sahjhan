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
status: done
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
- 2026-10-23T20:29Z handoff by claude (458e1a62-9f03-498d-ab4d-d00dde1c6d95)
- 2026-10-23T20:29Z handoff by claude (458e1a62-9f03-498d-ab4d-d00dde1c6d95)
- 2026-10-23T21:00Z handoff by claude (458e1a62-9f03-498d-ab4d-d00dde1c6d95)
- 2026-10-23T22:22Z handoff by claude (91a2d877-ce17-4ab5-9ccb-a31a561b6f73)
- 2026-10-23T22:23Z handoff by claude (91a2d877-ce17-4ab5-9ccb-a31a561b6f73)
- 2026-10-28T16:20Z handoff by claude (91a2d877-ce17-4ab5-9ccb-a31a561b6f73)
- 2026-10-28T16:23Z open→doing — claimed by claude (5b99787e-3e5f-4cac-892c-faec3ba0c708)
- 2026-10-28T16:29Z handoff by claude (5b99787e-3e5f-4cac-892c-faec3ba0c708)
- 2026-10-28T16:29Z handoff by claude (5b99787e-3e5f-4cac-892c-faec3ba0c708)
- 2026-10-28T16:58Z doing→done — verify exit 0 @ 7e08d14+7

## comments
- 2026-10-28T16:58Z [claude (5b99787e-3e5f-4cac-892c-faec3ba0c708)] v0.28.3 is released: https://github.com/jbrjake/sahjhan/releases/tag/v0.28.3 (published 2026-10-28T16:54Z, run 40726982776 green: tests on ubuntu and macos, four builds, publish). Tag v0.28.3 on 251f144 (chore: bump version to 0.28.3); main pushed through 7e08d14. Pre-flight: fmt, clippy -D warnings, seven cargo test --all runs at 799 passed / 0 failed / 67 ignored; the earlier 617/1 run was ledger_tests' tampered-hash flake, fixed in 2323a9a (sj-7cn3a1m). Release body set from RELEASE-NOTES-v0.28.3.md, byte-identical (sha256 3666d174…, 4683 bytes). Published aarch64-darwin binary checked against SHA256SUMS and gh attestation verify (release.yml @ refs/tags/v0.28.3, 251f144). The manifest lock is named on sj-1zq2xr5 with the before/after against the published binaries. #33's comment table re-run against the published binary holds row for row; posting it waits on the owner's review, its own task.
