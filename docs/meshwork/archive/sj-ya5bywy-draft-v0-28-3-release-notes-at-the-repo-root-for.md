---
id: sj-ya5bywy
title: Draft v0.28.3 release notes at the repo root for the owner's review
status: done
category: release
relates: [sj-dkvmtrp]
verify: contains RELEASE-NOTES-v0.28.3.md /## Upgrading/
docs:
  - CLAUDE.md#flow-manifest-seal-one-lock-from-load-to-last-save
seq: 15
created: 2026-10-23T20:53Z
---

The owner asked, this session, for release notes drafted and left uncommitted for review. Draft from `git log v0.28.2..HEAD` (49277b9 the manifest lock, ba9f236 a transition refused when its state moved, docs 9dbd149 / 45eaf1d / 03a8f4a), in the v0.25.0 style: `gh release view v0.25.0` first. Written to `RELEASE-NOTES-v0.28.3.md` at the repo root, untracked. The owner edits it, and it is posted at the release (sj-dkvmtrp). Behavior claims in it are checked against the built binary, not recalled.

## log
- 2026-10-23T20:53Z created
- 2026-10-23T20:55Z open→doing — claimed by claude (458e1a62-9f03-498d-ab4d-d00dde1c6d95)
- 2026-10-23T21:00Z doing→done — verify exit 0 @ 80de674+6

## comments
- 2026-10-23T21:00Z [claude (458e1a62-9f03-498d-ab4d-d00dde1c6d95)] Drafted RELEASE-NOTES-v0.28.3.md at the repo root, untracked, for the owner's review. Range v0.28.2..80de674: 49277b9, ba9f236, docs. Each transcript was run on the shipped 0.28.2 (holtz 0.170.16 plugin bin/) and a HEAD release build. The stress table is ranges over two or three runs of each, not one run.
