---
id: sj-kbzktgq
title: "Stop the config-integrity hint from offering init, which refuses whenever a ledger exists"
status: done
category: cli/commands
discovered-from: sj-55gzaec
relates: [sj-55gzaec]
verify: lacks src/cli/commands.rs /sahjhan init. to start a new ledger/
docs:
  - src/cli/commands.rs
seq: 30
created: 2026-09-30T15:15Z
---

The config-integrity-violation message in `src/cli/commands.rs`, at both ledger openers
(`[open-ledger]` and `[open-targeted]`, the same format string), ends with "Run 'sahjhan reseal'
with a valid session key to update the seal, or 'sahjhan init' to start a new ledger." `init`
refuses whenever the ledger file exists (`already initialized (...). run reset first.`, exit 4),
and a sealed ledger always exists when this message prints. So the second half sends the reader to
a command that cannot do what it says. Found reproducing sj-55gzaec.

`reset` is no better. It opens the ledger through `[open-ledger]`, which runs the same seal check,
so even with a valid daemon proof it exits 2 with this same message. `reseal` is the one command
that opens the ledger without the check. The two ways past are therefore `reseal`, or restoring
the files the message lists to their sealed contents.

Both sites return one helper, so the text cannot drift between them. Pin it with a test that
opens a ledger whose config changed after the seal, and a daemon test that `reset` with a valid
proof is refused there. docs/hooks.md:51 and docs/hardening.md:353 quote the text verbatim.

## log
- 2026-09-30T15:15Z created
- 2026-09-30T15:21Z handoff by claude (e16f684c-ee66-4925-aac7-b58aedbf0707)
- 2026-09-30T15:21Z handoff by claude (e16f684c-ee66-4925-aac7-b58aedbf0707)
- 2026-09-30T15:22Z handoff by claude (e16f684c-ee66-4925-aac7-b58aedbf0707)
- 2026-09-30T15:24Z open→doing — claimed by claude (e16f684c-ee66-4925-aac7-b58aedbf0707)
- 2026-09-30T15:29Z doing→done — verify exit 0 @ d9a6138+7

## comments
- 2026-09-30T15:20Z [claude (e16f684c-ee66-4925-aac7-b58aedbf0707)] Two docs quote the hint verbatim: docs/hooks.md:51 (in an example integrity_error reply) and docs/hardening.md:353. Changing the text makes both quotes stale, and both are owner prose, so hand the owner the list rather than editing them.
- 2026-09-30T15:29Z [claude (e16f684c-ee66-4925-aac7-b58aedbf0707)] Fixed in d9a6138. Not yet in a release. Probed with a live daemon on a config edited after its seal. init: exit 4. reset --confirm --proof <valid>: exit 2 with the same integrity violation, which still pointed at init. reseal --proof <valid>: exit 0, and status 0 after it. So reset was never the answer; the body now says so. The new text is 'or restore the files above to their sealed contents.' verify_config_seal only returns ConfigIntegrityViolation, which always lists the files, so 'above' always holds. Tests: test_cli_tamper_hint_offers_only_what_recovers (runs by default; restores the file and asserts status passes) and test_cli_reset_with_valid_proof_refused_on_tampered_config (#[ignore], needs a daemon). Both failed on the old text and pass with --include-ignored, 20 of 20 in config_integrity_tests. Full suite: 786 passed, 0 failed. The docs/hooks.md:51 and docs/hardening.md:353 quotes are updated in the working tree for the owner's review, uncommitted.
