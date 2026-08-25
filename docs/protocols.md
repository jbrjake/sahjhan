# writing a protocol

A protocol is a directory of TOML files, passed to the binary with `--config-dir` (default: `enforcement`). Three files are required, five are optional.

| file | required | what it declares |
| --- | --- | --- |
| `states.toml` | yes | the steps, and which are initial and terminal |
| `transitions.toml` | yes | the edges between states, and the gates on each |
| `protocol.toml` | yes | project config: paths, sets, aliases, queries, boundaries |
| `events.toml` | no | the schema for what may go in the ledger |
| `renders.toml` | no | markdown views generated from the ledger |
| `hooks.toml` | no | rules evaluated on every tool call — see [hooks.md](hooks.md) |
| `vault.toml` | no | state-gated access to daemon vault keys — see [hardening.md](hardening.md#state-gated-vault-keys) |
| `trusted-callers.toml` | no | which scripts may authenticate to the daemon — see [hardening.md](hardening.md#the-daemon) |

All eight are SHA-256 sealed into the genesis ledger entry at `init`, whether or not they exist.

[`examples/minimal`](../examples/minimal) is the smallest working protocol, and the README's quick-start runs it. 

This page builds a larger one.

## a worked example: enforcing TDD

Five files, wired together. Transitions sit in the middle: they reference state names for where the agent is and where it's going, sets from `protocol.toml` for tracking work, event types from `events.toml` for gate conditions, and `renders.toml` fires when transitions happen.

```
states.toml            transitions.toml              events.toml
┌──────────────┐       ┌────────────────────┐        ┌──────────────────┐
│ idle         │◀─from─┤ start              │        │ finding          │
│ writing-tests│◀─to───┤                    │        │   severity       │
│ implementing │       │ tests-done         │        │   file           │
│ fix-and-retry│       │   file_exists      │        │                  │
│ verifying    │       │   any_of           │        │ set_member_      │
└──────────────┘       │     ├ cmd_succeeds │        │   complete       │
                       │     └ ledger_has   │        │   set, member    │
protocol.toml          │   set_covered──┐   │        └──────────────────┘
┌──────────────────┐   │                │   │               ▲
│ sets:            │◀──┼────────────────┘   │               │
│  test-suites:    │   │ submit (2 routes)  │               │
│  - unit-tests    │   │   → verifying      │               │
│  - integ.-tests  │   │     cmd_succeeds   │               │
└──────────────────┘   │     k_of_n (2/3)   │               │
                       │     no_violations  │               │
                       │   → fix-and-retry  │               │
                       │     (fallback)     │               │
                       │                    │               │
                       │ retry              │               │
                       │   fix-and-retry    │               │
                       │     → implementing │               │
                       └─────────┬──────────┘               │
                                 │trigger                   │
                       ┌─────────┴──────────┐               │
                       │ STATUS.md          │               │
                       │   on_transition    │               │
                       │ FINDINGS.md        │               │
                       │   on_event [finding]┼──────────────┘
                       └────────────────────┘  query: WHERE type='finding'
                       renders.toml
```

You don't need to grasp it all up front. Let's go piece by piece.

### states: where the agent is

```toml
# tdd-protocol/states.toml
[states.idle]
label = "Idle"
initial = true

[states.writing-tests]
label = "Writing tests"

[states.implementing]
label = "Implementing"

[states.fix-and-retry]
label = "Fix and retry"

[states.verifying]
label = "Verifying"
terminal = true
```

Exactly one state is `initial`, and the agent moves between states only where a transition exists. Marking the end states `terminal` is what quiets the no-outgoing-transition warning. Nothing forces a protocol to have one. `fix-and-retry` is here because tests fail sometimes and the honest thing is to loop back (the branching section below wires it up).

### transitions: how the agent moves

Each transition names a `command`, a `from` state, a `to` state, and its gates.

```toml
# tdd-protocol/transitions.toml
[[transitions]]
from = "idle"
to = "writing-tests"
command = "start"
gates = []

[[transitions]]
from = "writing-tests"
to = "implementing"
command = "tests-done"
gates = [
    { type = "file_exists", path = "tests/test_feature.py", intent = "test file must exist on disk before implementation begins" },
    { type = "any_of", intent = "tests must run or be explicitly overridden", gates = [
        { type = "command_succeeds", cmd = "python -m pytest -q tests/", timeout = 60 },
        { type = "ledger_has_event", event = "manual_test_override" },
    ]},
    { type = "set_covered", set = "test-suites", event = "set_member_complete", field = "member", intent = "every test suite must be written before implementing" },
]

# Happy path: tests pass + quality checks → advance
[[transitions]]
from = "implementing"
to = "verifying"
command = "submit"
gates = [
    { type = "command_succeeds", cmd = "python -m pytest -q tests/", timeout = 120, intent = "all tests must pass before verification" },
    { type = "k_of_n", k = 2, intent = "at least 2 of 3 code quality checks must pass", gates = [
        { type = "command_succeeds", cmd = "python -m mypy src/" },
        { type = "command_succeeds", cmd = "python -m pylint src/" },
        { type = "command_succeeds", cmd = "python -m bandit -r src/" },
    ]},
    { type = "no_violations", intent = "clean record — no tampering" },
]

# Fallback: tests fail → go fix them
[[transitions]]
from = "implementing"
to = "fix-and-retry"
command = "submit"
gates = []

# Recovery loop
[[transitions]]
from = "fix-and-retry"
to = "implementing"
command = "retry"
gates = []
```

The `any_of` on `tests-done` passes if the suite runs *or* someone recorded a `manual_test_override` event, because CI goes down and the escape hatch should be auditable rather than absent. The `k_of_n` requires 2 of 3 quality tools, because demanding perfection is how nothing ever ships.

Every gate is something sahjhan checks itself. `file_exists` looks at the disk. `command_succeeds` runs the suite. The agent self-reports nothing. Full gate reference: [gates.md](gates.md).

`intent` is optional and worth writing. sahjhan prints it beside the failure so the agent is told what to fix and why. Omit it and sahjhan generates a default from the gate type (for `query` gates that reference a named query, the query's own `intent` fills in first).

### protocol.toml: paths, sets, aliases

```toml
# tdd-protocol/protocol.toml
[protocol]
name = "tdd"
version = "1.0.0"
description = "Test-driven development enforcement"

[paths]
managed = ["output", "src", "tests"]
data_dir = "output/.sahjhan"
render_dir = "output"

[sets.test-suites]
description = "Test suites that must be written"
values = ["unit-tests", "integration-tests"]

[aliases]
"start" = "transition start"
"done" = "transition submit"
```

`managed` lists directories the agent may not write to directly. The hooks block edits there, and it bounds what the manifest may track. The manifest tracks the files sahjhan itself writes (the ledger and rendered views), not the contents of `src/` or `tests/`. sahjhan's artifacts must themselves live under a managed path, which is why `data_dir` and `render_dir` sit inside `output/` rather than at the project root. `data_dir` holds the ledger, manifest, and ledger registry. `render_dir` is where rendered markdown lands.

A **set** is a checklist the agent can't skip items on. Declare the members, and the agent checks them off one at a time:

```bash
$ sahjhan set complete test-suites unit-tests
```

That records a `set_member_complete` event with `set=test-suites` and `member=unit-tests`. The `set_covered` gate asks whether every member has one. Its `event` and `field` parameters name which events count as check-offs; you'll almost always use the values shown above, since that's what `set complete` writes.

Aliases are shorthand. `sahjhan start` expands to `sahjhan transition start`.

`protocol.toml` also carries `[queries]`, `[[boundaries]]`, `[attestation]`, and `[lint]`. Those are covered in [lint.md](lint.md). `[guards.write_gated]` is covered in [hooks.md](hooks.md), and `[ledgers]` is covered in
[ledgers.md](ledgers.md).

### events: what may go in the ledger

Without `events.toml`, any event type is accepted with any fields. Declaring a type gets its fields validated at recording time and turned into native Arrow columns for SQL. Undeclared types still pass through unvalidated.

```toml
# tdd-protocol/events.toml
[events.finding]
description = "Code quality issue found during review"
fields = [
    { name = "id", type = "string" },
    { name = "severity", type = "string", pattern = "^(LOW|MEDIUM|HIGH|CRITICAL)$" },
    { name = "file", type = "string" },
]

[events.set_member_complete]
description = "A set member was checked off"
fields = [
    { name = "set", type = "string" },
    { name = "member", type = "string" },
]
```

The `pattern` regex on `severity` means the agent picks one of four values or gets rejected. Declaring `set_member_complete` doesn't change what `sahjhan set complete` records because that command writes its `set` and `member` fields directly, skipping validation. But the declaration is what turns those fields into SQL columns and gives lint its vocabulary entry.

Fields are required by default. Mark the ones that only matter sometimes as `optional`:

```toml
[events.finding_resolved]
description = "A finding was resolved"
fields = [
    { name = "id", type = "string", pattern = "^B[HJ]-\\d{3}$" },
    { name = "commit_hash", type = "string" },
    { name = "evidence_path", type = "string", optional = true },
]
```

Omit `evidence_path` and sahjhan accepts the event. Provide it and it's still checked against `pattern`. Deciding *when* the field matters is a job for your gates. The schema only decides whether to reject the event for leaving it out.

Two more keys live here. `restricted = true` means the event needs an HMAC proof and `sahjhan event` will refuse it. `attestation = "<level>"` names how strong the evidence is. Those are covered in [hardening.md](hardening.md) and [lint.md](lint.md).

### renders: status files the agent can't write

Say you want a `STATUS.md`, but you don't want the agent to gloss anything by narrating the file. You can have sahjhan materialize it instead.

```toml
# tdd-protocol/renders.toml
[[renders]]
target = "STATUS.md"
template = "templates/status.md.tera"
trigger = "on_transition"

[[renders]]
target = "FINDINGS.md"
template = "templates/findings.md.tera"
trigger = "on_event"
event_types = ["finding"]
```

`on_transition` renders fire on every state change. `on_event` renders fire when the named event types are recorded. Templates are [Tera](https://keats.github.io/tera/) (Jinja2 syntax).

Templates receive the full event history as `events`, an array of objects with `seq`, `event_type`, `timestamp`, and `fields`, plus `state`, `protocol`, `sets`, `ledger_len`, and `violations`.

You can inspect what they see with:

```bash
$ sahjhan render --dump-context
```

Two custom Tera filters ship with the engine. `where_eq` keeps array items whose attribute equals a value; `unique_by` deduplicates by a field, keeping the last occurrence. Both take dot-notation for nested fields:

```tera
{% set resolved = events | where_eq(attribute="event_type", value="finding_resolved")
                        | unique_by(attribute="fields.id") %}
Resolved: {{ resolved | length }}
```

Renders can target a specific ledger by name or by template, see [ledgers.md](ledgers.md).

## branching: two routes, one command

Several transitions may share a `from` state and a `command`. sahjhan evaluates the candidates in declaration order and takes the first whose gates all pass, so the specific case goes first and the fallback goes last.

```toml
# First candidate: strict gates
[[transitions]]
from = "implementing"
to = "verifying"
command = "submit"
gates = [
    { type = "command_succeeds", cmd = "python -m pytest -q tests/", timeout = 120 },
    { type = "k_of_n", k = 2, gates = [ ... ] },
    { type = "no_violations" },
]

# Second candidate: no gates, always matches
[[transitions]]
from = "implementing"
to = "fix-and-retry"
command = "submit"
gates = []
```

`submit` doesn't fail, it routes. The ledger records which candidate was taken, so afterward you can count the loops.

`sahjhan validate` warns when a branching command has no fallback, as in every candidate carries gates, so all of them can fail and the command becomes a dead end. Sometimes that's what you meant.

`sahjhan gate check submit` shows the candidates and which would match:

```bash
$ sahjhan gate check submit
gate-check: submit
candidate 1: implementing → verifying
  ✗ command_succeeds: command 'python -m pytest -q tests/' exited with status 1 — all tests must pass before verification
── stdout (tail) ──
F                                                                        [100%]
=================================== FAILURES ===================================
_________________________________ test_feature _________________________________

    def test_feature():
>       assert 2 + 2 == 5
E       assert (2 + 2) == 5

tests/test_feature.py:2: AssertionError
=========================== short test summary info ============================
FAILED tests/test_feature.py::test_feature - assert (2 + 2) == 5
1 failed in 0.01s
  ✗ k_of_n: only 0 of 2 required passed; failed: [command_succeeds: command succeeds: python -m mypy src/; command_succeeds: command succeeds: python -m pylint src/; command_succeeds: command succeeds: python -m bandit -r src/] — at least 2 of 3 code quality checks must pass
  ✓ no unresolved protocol_violation events
candidate 2: implementing → fix-and-retry
  (no gates — always passes)
result: would take → fix-and-retry
```

## template variables

Gate commands and SQL can carry `{{var}}` placeholders, which gives them a lot of flexibility. Values come from the declared params of the state a candidate transition points *to* — the destination, not where the agent currently stands — and from config. In command-running gates the values are POSIX shell-escaped before interpolation. In `sql` and file-gate paths they interpolate plain.

Declare a param on a state and say where it gets its value (it resolves when a transition targets that state):

```toml
[states.reviewing]
label = "Reviewing"
params = [{ name = "current_perspective", set = "perspectives", source = "current" }]
```

| `source` | value |
| --- | --- |
| `current` | the first set member with no `set_member_complete` event yet |
| `last_completed` | the most recently completed member |
| `values` (default) | every member, comma-joined |

The map handed to a gate also contains `paths.data_dir`, `paths.render_dir`, `paths.managed`, and each `sets.<name>` as a comma-joined string. Arguments on the command line override anything of the same name:

```bash
$ sahjhan transition review current_perspective=security
```

Positional arguments map onto the names a transition declares in `args`. A gate whose variables can't all be resolved is reported as *unevaluable* (`?`) instead of being run with a literal `{{var}}` in the string. See [gates.md](gates.md).

## violations

Nothing records a `protocol_violation` automatically. It's ledger vocabulary your protocol writes, typically via a `hooks.toml` rule with `auto_record` when a guard fires, or by hand when you catch something. The `no_violations` gate blocks while any are unresolved. Resolving one means recording the counterpart:

```bash
$ sahjhan event violation_resolved --field "detail=reverted unauthorized edit to src/main.rs"
```

Resolution is counter-based, not paired. Each `violation_resolved` cancels one `protocol_violation`, and the gate passes when the resolved count reaches the violation count. Both event types stay in the ledger permanently. The violations don't disappear, they get addressed.
