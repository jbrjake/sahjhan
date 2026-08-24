# static analysis: `sahjhan lint`

`sahjhan validate` answers "is this config well-formed?" Do the states exist, do the gates name real types, do the template files resolve? `lint` answers the next question: is this protocol *coherent*?

A gate can be perfectly well-formed and still be a dead-end. It can wait for an event nothing is able to record. It can wait for one whose only producer runs in a state the run has already left. Two gates that must agree about the same fact can drift apart until the blocking condition is strictly stronger than the escape hatch it prints, and the agent deadlocks while being told exactly which impossible thing to do. None of that is a parse error, and all of it is decidable at rest from config the engine already has.

Nothing here opens a ledger or runs a gate command. Config in, findings out, which makes it cheap enough for a pre-commit hook:

```bash
sahjhan --config-dir enforcement lint --strict || exit 1
```

```bash
$ sahjhan lint
L2 error: transitions.toml: transition 'finish' (middle → late)
    gate 'ledger_has_event' requires event 'signed_off', but every producer runs only
    in states that cannot precede 'middle' — hook:sign-off (in late)
    hint: widen the producer's available_in_states, or move the gate to a transition
          the producer can precede
L4 error: states.toml: state 'anomaly'
    non-terminal state 'anomaly' has no outgoing transition — a run that reaches it cannot continue
    hint: add a transition out of it, or mark the state terminal = true
2 error(s), 0 warning(s) from 7 check(s): L1, L2, L3, L4, L5, L6, L7
```

## the checks

| check | what it catches |
| --- | --- |
| `L1` | A gate requires an event nothing can produce. The agent reads the intent, does the work, stays blocked. |
| `L2` | A gate requires an event whose producers can only run in states that cannot precede it. Satisfiable on paper, unsatisfiable in every actual run. |
| `L3` | A path reaches a boundary's target without crossing the boundary. The route-around nobody noticed. |
| `L4` | A non-terminal state with no exit, or whose every exit is blocked by an unsatisfiable gate. |
| `L5` | A declared event nothing produces or consumes. Dead vocabulary that reads as load-bearing. |
| `L6` | Two copies of one predicate — inline SQL duplicating a named query, or each other. Drift waiting to happen. |
| `L7` | A gate demanding evidence stronger than the event supplying it. Reads as a strong check, enforces a weak one. |

Errors mean the protocol is provably broken given what the engine can see and sahjhan exits with code 3.
Warnings mean it's suspicious but a legitimate reading exists, but you can upgrade them to hard errors with `--strict`.

The ordering of the checks matters. L4 asks whether a state's exits are usable, which is only answerable after L1 and L2 have marked the transitions that can never fire. `--only` filters the findings emitted, never the linting process.

```
--only L1 --only L3     narrow the reported findings
--strict                warnings fail too
--json                  the usual envelope
```

Checks can also be switched off in config:

```toml
[lint]
disabled_checks = ["L6"]
```

## boundaries

Some edges exist to make something happen, and the protocol is only sound if every route crosses them. Declare the boundary, then tag the edges that satisfy it:

```toml
[[boundaries]]
name = "context-reset"
must_traverse = { from = "merge_done", to = "fix_loop" }
```

```toml
[[transitions]]
from = "awaiting_clear"
to   = "fix_loop"
command = "resume"
boundary = "context-reset"
```

L3 deletes every tagged edge and asks whether the target is still reachable. If it is, it prints the surviving path:

```bash
$ sahjhan --config-dir examples/lint-demo lint
clean. 7 check(s) run: L1, L2, L3, L4, L5, L6, L7

$ cp -r examples/lint-demo /tmp/broken     # reroute paused's `resume` to fix_loop
$ sahjhan --config-dir /tmp/broken lint
L3 error: protocol.toml: boundary 'context-reset'
    boundary 'context-reset' can be routed around: merge_done reaches fix_loop without crossing it — merge_done -(pause)-> paused -(resume)-> fix_loop
    hint: tag that path's edge with boundary = "context-reset", or remove the route (tagged today: resume)
1 error(s), 0 warning(s) from 7 check(s): L1, L2, L3, L4, L5, L6, L7
```

This is the check that most repays living in the engine. You can grep your own config for the tag. You can't see by grepping that a second `resume` added six months later, from an unrelated pause state, quietly became a way around.

## named queries

A predicate that decides a fact should exist once. Declare it in `protocol.toml`, then reference it by name:

```toml
[queries.fix_budget]
sql    = "SELECT count(*) < 3 as result FROM events WHERE type = 'fix_commit'"
intent = "three fixes is the budget"
```

```toml
{ type = "query", query = "fix_budget" }
```

Two gates that must agree are now the same object rather than two strings hoped to be equal, and `intent` travels with the predicate instead of being restated at every use. L6 flags inline copies you haven't converted yet, using a token-level normalized edit distance over case- and whitespace-insensitive SQL. The default similarity cutoff is 0.85:

```toml
[lint]
similarity_threshold = 0.9
```

## producers

L1 and L2 need to know who can record an event. The engine infers what it can, like that transitions `emit` and hooks `auto_record`, and you declare the rest:

```toml
[[events.context_reset.producers]]
id = "hook:session-start"
available_in_states = ["awaiting_clear"]
```

`id` is opaque; the engine only reports it back to you. `available_in_states` is the window L2 checks against the reachability relation.

By default L1 only errors on *restricted* events with no producer, because `sahjhan event` can record any declared non-restricted type and the engine would otherwise be claiming more than it knows. Once your producers are declared, opt into full closure:

```toml
[lint]
require_producers = true
```

## attestation levels

Evidence has strength, and the engine compares it without knowing what any of it means. Declare an ordering, weakest to strongest:

```toml
[attestation]
levels = ["agent", "tool", "ambient", "host"]
```

Then say how strong an event is, and how strong a transition demands:

```toml
[events.context_reset]
attestation = "host"
```

```toml
[[transitions]]
command = "resume"
  [transitions.integrity]
  requires_attestation = "host"
```

The levels are opaque strings whose only property is their position in your list. L7 compares them and reports issues like a gate that demands host-level proof while accepting something the agent can write itself. An individual gate may override the transition's requirement with its own `requires_attestation`.

## a worked example

[`examples/lint-demo`](../examples/lint-demo) is a small fix-loop protocol that exercises every one of the seven checks and passes all of them. Break any single declaration in it and lint tells you which one and why. Its config comments name the check each declaration is there to satisfy.
