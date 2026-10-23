---
id: sj-xarb4a4
title: "Name the engine's _checkpoint in ENGINE_EVENTS, so a since anchor on it bounds its window"
category: config
seq: 5
labels: [gh-33]
docs:
  - CLAUDE.md#flow-config-loading
verify: "all(run cargo test target=config_tests since_anchor_on_the_engine_checkpoint, run cargo test target=gate_tests since_anchor_on_a_checkpoint_bounds_the_window)"
status: done
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
- 2026-10-23T21:53Z open→doing — claimed by claude (91a2d877-ce17-4ab5-9ccb-a31a561b6f73)
- 2026-10-23T21:59Z doing→done — verify exit 0 @ 65615d2+6

## comments
- 2026-10-23T21:46Z [claude (91a2d877-ce17-4ab5-9ccb-a31a561b6f73)] Name it once. The same pattern as provenance.rs's RECORDED_BY: a const for '_checkpoint' that write_checkpoint, find_latest_checkpoint and ENGINE_EVENTS all read, so the list and the writer cannot drift again. Worth checking the other five names the same way. Only 'genesis' appears as a literal at its write site (chain.rs:512).
- 2026-10-23T21:46Z [claude (91a2d877-ce17-4ab5-9ccb-a31a561b6f73)] Correction to the comment above: every engine event name is a bare literal at its write site, not only genesis. state_transition (machine.rs:417), gate_attestation (machine.rs:441), set_member_complete (status.rs:357), config_reseal (authed_event.rs:180), genesis (chain.rs:512), _checkpoint (chain.rs:351). The other five match ENGINE_EVENTS only because nobody misspelled them. One const per name, read by the writer, its readers and the list, closes the class.
- 2026-10-23T21:59Z [claude (91a2d877-ce17-4ab5-9ccb-a31a561b6f73)] Done: f9a017d (fix + tests), docs in the next two commits. All six engine event types are constants in config/events.rs, used by every writer and reader in src/. Two tests and the extended L5 exemption test were each watched failing with the old 'checkpoint' put back in the list. Suite 794 passed, 0 failed. #33 stays open until sj-q6pq18t lands.
- 2026-10-23T22:18Z [claude (91a2d877-ce17-4ab5-9ccb-a31a561b6f73)] Follow-up, 9d9df66: driving the release-note claims against a HEAD release build showed the refusal for last_event_of_type:checkpoint telling the user to declare [events.checkpoint]. Doing so loads the anchor again, and it still never matches. When the undeclared name is an engine event without its leading underscore, the refusal now names the engine's spelling ('the engine writes _checkpoint'). The assertion was added first and watched failing. Suite 799 passed.
