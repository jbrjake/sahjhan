# gates

A gate is a condition sahjhan checks itself before allowing a transition. The
agent never reports a gate result; it asks to move and sahjhan decides.

Gates are declared inline on a transition, and the list is implicitly AND —
every gate must pass:

```toml
[[transitions]]
from = "implementing"
to = "verifying"
command = "submit"
gates = [
    { type = "command_succeeds", cmd = "python -m pytest tests/", timeout = 120,
      intent = "all tests must pass before verification" },
    { type = "no_violations" },
]
```

The same gate types work inside `hooks.toml` rules — see [hooks.md](hooks.md).

## gate types

| type | parameters | what it checks |
| --- | --- | --- |
| `file_exists` | `path` | The file is on disk. Not "I created it." On disk. |
| `files_exist` | `paths` | Every listed file is on disk. |
| `command_succeeds` | `cmd`, `timeout`, `attest` | sahjhan runs the command. Exit 0 or no deal. |
| `command_output` | `cmd`, `expect`, `timeout`, `attest` | sahjhan runs the command; stdout must match `expect`. |
| `ledger_has_event` | `event`, `min_count`, `max_count`, `filter` | At least `min_count` (and at most `max_count`) events of this type. |
| `ledger_has_event_since` | `event`, `since`, `min_count`, `filter` | The event was recorded since a reference point. |
| `ledger_lacks_event` | `event`, `filter` | Zero matching events. The inverse, for "must not have done X". |
| `set_covered` | `set`, `event`, `field` | Every member of the set has a matching event. |
| `min_elapsed` | `event`, `seconds` | N seconds since the last event of that type. |
| `no_violations` | (none) | No unresolved `protocol_violation` events. |
| `field_not_empty` | `field` | The named field of the event being recorded is present and non-blank. |
| `snapshot_compare` | `cmd`, `extract`, `compare`, `reference`, `timeout`, `attest` | Compare a live value against a recorded baseline. |
| `query` | `sql` *or* `query`, `expect` | SQL against the ledger, evaluated by DataFusion. |

Every type also accepts `intent`, a sentence explaining why the gate exists.
sahjhan prints it beside the failure when the gate blocks, so the agent is told
what to fix rather than that something failed. Omit it and sahjhan generates a
default from the gate type.

### notes on the ones with sharp edges

**`min_elapsed` proves the agent owns a clock.** By itself that's all it proves.
Ask me how I know. Pair it with a gate that requires evidence.

**`max_count` on `ledger_has_event`** turns the gate into a budget: at most three
`fix_commit` events before the run has to do something else.

**`since` on `ledger_has_event_since`** takes either `"last_transition"` or an
event type name. Given an event type, it measures from the last occurrence of
that type, falling back to the last transition if there isn't one.

**`filter`** is a key/value map matched against the event's fields, so
`ledger_has_event` can ask for a `finding` with `severity=CRITICAL` rather than
any finding at all.

**`attest`** defaults to true on the three gates that execute something. See
[gate attestation](hardening.md#gate-attestation) for what gets recorded and
why. Set `attest = false` to skip it on a warmup or an `echo`.

**`query`** takes inline `sql` or the name of a predicate declared once under
`[queries]`. Two gates that must agree about the same fact should be the same
named query rather than two strings hoped to be equal; lint L6 finds the copies
you haven't converted. See [lint.md](lint.md#named-queries).

## composites

Sometimes one condition won't say it. "Either the tests pass or someone signed
off." "At least 2 of 3 scanners." "No regressions recorded." Wrap gates:

| composite | parameters | passes when |
| --- | --- | --- |
| `any_of` | `gates` | any child passes (OR) |
| `all_of` | `gates` | every child passes (explicit AND) |
| `not` | `gate` | the single child fails (NOT) |
| `k_of_n` | `k`, `gates` | at least `k` children pass |

```toml
# either automated tests or a recorded manual override
{ type = "any_of", intent = "tests must run or be explicitly overridden", gates = [
    { type = "command_succeeds", cmd = "python -m pytest tests/", timeout = 60 },
    { type = "ledger_has_event", event = "manual_test_override" },
]}

# 2 of 3 quality checks must pass
{ type = "k_of_n", k = 2, intent = "at least 2 of 3 code quality checks must pass", gates = [
    { type = "command_succeeds", cmd = "python -m mypy src/" },
    { type = "command_succeeds", cmd = "python -m pylint src/" },
    { type = "command_succeeds", cmd = "python -m bandit -r src/" },
]}
```

`not` takes a single child under `gate`, not a list:

```toml
{ type = "not", intent = "no regressions before release", gate = {
    type = "ledger_has_event", event = "regression"
}}
```

Composites nest: an `any_of` can hold an `all_of` holding leaf gates. The depth
limit is whatever you can still read six months from now, which in practice is
about two levels. sahjhan won't stop you going deeper.

`sahjhan validate` checks that composites are well-formed — `any_of` and `all_of`
need a `gates` array, `not` needs a single `gate`, `k_of_n` needs a positive `k`
no larger than the number of children. It catches bad syntax, not bad logic.

## template variables

Gate `cmd` and `sql` strings may contain `{{var}}` placeholders. Values are
resolved from the current state's declared params, from command-line arguments,
and from config (`paths.*`, `sets.<name>`). Values are POSIX shell-escaped
before interpolation into a command, and passed raw into SQL. Where the values
come from is covered in [protocols.md](protocols.md#template-variables).

Two behaviors matter when debugging:

- A value that fails its event-field `pattern` is rejected before it reaches the
  shell.
- A placeholder that can't be resolved makes the gate **unevaluable** rather than
  failing it, so a gate is never run with a literal `{{var}}` in the string and
  never reports a misleading failure.

## dry-running a gate

`sahjhan gate check <command>` evaluates the gates for a transition without
taking it:

```bash
$ sahjhan gate check complete
gate-check: complete
  ✗ set_covered: set 'check' not fully covered; missing: tests, lint — all set members must be completed
result: blocked
```

Three statuses. `✓` passed, `✗` failed, `?` unevaluable — the third meaning the
gate needs a template variable you didn't supply:

```bash
$ sahjhan gate check "set complete perspective"
  ✓ SQL: SELECT count(*) >= 2 FROM events WHERE type='iteration_complete'
  ? query: unevaluable (requires arg: current_perspective)
  ? query: unevaluable (requires arg: current_perspective)
  ✗ ledger_has_event_since: no 'lens_sweep_started' event — sweep must begin
```

Gates with no template variables evaluate normally regardless.

For a branching command, `gate check` lists every candidate and names the one
that would be taken. Note that `command_succeeds` gates really do run their
commands here, so a dry run of a transition gated on your test suite runs your
test suite.
