---
id: sj-9a7ret0
title: Refuse to commit a transition whose state moved while its gates ran
status: open
category: state/transitions
discovered-from: sj-1zq2xr5
relates: [sj-1zq2xr5]
verify: run cargo test target=concurrent_append_tests test_transition_refuses_when_its_gate_moved_the_state
docs:
  - CLAUDE.md#flow-transition-lifecycle
seq: 10
created: 2026-10-23T20:23Z
---

A transition is prepared in one state and committed later: its gates and emit
commands run first, then `commit_transition` appends a `state_transition` whose
`from` is the state the gates saw. If another process moves the machine in
between — a gate command that itself runs `sahjhan transition`, or a second
actor — the commit still records `from = <stale state>` and moves the machine to
a target that was chosen, and gated, from a state it is no longer in.

Deterministic repro: a transition `race` (idle → done) whose gate runs
`sahjhan transition begin` (idle → working). After `transition race` the ledger
holds `idle → working`, then `idle → done`. The second record is false, and
`race`'s gates were evaluated in a state the machine had already left.

Fix: `commit_transition` re-reads the ledger and re-derives the state before its
first append, and refuses with `StateError::StateMoved` when the state is no
longer the one prepared from. Nothing is appended. Under the manifest lock,
which every CLI writer holds across commit (sj-1zq2xr5), nothing can move the
machine between that check and the append. The CLI reports it as a blocked
transition (exit 1): the command did not happen, and running it again
re-checks its gates in the state the machine is actually in.

Test: `tests/concurrent_append_tests.rs`, the race repro above against the built
binary; red first.

## log
- 2026-10-23T20:23Z created
