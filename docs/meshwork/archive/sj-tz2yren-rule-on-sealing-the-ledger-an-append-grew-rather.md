---
id: sj-tz2yren
title: Rule on sealing the ledger an append grew rather than data_dir/ledger.jsonl
category: manifest
seq: 30
discovered-from: sj-1zq2xr5
relates: [sj-1zq2xr5]
docs:
  - CLAUDE.md#flow-manifest-seal-one-lock-from-load-to-last-save
verify: run cargo test target=concurrent_append_tests test_an_append_to_a_named_ledger_seals_that_ledger
status: done
created: 2026-10-23T20:24Z
---
`track_ledger_in_manifest` (`src/cli/commands.rs`, `[track-ledger]`) hashes
`ledger_path(data_dir)`, the default ledger, after every append, whichever
ledger was appended to. It has done so since the first CLI commit, before named
ledgers existed. So an append to a named or active ledger (holtz's
`docs/holtz/runs/<n>/ledger.jsonl`) re-hashes a file that did not change, and
the one that did is never in the manifest. The hash chain catches a rewritten
entry. It does not catch a run ledger truncated by its last N entries, which
still chains. The manifest is what would catch that.

The change is small: track `ledger.path()` under its project-root key when it
is under a managed path, and leave an unmanaged one untracked. Tracking it as
E13 would fail every event on a ledger outside `managed`.

Why this is a ruling and not a fix: it puts every run ledger under `manifest
verify`. Every append path seals now (sj-1zq2xr5), but a writer *killed*
between its append and its seal still leaves a mismatch. Claude Code kills a
hook that overruns its timeout, and holtz's couriers are hooks. Today only the
default ledger carries that exposure, and holtz never writes it. After this
change holtz's run ledger does, and holtz records every mismatch as a permanent
violation. Truncation detection against that exposure is the owner's call, and
probably holtz's too.

## log
- 2026-10-23T20:24Z created
- 2026-10-23T22:05Z open→doing — claimed by claude (91a2d877-ce17-4ab5-9ccb-a31a561b6f73)
- 2026-10-23T22:13Z doing→done — verify exit 0 @ 09497f9+5

## comments
- 2026-10-23T21:53Z [claude (91a2d877-ce17-4ab5-9ccb-a31a561b6f73)] Owner ruled yes, in session 91a2d877 (2026-10-23): seal the ledger an append grew, under its project-root key when it is under a managed path. Run ledgers come under manifest verify, killed-writer exposure accepted. Lands in v0.28.3.
- 2026-10-23T22:13Z [claude (91a2d877-ce17-4ab5-9ccb-a31a561b6f73)] Done: 798971c (fix + tests), 4d52ecf and 09497f9 (docs). Keyed through paths::project_relative so a cwd-relative --ledger-path from a subdir, with '..', gets the same key as --ledger. Correction to this task's body: a registered ledger outside data_dir is stored absolute in the registry, not as <data_dir>/../…, so the '..' case is --ledger-path or a hand-edited registry. Watched failing three ways: sealing the default ledger always, a key not spelled against the root (the --ledger-path append left the seal stale), and no skip for an unmanaged ledger (E13 on every append). Suite 799 passed, 0 failed; the 28 ignored daemon tests in daemon_record_event / config_integrity / daemon_signing pass. Not sealed: ledger create and ledger import, which make a ledger without appending to one. A genesis-only ledger cannot be truncated short of emptying it, which Ledger::open refuses, so it is sealed by its first append.
