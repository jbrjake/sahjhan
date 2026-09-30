---
id: sj-2c19dfs
title: Name the release carrying the lint-demo and init fixes on sj-rgpc2qt once it is cut
status: open
category: release
discovered-from: sj-rgpc2qt
relates: [sj-rgpc2qt, sj-55gzaec]
verify: contains Cargo.toml /^version = "0\.(28\.[2-9]|29\.)/
docs:
  - examples/lint-demo/protocol.toml
  - jbrubin-net#docs/superpowers/specs/2026-09-24-project-sites-design.md#12-evidence
seq: 40
created: 2026-09-30T15:21Z
---

18f26cc (lint-demo runs as shipped, sj-rgpc2qt) and 1ad66d1 (a refused init writes nothing,
sj-55gzaec) are on main after v0.28.1 and ride in the next release, v0.28.2 as the range stands.
Cutting it is the owner's.

jbrubin-net#jb-q2b1eaq's closing act pins its lint-demo tapes to "a release cut after the commit
the answer's close comment names", and sj-rgpc2qt's close comment promises a follow-up naming the
tag. So once the release is cut: `comment sj-rgpc2qt` with the tag and the two commits it carries
for the example, then close this.

The untracked `RELEASE-NOTES-v0.28.2.md` draft at the repo root predates both commits and does
not mention them. Adding them is an edit to a draft under the owner's review: propose the
sections, don't write them in. The verify checks only that the version was bumped past 0.28.1.
The comment is the act, and the store cannot check it.

## log
- 2026-09-30T15:21Z created
