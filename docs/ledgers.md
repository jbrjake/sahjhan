# ledgers and queries

The ledger is JSONL: one line of JSON per event, hash-chained to the line before
it. Every event is greppable, jq-able, and queryable in SQL. sahjhan embeds
Apache DataFusion, so the history is a table.

## querying

```bash
# how many findings, by severity?
sahjhan query "SELECT severity, count(*) FROM events WHERE type='finding' GROUP BY 1"

# what happened across every run?
sahjhan query --glob "docs/runs/*/ledger.jsonl" \
  "SELECT _source, type, count(*) FROM events GROUP BY 1, 2 ORDER BY 3 DESC"

# quick count
sahjhan query --type finding --count
```

Fields declared in `events.toml` become native Arrow columns, so SQL never digs
values out of JSON strings. Define `severity` in the event schema and it's a
real column you filter and group on. `--glob` adds a `_source` column naming the file
each row came from. `--format` takes `table`, `json`, `csv`, or `jsonl`.

The same SQL works as a gate condition, evaluated against the live ledger every
time the transition is attempted:

```toml
{ type = "query", sql = "SELECT count(*) < 15 as result FROM events WHERE type='finding'", expect = "true" }
```

The agent can't argue with a `COUNT(*)`. If the same predicate guards more than
one transition, declare it once under `[queries]` and reference it by name — see
[lint.md](lint.md#named-queries).

## multiple ledgers

Not every log needs a state machine. Sometimes you want an append-only
accumulator: a project-level event stream living alongside the per-run protocol.

```bash
# a project-wide, event-only ledger
sahjhan ledger create --name project --path project.jsonl --mode event-only

# record to it
sahjhan --ledger project event finding --field id=BH-042 --field severity=HIGH

# query across all of them
sahjhan query --glob "*.jsonl" "SELECT type, count(*) FROM events GROUP BY 1"
```

Stateful ledgers are bound to the state machine; event-only ledgers just
accumulate. Both are hash-chained. `--ledger` and `--ledger-path` steer every
command that reads or writes the ledger; the exception is `ledger checkpoint`,
which takes `--name` or falls back to the active marker and ignores them.

`sahjhan ledger list` shows what's registered, `ledger verify` re-checks a
chain, `ledger remove` unregisters without deleting the file, and `ledger import`
wraps bare JSONL from stdin in a hash-chained ledger.

## templates

If your protocol creates many ledgers with the same shape — runs, sprints,
iterations — declare a template in `protocol.toml` instead of hand-crafting each:

```toml
[ledgers.run]
description = "Per-run audit ledger"
path_template = "runs/{template.instance_id}/ledger.jsonl"
```

```bash
$ sahjhan ledger create --from run 25
created: run-25

$ sahjhan ledger create --from run 26
created: run-26
```

The name is derived (`run-25`), the path expands from the pattern
(`runs/25/ledger.jsonl`), and the registry records which template each ledger
came from, so renders can find them by template rather than by name
(`ledger_template` in `renders.toml`). Queries have no template selector —
reach for `--glob` there. `{template.name}` works in the pattern too.

A `[ledgers.X]` entry can carry a fixed `path` instead of a `path_template`,
but it's a declaration only — nothing creates or registers that ledger, and
`ledger create --from` refuses it. For a singleton, create it directly with
`ledger create --name project --path project.jsonl`.

`sahjhan validate` checks that each `[ledgers.X]` carries exactly one of `path`
or `path_template`, and that a `path_template` contains
`{template.instance_id}`. It doesn't reject unknown placeholders or check paths
for collisions.

## the active ledger

Typing `--ledger run-25` on every command gets old by the third command. The
active-ledger marker is a pointer file in the data directory saying "this one,
unless you say otherwise."

```bash
$ sahjhan ledger activate run-25
Activated ledger: run-25

$ sahjhan event finding --field id=BH-042 --field severity=HIGH
# recorded to run-25, no flag needed

$ sahjhan ledger create --from run 26 --activate
# creates run-26 and moves the marker in one step

$ sahjhan ledger deactivate
# clears the marker
```

Resolution order, highest priority first:

1. `--ledger-path <path>`
2. `--ledger <name>`
3. the active-ledger marker (a warning if it names something unregistered)
4. the first registry entry (`init` registers `default` first), else `data_dir/ledger.jsonl`

`sahjhan status` prints which ledger it read and why, so you don't spend twenty
minutes wondering where your events went:

```
Ledger: default (no active-ledger marker)
```

`sahjhan reset` clears the marker along with everything else it archives.

## checkpoints

`sahjhan ledger checkpoint` writes a checkpoint event into the chain, defaulting
to the active ledger. Checkpoints happen when you ask: `[checkpoints] interval`
parses in `protocol.toml` but nothing reads it yet.
