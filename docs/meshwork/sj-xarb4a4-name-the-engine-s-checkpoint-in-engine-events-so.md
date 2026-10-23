---
id: sj-xarb4a4
title: "Name the engine's _checkpoint in ENGINE_EVENTS, so a since anchor on it bounds its window"
category: config
seq: 5
labels: [gh-33]
docs:
  - CLAUDE.md#flow-config-loading
verify: "all(run cargo test target=config_tests since_anchor_on_the_engine_checkpoint, run cargo test target=gate_tests since_anchor_on_a_checkpoint_bounds_the_window)"
status: open
created: 2026-10-23T21:45Z
---
GitHub #33, second class. `ENGINE_EVENTS` (`src/config/events.rs:25`) lists
`"checkpoint"`. The engine writes `"_checkpoint"` (`Ledger::write_checkpoint`,
`src/ledger/chain.rs:351`) and reads the same name back
(`find_latest_checkpoint`). #33 reports this as an L5 false positive. It is
also a validator hole of #34's class, because `[resolve-since-anchor]` admits
any name in that list:

- `since = "last_event_of_type:checkpoint"` validates clean and never matches.
  No baseline is ever found, so the window is the whole ledger.
- `since = "last_event_of_type:_checkpoint"`, the engine's real name, is refused
  with "anchors on undeclared event type '_checkpoint'" unless the consumer
  declares `[events._checkpoint]`.

Driven 2026-10-23 against a HEAD release build (0.28.2 + the unpushed range),
minimal protocol, gate `ledger_has_event_since event=check_done
since="last_event_of_type:checkpoint"`: `validate` says `valid.`; ledger seq 2
`check_done`, seq 3 `_checkpoint`; `transition complete` passes, though no
`check_done` follows the checkpoint.

The fix: rename the entry and test both directions. A config naming
`last_event_of_type:checkpoint` is refused at load after this. That is the
right outcome because the anchor never matched, and the release notes carry it
under Breaking changes with the migration (`_checkpoint`). Check every other
reader of `is_engine_event` (`[check-since-filter]`, lint `[build-producers]`,
L5) for a behavior change.

Also: CLAUDE.md's Provenance flow (line 739) says `sahjhan init / a checkpoint`
records `engine:init`. A checkpoint records `agent:cli` (driven, `sahjhan query`
on the same ledger). Correct the map in the same commit.

Closing #33 needs this and the L5 template task. The resolution comment
follows the ISSUE CLOSURE RULE.

## log
- 2026-10-23T21:45Z created
