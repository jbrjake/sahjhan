---
id: sj-x8j01wr
title: Refuse a checkpoint on a config off its seal with the shared seal refusal
category: cli
seq: 15
docs:
  - CLAUDE.md#flow-config-integrity-verification
verify: "all(lacks src/cli/ledger.rs /valid session key to update the seal\\./, run cargo test target=config_integrity_tests a_checkpoint_off_its_seal_offers_both_ways_past)"
status: done
created: 2026-10-23T21:45Z
---
`cmd_ledger_checkpoint` (`src/cli/ledger.rs:404`) hand-rolls its own seal
refusal: "Run 'sahjhan reseal' with a valid session key to update the seal."
`[seal-refusal]` (`src/cli/commands.rs:156`) is the one refusal both ledger
openers return. It names reseal *or* restoring the sealed files, and
the checkpoint's copy leaves out the second way. Route the checkpoint through
`seal_refusal` (make it `pub(crate)`), so all three paths answer alike.

`cmd_ledger_verify`'s copy at `src/cli/ledger.rs:325` is a warning, not a
refusal, because the user asked to see the chain. Leave it as is.

## log
- 2026-10-23T21:45Z created
- 2026-10-23T22:04Z open→doing — claimed by claude (91a2d877-ce17-4ab5-9ccb-a31a561b6f73)
- 2026-10-23T22:05Z doing→done — verify exit 0 @ c6cabcb+5

## comments
- 2026-10-23T22:05Z [claude (91a2d877-ce17-4ab5-9ccb-a31a561b6f73)] Done: fix + test in the commit before the store close, docs after it. The test was written first and failed on the missing 'restore the files' line. It also pins that a refused checkpoint appends nothing, and that restoring the files is enough to proceed.
