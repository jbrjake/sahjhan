---
id: sj-44ja0wa
title: "Post #33's resolution comment once the owner has reviewed it"
status: open
category: release
discovered-from: sj-dkvmtrp
relates: [sj-xarb4a4, sj-q6pq18t]
docs:
  - CLAUDE.md#issue-closure-rule
seq: 20
created: 2026-10-28T16:58Z
verify: contains docs/meshwork/sj-44ja0wa-post-33-s-resolution-comment-once-the-owner-has-r.md /^- 2026-\d\d-\d\d owner approved/
---

ISSUE CLOSURE RULE for #33, which v0.28.3 closed (46636de says `Closes #33`).
The draft is `ISSUE-33-COMMENT.md` at the repo root. The owner has not
reviewed it, so it is shown to the owner first and posted only on their word.
The only comment on #33 now is a v0.22.1 status note from before any fix,
which this one supersedes.

The table was re-run on 2026-10-28 against shipped 0.28.2 (holtz 0.170.16's
`bin/`) and the published v0.28.3 aarch64-darwin binary (SHA256SUMS and
attestation checked). Every row holds:
- `lint --only L5` with the issue's template plus a declared `_checkpoint`:
  3 warnings (`_checkpoint`, `prediction`, `prediction_outcome`) → `clean. 1
  check(s) run: L5`.
- `since = "last_event_of_type:checkpoint"`, with `check_done` at seq 2 and
  `_checkpoint` at seq 3: `valid.` and `transition complete` passes → refused
  at load, exit 3, with the hint naming `_checkpoint`.
- `since = "last_event_of_type:_checkpoint"`, undeclared: refused, exit 3 →
  `valid.`; `complete` blocks (exit 1) with `found 0 'check_done' event(s)
  after last '_checkpoint' event`, then passes after a `check_done`.

`ledger checkpoint` needs `--name default` on a fresh project (exit 4
otherwise). Without it the row-2 transcript writes no checkpoint and proves
nothing.

The verify is the owner's line in this file, `- <date> owner approved`, which
the owner writes and an agent never does. After it: `gh issue comment 33
--body-file ISSUE-33-COMMENT.md`, comment the returned URL on this task, then
close it. The verify cannot see the post, so the URL comment is the record
that it happened.

## log
- 2026-10-28T16:58Z created
