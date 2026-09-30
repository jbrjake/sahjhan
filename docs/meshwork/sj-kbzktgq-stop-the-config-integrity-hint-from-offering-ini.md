---
id: sj-kbzktgq
title: "Stop the config-integrity hint from offering init, which refuses whenever a ledger exists"
status: open
category: cli/commands
discovered-from: sj-55gzaec
relates: [sj-55gzaec]
verify: lacks src/cli/commands.rs /sahjhan init. to start a new ledger/
docs:
  - src/cli/commands.rs
seq: 30
created: 2026-09-30T15:15Z
handoff: |
  Up next, from the 2026-09-30 session that answered
  jbrubin-net#jb-q2b1eaq.
  
  This task: the hint text is in `src/cli/commands.rs` at two sites,
  `[open-ledger]` (~line 150) and
  `[open-targeted]` (~line 281), in the same format string. Replace the
  `or 'sahjhan init' to start
  a new ledger` half with what actually starts over from a sealed ledger
  (`reset`, which archives and
  needs a daemon-signed proof, see `cli/init.rs` `[cmd-reset]`). Pin it
  with a test that opens a
  ledger whose config changed after the seal.
  `tests/config_integrity_tests.rs` already builds that
  state. docs/hooks.md:51 and docs/hardening.md:353 quote the old text
  verbatim. Both are owner
  prose: list them, don't edit them.
---

The config-integrity-violation message in `src/cli/commands.rs` (two sites, lines ~150 and ~281:
`[open-ledger]` and `[open-targeted]`) ends with "Run 'sahjhan reseal' with a valid session key
to update the seal, or 'sahjhan init' to start a new ledger." `init` refuses whenever the ledger
file exists (`already initialized (...). run reset first.`, exit 4), and a sealed ledger always
exists when this message prints, so the second half sends the reader to a command that cannot do
what it says. Found reproducing sj-55gzaec.

The honest second option is `reset` (which archives the ledger and needs a daemon-signed proof),
not `init`. Both sites say the same thing, so fix them together, and pin the text with a test that
opens a ledger whose config changed after the seal.

## log
- 2026-09-30T15:15Z created
- 2026-09-30T15:21Z handoff by claude (e16f684c-ee66-4925-aac7-b58aedbf0707)
- 2026-09-30T15:21Z handoff by claude (e16f684c-ee66-4925-aac7-b58aedbf0707)
- 2026-09-30T15:22Z handoff by claude (e16f684c-ee66-4925-aac7-b58aedbf0707)

## comments
- 2026-09-30T15:20Z [claude (e16f684c-ee66-4925-aac7-b58aedbf0707)] Two docs quote the hint verbatim: docs/hooks.md:51 (in an example integrity_error reply) and docs/hardening.md:353. Changing the text makes both quotes stale, and both are owner prose, so hand the owner the list rather than editing them.
