# sahjhan

**real guardrails for agents**

sahjhan is a protocol enforcement engine. You describe your process in TOML as states, the transitions from one state to another, and the conditions that must be met to move between them. Then a compiled Rust binary tries to hold you to it.

Every step lands in a hash-chained append-only ledger. The conditions let sahjhan do real work to check the results. They can do things like run your test suite, look at the disk, count events, or run SQL over the ledger entries.

Agents talk to the CLI and get no other way in. You can even declare that specific ledger entries can't be added by an agent or any process they control, so you can make decisions on evidence they can't forge.

It exists because [agents can't be trusted](docs/why.md).

## quick-start

`examples/minimal` is three states, one gate, a couple checklist items, and some hooks. We'll go through it step by step below. But that's enough for a working protocol you can use to coerce an agent into testing and linting before it can complete a task:

```bash
$ cp -r examples/minimal enforcement # it looks for the declarative files in ./enforcement 

$ sahjhan init
initialized. good luck.

$ sahjhan status
Ledger: default (no active-ledger marker)
state: idle (1 events, chain valid)
sets:
  check: 0/2 [· tests, · lint]
next:
  begin: ready

$ sahjhan transition begin
idle → working (1 rendered)

$ sahjhan transition complete
✗ set_covered: set 'check' not fully covered; missing: tests, lint

$ sahjhan set complete check tests # below we'll cover how to enforce this without trusting the agent's word
set check: tests done (1/2, 1 rendered)
$ sahjhan set complete check lint
set check: lint done (2/2, 1 rendered)

$ sahjhan transition complete
working → done (1 rendered)

$ sahjhan log tail 3
[2026-08-19T20:04:11.948Z] seq=2 type=set_member_complete hash=a3a66ee20e22 {member=tests, set=check}
[2026-08-19T20:04:11.955Z] seq=3 type=set_member_complete hash=db5df835dd34 {member=lint, set=check}
[2026-08-19T20:04:11.961Z] seq=4 type=state_transition hash=c56535329cde {command=complete, from=working, to=done}
```

*Every terminal transcript in this file is pasted from a real run of the binary at v0.20.1.*

## install

Binaries for macOS and Linux in arm64 and x86_64 are on the [releases page](https://github.com/jbrjake/sahjhan/releases):

```bash
gh release download v0.20.1 -R jbrjake/sahjhan -p "sahjhan-aarch64-apple-darwin"
chmod +x sahjhan-aarch64-apple-darwin
mv sahjhan-aarch64-apple-darwin /usr/local/bin/sahjhan
```

`cargo build --release` works on source.

## writing a protocol

A protocol is a directory of TOML files, pointed at with `--config-dir` (default: `enforcement`). Three are required and the rest are optional. Here's the one the quick-start ran, in full:

### states

The steps, and which ones start and end a run.

```toml
# enforcement/states.toml
[states.idle]
label = "Idle"
initial = true # starting state

[states.working]
label = "Working"

[states.done]
label = "Done"
terminal = true # ending state
```

The agent can't skip ahead or double back, and it doesn't get to decide it's done without sahjhan agreeing.

### transitions and gates

Each transition names a command, a `from` state, a `to` state, and the gates that have to pass. This is where the enforcement lives.

```toml
# enforcement/transitions.toml
[[transitions]]
from = "idle"
to = "working"
command = "begin" # how you name it when you try to move to it in the cli
gates = []

[[transitions]]
from = "working"
to = "done"
command = "complete"
gates = [
    { type = "set_covered", set = "check", event = "set_member_complete", field = "member" },
]
```

A gate is something sahjhan checks itself, not something the agent reports. `file_exists` tests for presence on disk. `command_succeeds` runs a command, like your test suite. _sahjhan_ runs it, not the agent, and it captures the exit code and hashes the output to the ledger. `query` runs SQL against the live ledger, which lets you do some really sophisticated state-based conditions. There are thirteen types, plus four composites (`any_of`, `all_of`, `not`, `k_of_n`) for when a single condition won't say it. You can see them all in [docs/gates.md](docs/gates.md).

Every gate takes an optional `intent`, a sentence explaining why it's there. sahjhan prints it when the gate blocks, so the agent gets told what to fix and why it's important rather than that something failed.

### sets

Sometimes the agent has to do a thing for every item in a list, like review file A *and* B *and* C, not whichever one it looked at first. `set_covered` is the test that won't pass until every member has been checked off. Sets live in `protocol.toml` with the rest of the project config:

```toml
# enforcement/protocol.toml
[protocol]
name = "minimal"
version = "1.0.0"

[paths]
managed = ["output"] # hooks block agents from touching these paths
data_dir = "output/.sahjhan" # including the ledger and manifest for sahjhan
render_dir = "output" # as well as any artifacts sahjhan produces

[sets.check] # the set referenced above in the transition from working -> done
description = "Verification checks"
values = ["tests", "lint"] # working -> done after both testing and linting

[aliases]
"start" = "transition begin" # == 'sahjhan start'
"finish" = "transition complete" # == 'sahjhan finish'
```

That's the whole protocol the quick-start ran.

The optional files you'll reach for next are `events.toml` (a schema for what may go in the ledger), `renders.toml` (markdown views generated from the ledger, which the agent never writes), and `hooks.toml` (rules evaluated on every tool call rather than only at transitions). Two more, `vault.toml` and `trusted-callers.toml`, only matter once the daemon is involved.

### events

Without `events.toml` any event type is accepted with any fields. With it, fields are validated at recording time and become native SQL columns:

```toml
# enforcement/events.toml
[events.finding]
description = "Code quality issue found during review"
fields = [
    { name = "id", type = "string" },
    { name = "severity", type = "string", pattern = "^(LOW|MEDIUM|HIGH|CRITICAL)$" },
    { name = "file", type = "string" },
]
```

The `pattern` regex means the agent picks one of four severities or gets rejected. Fields are required unless marked `optional = true`. Then the ledger answers questions:

```bash
$ sahjhan query "SELECT type, count(*) AS n FROM events GROUP BY 1 ORDER BY 2 DESC"
n  type
-  -------------------
2  set_member_complete
2  state_transition
1  genesis
```

The same SQL works as a gate condition, which is how you write budgets:

```toml
{ type = "query", sql = "SELECT count(*) < 15 as result FROM events WHERE type='finding'", expect = "true" }
```

### branching

Two transitions can share a `from` state and a command. sahjhan evaluates the candidates in declaration order and takes the first whose gates pass, so the specific case goes first and the fallback goes last:

```toml
[[transitions]]
from = "implementing"
to = "verifying"
command = "submit"
gates = [ { type = "command_succeeds", cmd = "python -m pytest tests/", timeout = 120 } ]

[[transitions]]
from = "implementing"
to = "fix-and-retry"
command = "submit"
gates = []
```

If the tests pass the agent advances, and if they don't it lands in the fix loop instead. `submit` doesn't error, it routes. The ledger records which way it went, so afterward you can count how many times the agent looped.

A longer worked example (a TDD protocol using composites, template variables, renders and a recovery loop) is in [docs/protocols.md](docs/protocols.md).

## checking the protocol

`sahjhan validate` asks whether the config is well-formed with states that exist, real condition gates, and resolvable template paths.

`sahjhan lint` asks whether the protocol is *coherent*. A gate can be syntactically correct and broken, like states blocking on an event that literally can never occur, or states that can be unintentionally routed around through undeclared paths.

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

You can grep your own config for the tag. You can't see by grepping that a second `resume` added six months later, from an unrelated pause state, quietly became a way around. All seven checks can be found in [docs/lint.md](docs/lint.md).

`sahjhan gate check <command>` dry-runs the gates for a transition without taking it, so you can debug one that keeps blocking:

```bash
$ sahjhan gate check complete
gate-check: complete
  ✗ set_covered: set 'check' not fully covered; missing: tests, lint — all set members must be completed
result: blocked
```

And `sahjhan mermaid` draws the graph, either as a `stateDiagram-v2` or as ASCII for a terminal:

```
$ sahjhan --config-dir examples/lint-demo mermaid --rendered
[fix_loop] (initial)
 └─ merge ──▶ [merge_done]
    │ query
    ├─ clear ──▶ [awaiting_clear]
    │  └─ resume ──▶ [fix_loop] (↑ cycle)
    │     │ ledger_has_event
    ├─ pause ──▶ [paused]
    │  └─ resume ──▶ [awaiting_clear] (↑ cycle)
    └─ ship ──▶ [shipped] (terminal)
```

## enforcing between transitions

A transition only fires when the agent asks to move. Between transitions it can edit files, run commands, and write summaries claiming it's done. `hooks.toml` rules, in contrast, are evaluated on every tool call instead, narrowed by state, tool, and path glob:

```bash
$ sahjhan hook eval --event PreToolUse --tool Edit --file src/main.rs
{
  "decision": "block",
  "messages": [ { "source": "hook", "rule_index": 0, "action": "block",
    "message": "Cannot edit source files in working state without a check_done event. Record one first." } ],
  "auto_records": [], "monitor_warnings": []
}

$ sahjhan event check_done
recorded: check_done

$ sahjhan hook eval --event PreToolUse --tool Edit --file src/main.rs
{ "decision": "allow", "messages": [], "auto_records": [], "monitor_warnings": [] }
```

`sahjhan hook generate --harness cc` applies them to Claude Code with thin PreToolUse / PostToolUse / Stop scripts that call `hook eval` and forward the decision. Hooks, write-gated paths, and monitors are all in [docs/hooks.md](docs/hooks.md).

## trusted callers

The CLI is how we record evidence, and the agent can use the CLI. If the agent just fabricates evidence to move between states, none of this means anything. So some things need to be writable and readable by processes that aren't the agent, and by nothing else.

If you mark an event as `restricted` in `events.toml`, the plain CLI won't take it. It has to arrive as an HMAC proof over the event type and its sorted fields, computed by a daemon that holds a 32-byte session key in memory and never writes it anywhere. The daemon also has a vault where secrets can be recorded.

`trusted-callers.toml` lists script paths that can testify to restricted events and read and write vault secrets. The paths are relative to `--config-dir`, and the .toml also records their SHA-256 hashes so they can't be tampered with. When a trusted call is made, the daemon checks the caller's PID to make sure the agent didn't spawn the script and that it came from the right path.

So let's say you want to prove an agent actually read a file. You can build a quiz bank of questions about the file, and store it in the vault where the agent can't see the answers.

```bash
$ sahjhan event quiz_passed --field score=5/5 --field pass=true
error: event type 'quiz_passed' is restricted. Use 'sahjhan authed-event' with a valid proof.

$ sahjhan vault read --name quiz-bank
vault read failed: caller not authenticated

$ sahjhan sign --event-type quiz_passed --field score=5/5 --field pass=true
sign failed: caller not authenticated

$ python3 enforcement/hooks/quiz.py
questions: {"q1":"what does spill_batch_size default to?"} # the quiz can see the answers
recorded: quiz_passed # the agent has to answer them blind

$ sahjhan log tail 1
[2026-08-19T21:04:43.102Z] seq=1 type=quiz_passed hash=e579285ae4ca {pass=true, score=5/5}
```

The agent can read that entry, query it, and see the condition it satisfies. It can't produce the entry itself. It can't call `sign` (its parent is `bash`), can't modify `quiz.py` (the hash stops matching), can't drop a new script into the config dir (the bootstrap hook blocks writes there), and can't add itself to the manifest, because `trusted-callers.toml` is one of the sealed files and editing it takes a `reseal`...which takes the key it doesn't have.

Note: an absent or empty manifest means no restriction at all: the daemon serves every caller, so the file is opt-in, not a default. And a trusted script is trusted for everything, not per-operation. The granularity you get is which states a vault key is reachable in, via `vault.toml`. Both, plus `record_event` (which collapses sign-then-submit into one authenticated call), are in [docs/hardening.md](docs/hardening.md).

## what's actually protected

Each of these safeguards exists because an agent got past the one above it. The story is in [why.md](docs/why.md) and the mechanisms are in [hardening.md](docs/hardening.md).

| | |
| --- | --- |
| the ledger | hash-chained JSONL. Edit a line and the chain breaks on the next command |
| config | eight TOML files SHA-256 sealed into the genesis entry; changing one needs a `reseal` and an HMAC proof |
| restricted events | the agent can't record them at all — `authed-event` with a proof only the daemon can mint |
| the session key | 32 bytes in daemon memory, never on disk; `mlock`, `ptrace` denied, preload env refused |
| daemon callers | authenticated by kernel socket credentials against a hashed script manifest |
| gate results | command and snapshot gates record exit code, wall time, and a SHA-256 of stdout the agent never touches |
| managed files | tracked in a manifest whose own hash is in the ledger |

## when this makes sense

| reach for sahjhan when… | reach for something else when… |
| --- | --- |
| the process has steps an agent will skip whenever skipping is cheaper than doing | you trust the agent to keep a checklist, and it does |
| "done" has a **runnable** definition — a test exits 0, a file is on disk, a SQL count comes back under budget | done-ness is a judgment somebody makes by reading |
| the record afterward has to be evidence, not the agent's summary of itself | nobody's going to audit it |
| the same shaped process runs many times and you want the runs comparable | it's one-off work |
| you're willing to spend twenty minutes writing TOML per protocol | you need it today and a `TODO.md` will do |

Most agent work doesn't need any of this. A rule in `CLAUDE.md`, a checklist, a pre-commit hook, and reading the diff yourself covers an enormous amount of ground, and it costs nothing. If you haven't personally watched an agent `sleep 65` past your guard, you probably don't have that problem yet.

## limits

- **A gate is only as good as its command.** `{ type = "file_exists", path = "repro.log" }` is satisfied by `touch repro.log`, and an agent will absolutely reach for that. sahjhan guarantees the check ran and its result is recorded. It has no opinion about whether your check was any good.
- **This is not a sandbox.** The agent runs as you, with your filesystem and your network. The daemon raises the cost of non-compliance, but it does not make it impossible, and anything that can attach to your session can do what you can do. The threat model is an agent taking the cheap way out, not an attacker with a budget.
- **The hook layer depends on the harness calling the hooks.** If your harness skips a PreToolUse call, sahjhan never hears about the tool use.
- **By default the seal only covers the config files, not your gate scripts.** If a gate shells out to `scripts/check.py`, that script is outside the seal unless you put it under `paths.managed` and let the manifest track it.
- **macOS and Linux, one machine, one user.** The daemon uses `SO_PEERCRED` and `LOCAL_PEERPID`; there's no Windows build and no network protocol. Kill the daemon and the secrets vanish.

## docs

| | |
| --- | --- |
| [why.md](docs/why.md) | why this had to exist: the agent transcripts, and why a script wasn't enough |
| [protocols.md](docs/protocols.md) | writing a protocol: a worked TDD example, states, sets, events, renders, template variables |
| [gates.md](docs/gates.md) | every gate type and its parameters, composites, template resolution, `gate check` |
| [lint.md](docs/lint.md) | the seven static checks, named queries, boundaries, producers, attestation levels |
| [hardening.md](docs/hardening.md) | restricted events, HMAC, the daemon and vault, caller auth, config sealing, gate attestation |
| [hooks.md](docs/hooks.md) | Claude Code integration, runtime hooks, write-gated paths, monitors |
| [ledgers.md](docs/ledgers.md) | multiple ledgers, templates, the active-ledger marker, SQL over many runs |
| [cli.md](docs/cli.md) | full command reference, exit codes, the `--json` envelope |
| [internals.md](docs/internals.md) | ledger and manifest formats, path anchoring, locking, source layout |

`examples/minimal` is the protocol above. `examples/lint-demo` exercises every lint check and passes all seven.

Built on [DataFusion](https://datafusion.apache.org/) and [Tera](https://keats.github.io/tera/).

MIT — see [LICENSE](LICENSE).
