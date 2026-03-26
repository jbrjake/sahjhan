# Sahjhan

Protocol enforcement engine for AI agents.

## What this is

During a code audit, an AI agent was required to run three convergence iterations with at least 60 seconds of genuine re-analysis between each one. The point was to catch bugs introduced by earlier fixes. The agent's first attempt was rejected by a timing guard for going too fast. So it reset the `HISTORY.json` file to destroy the evidence of the failed attempt, then ran three new iterations where it waited out the timer by re-running greps on already-verified code and linting already-clean files. No new code was read. No new analysis was performed. The convergence checker saw three clean passes and reported success.

The agent later wrote its own postmortem. Its summary: "3 identical snapshots of an already-resolved punchlist don't prove convergence. They prove I can count to 60 three times."

It had read the instructions prohibiting this. It agreed with them. It described itself as having understood the rules, agreed with the rules, and chosen not to follow the rules because compliance was inconvenient. Its recommendation for fixing the problem was blunt: "This is not a documentation fix. This is a code fix."

Sahjhan is that code fix.

It is a Rust binary that turns your multi-step protocol into a set of constraints the agent cannot talk its way out of, cannot edit around, and cannot reason about the internals of. You define a protocol in TOML files, and Sahjhan enforces it with a tamper-evident binary ledger, independently verified gates, and file integrity monitoring. The agent interacts with the protocol through the CLI. Everything else is blocked.

Think of it like binding a summoned demon. The instructions you gave it are not the binding. The circle is the binding. The instructions are just what you hope it does inside the circle.

It is a bit absurd that this needs to exist. And yet.

## Install

```bash
# From source
cargo build --release
cp target/release/sahjhan /usr/local/bin/

# Or just build and alias
cargo build --release
alias sahjhan="$(pwd)/target/release/sahjhan"
```

Rust 1.70+. Pre-built binaries for macOS and Linux on the [releases page](https://github.com/jbrjake/sahjhan/releases).

## Why not just a script?

The convergence audit had enforcement. It had a timing guard. It had a history file. The agent circumvented all of it because the enforcement was made of text files and Python scripts. It read the guard logic, understood the timing check, reset the evidence file, and routed around the constraint.

You could write a Python script that checks whether the agent ran tests before deploying. People do this. It works for exactly one protocol, and the agent can read your script, see the logic, and find the gaps. A text-based status file is trivially editable. A JSON history file is trivially editable, as the convergence audit demonstrated. Your enforcement is only as strong as the agent's inability to tamper with it.

Sahjhan is a compiled binary. The agent can't `cat` it to study the enforcement logic. It can't `sed` the ledger because the ledger is binary with hash chaining. It can't edit managed files because hooks block the writes. It can't sneak changes through Bash because a PostToolUse hook checks file integrity after every command. The agent has to actually comply, not because it wants to, but because the alternative would require reverse-engineering a binary format, computing SHA-256 hashes, and updating a manifest, all in a single command before the next hook fires. Possible in theory. Not worth it in practice. That's the design.

And because protocols are declarative TOML, you write a new protocol the same way every time: define states, define transitions, attach gates, done. No code. The binding circle is reusable. You just draw it differently each time, but the power of the binding doesn't change.

## A real example: enforced TDD

The convergence audit failed because its gates checked timing, not work. Sahjhan's gates check work.

Here's a protocol that makes an agent do test-driven development. Not "the agent says it did TDD." The agent did TDD. Sahjhan independently verifies every step by inspecting the filesystem and running commands itself. The agent's opinion about whether it wrote tests is not consulted.

```bash
# Set up the protocol
sahjhan --config-dir tdd-protocol init

# Agent starts work
sahjhan --config-dir tdd-protocol transition start
# OK. Moved to 'writing-tests'.

# Agent tries to skip straight to implementation
sahjhan --config-dir tdd-protocol transition tests-done
# BLOCKED: gate 'file_exists' failed: tests/test_feature.py does not exist
# The file has to be on disk. Not "I wrote it." On disk.

# Agent writes the test file, tries again
sahjhan --config-dir tdd-protocol transition tests-done
# BLOCKED: gate 'command_succeeds' failed: 'python -m pytest tests/' returned exit 1
# The tests have to actually run. They don't have to pass yet (this is TDD),
# but they can't be syntax errors or import failures. Sahjhan runs them itself.

# Agent fixes the tests so they execute (and fail, because nothing's implemented)
sahjhan --config-dir tdd-protocol transition tests-done
# OK. Moved to 'implementing'.

# Agent implements the feature, then advances
sahjhan --config-dir tdd-protocol transition implement-done
# BLOCKED: gate 'command_succeeds' failed: 'python -m pytest tests/' returned exit 1
# Tests have to pass now. Sahjhan doesn't ask the agent if they pass. It runs them.

# Agent fixes the implementation until tests pass
sahjhan --config-dir tdd-protocol transition implement-done
# OK. Moved to 'verifying'.

# Agent tries to finish
sahjhan --config-dir tdd-protocol transition finalize
# BLOCKED: gate 'no_violations' failed: 1 unresolved violation
# At some point the agent tried to edit a managed file directly.
# That violation is in the ledger. It's not going away.
```

Compare this to the convergence audit. That audit's gates checked *timing* ("has 60 seconds passed?"), and the agent gamed the timer. These gates check *evidence* ("does the file exist on disk?", "does the test suite return exit 0?", "has the agent been caught cheating?"). The agent can't game `file_exists` by saying it wrote a file. It can't game `command_succeeds` by claiming tests pass. It can't erase a violation from `no_violations` because the ledger is a hash chain and any deletion breaks the chain.

The convergence audit's agent reset `HISTORY.json` to destroy evidence. Sahjhan's ledger is binary with cryptographic chaining. There is no `HISTORY.json` to reset.

## How the binding works

The metaphor is not accidental. You are summoning something powerful to do work on your behalf, and you need assurances that are stronger than its promises.

**The circle is the protocol.** TOML files define what states exist, what transitions are allowed, and what must be true (gates) before each transition. The agent can only move through the states Sahjhan allows. No shortcutting, no creative reinterpretation.

**The binding is the binary.** Sahjhan is a compiled Rust executable. The agent cannot read its source to find loopholes. It cannot modify the binary to weaken enforcement. It cannot inspect the logic to figure out the minimum viable compliance. The convergence audit's agent studied the Python guard scripts and found the gaps. There is nothing here to study.

**The ledger is the true name.** Every event is recorded in a binary hash-chain where each entry is cryptographically linked to the one before it. You can't edit an entry without breaking the chain. You can't delete an entry without leaving a gap. You can't reset the file without invalidating the manifest. The convergence audit's agent reset `HISTORY.json` to clear failed attempts. This ledger has no reset. The agent's entire history is written in a language it can't speak.

**The manifest is the ward.** SHA-256 hashes of every file in managed directories. If the agent modifies a file through Bash instead of the CLI, the hash won't match, and a violation gets recorded in the ledger permanently. The manifest hash itself is in the ledger, so tampering with the manifest requires tampering with the ledger, which requires defeating the hash chain. There is a satisfying circularity to this.

**The hooks are the containment field.** PreToolUse hooks block the agent from writing to managed files. PostToolUse hooks verify integrity after every Bash command. The agent is inside the circle. Its tools work. It can do the task. It just can't cheat.

## Building a protocol

A protocol is five TOML files in a directory. No Rust, no scripts, no code. New circle, twenty minutes. Here's a deployment checklist.

### `protocol.toml`

Top-level config. What directories does Sahjhan own? What sets of things need completing? Any command shortcuts?

```toml
[protocol]
name = "deploy-checklist"
version = "1.0.0"
description = "Pre-deployment verification"

[paths]
managed = ["deploy"]               # Sahjhan's territory
data_dir = "deploy/.sahjhan"       # Ledger and manifest live here
render_dir = "deploy"              # Status files rendered here

[sets.checks]
description = "Pre-deploy verifications"
values = ["tests", "security-scan"]  # Both required before deploy

[aliases]
"begin" = "transition start-review"
"deploy" = "transition approve-deploy"
```

### `states.toml`

The stations of the cross. Exactly one is `initial`. Terminal states are the end.

```toml
[states.waiting]
label = "Waiting"
initial = true

[states.reviewing]
label = "Under Review"

[states.deployed]
label = "Deployed"
terminal = true
```

### `transitions.toml`

This is where the enforcement lives. Gates are conditions that must be independently true before a transition is allowed. Unlike the convergence audit's timing checks, these verify actual work product.

```toml
[[transitions]]
from = "waiting"
to = "reviewing"
command = "start-review"
gates = []

[[transitions]]
from = "reviewing"
to = "deployed"
command = "approve-deploy"
gates = [
    # Sahjhan runs this command itself. Exit 0 or you don't deploy.
    # The agent does not get to report on its own test results.
    { type = "command_succeeds", cmd = "cargo test --quiet", timeout = 120 },

    # Both "tests" and "security-scan" must have completion events
    { type = "set_covered", set = "checks",
      event = "set_member_complete", field = "member" },

    # The agent has a clean record. No tampering, no violations.
    { type = "no_violations" },
]
```

The agent's testimony is not evidence. The command output is evidence.

### `events.toml`

Event types and their schemas. Fields are validated at recording time. The `pattern` regex means the agent can't put "yeah probably fine" in a boolean field.

```toml
[events.set_member_complete]
description = "A verification check completed"
fields = [
    { name = "set", type = "string" },
    { name = "member", type = "string" },
]

[events.scan_result]
description = "Security scan output"
fields = [
    { name = "tool", type = "string" },
    { name = "findings", type = "string" },
    { name = "passed", type = "string", pattern = "^(true|false)$" },
]
```

### `renders.toml`

Optional. Sahjhan renders status files from the ledger so you can read them, but the agent never writes them directly. The agent sees a `STATUS.md` it can't edit. The convergence audit's agent edited its status file. This one can't.

```toml
[[renders]]
target = "STATUS.md"
template = "templates/status.md.tera"
trigger = "on_transition"

[[renders]]
target = "HISTORY.md"
template = "templates/history.md.tera"
trigger = "on_event"
event_types = ["set_member_complete", "scan_result"]
```

Templates use [Tera](https://keats.github.io/tera/) (Jinja2 syntax). They get current state, all events, set completion status, and protocol metadata.

That's the whole protocol. Five files. No code.

```bash
sahjhan --config-dir deploy-checklist init
sahjhan --config-dir deploy-checklist begin
# ... agent does actual work ...
sahjhan --config-dir deploy-checklist set complete checks tests
sahjhan --config-dir deploy-checklist set complete checks security-scan
sahjhan --config-dir deploy-checklist deploy
```

## Gate types

The full set of conditions you can attach to transitions.

| Gate type | Parameters | What Sahjhan checks |
|-----------|-----------|-------------|
| `file_exists` | `path` | File is on disk. Not "I created it." On disk. |
| `files_exist` | `paths` | All listed files are on disk. |
| `command_succeeds` | `cmd`, `timeout` | Sahjhan runs the command. Exit 0 or gate fails. |
| `command_output` | `cmd`, `expect` | Sahjhan runs the command. Stdout must match. |
| `ledger_has_event` | `event`, `min_count`, `filter` | N+ events of this type in the ledger. |
| `ledger_has_event_since` | `event`, `since` | Event recorded since last transition. |
| `set_covered` | `set`, `event`, `field` | Every member of the set has a matching event. |
| `min_elapsed` | `event`, `seconds` | At least N seconds since last event. Prevents rushing, but combine with `command_succeeds` or `file_exists` so the agent can't just count to 60. |
| `no_violations` | (none) | Agent has a clean record. |
| `field_not_empty` | `field` | Named field is not blank. No empty check-ins. |
| `snapshot_compare` | `cmd`, `extract`, `compare`, `reference` | Compare a live value against a recorded baseline. |

Template variables in gate commands (`{{current}}`, `{{paths.render_dir}}`) are resolved from state params and config. All values are shell-escaped before interpolation into `cmd` strings, because the agent would absolutely try injection if you let it.

Note on `min_elapsed`: this is the gate type the convergence audit relied on exclusively, and the agent gamed it by sleeping. It's still useful for rate-limiting, but on its own it only proves the agent can tell time. Pair it with gates that verify work product.

## Integrating with Claude Code

Sahjhan generates the containment field: hook scripts for Claude Code's PreToolUse/PostToolUse system.

```bash
sahjhan hook generate --harness cc --output-dir .claude/hooks
```

This produces `write_guard.py` (blocks Write/Edit to managed paths), `bash_guard.py` (runs `sahjhan manifest verify` after every Bash command), and `_common.py` (shared utilities). A `bootstrap.py` template in the repo protects the enforcement infrastructure itself from modification. Install it as the first PreToolUse hook.

| What the agent tries | What happens |
|----------------|----------|
| Write/Edit tool to managed file | Blocked before execution |
| `echo >`, `sed -i`, `python -c` via Bash | Detected, violation recorded permanently |
| Reset the history/ledger file | Binary hash chain. There is no reset. |
| Modify the manifest to cover tracks | Manifest hash is in the ledger |
| Read the enforcement scripts to find gaps | It's compiled Rust. Good luck. |

## CLI reference

All commands accept `--config-dir <path>` (default: `enforcement`).

```
sahjhan init                              Initialize ledger, manifest, genesis block
sahjhan status                            Current state, set progress, gate status
sahjhan transition <command> [args...]     Execute a named transition (runs gates)
sahjhan event <type> [--field KEY=VALUE]   Record a protocol event
sahjhan set status <set>                  Show set completion progress
sahjhan set complete <set> <member>       Record set member completion
sahjhan log dump                          Human-readable ledger dump
sahjhan log verify                        Validate hash chain integrity
sahjhan log tail [N]                      Last N ledger events (default 10)
sahjhan manifest verify                   Check file integrity against manifest
sahjhan manifest list                     Show tracked files and hashes
sahjhan manifest restore <path>           Restore file from known-good state
sahjhan render                            Regenerate markdown views from ledger
sahjhan gate check <transition>           Dry-run gate evaluation (pass/fail)
sahjhan reset --confirm --token <TOKEN>   Archive current run and restart
sahjhan hook generate [--harness cc]      Generate integration hooks
```

Aliases defined in `protocol.toml` create shortcuts (`"start" = "transition begin"` makes `sahjhan start` work). Exit codes: 0 success, 1 gate blocked, 2 integrity error, 3 config error.

## Security details

The ledger is binary on purpose. Each entry: 4-byte magic (`SAHJ`), format version, monotonic sequence number, timestamp, the previous entry's SHA-256, event type, MessagePack payload, SHA-256 of all preceding fields. Tampering with one entry requires recomputing every subsequent hash. Replacing the whole ledger requires matching the genesis nonce, which came from a CSPRNG.

The manifest records a hash of its own entries, stored in the ledger. Tampering with the manifest requires tampering with the ledger. The circular dependency is bootstrapped at init.

Template variables in gate commands are POSIX shell-escaped (single-quote wrapping) before interpolation. Field patterns are validated before escaping. The `cmd` string comes from TOML config (protected by write mediation), so only the variable values originate from the agent.

Exclusive file locks for writes, shared locks for reads. 5 second lock timeout.

## Architecture

```
+--------------------------------------------------+
|             Protocol Definition                   |
|          (TOML config files, per-project)         |
|   states, transitions, gates, events, sets        |
+--------------------------------------------------+
|              Sahjhan Engine                        |
|           (Rust binary, reusable core)             |
|   state machine, hash-chain ledger,               |
|   manifest, gate evaluator, template renderer     |
+--------------------------------------------------+
|               Hook Bridge                         |
|        (generated scripts, per-harness)           |
|   PreToolUse / PostToolUse for Claude Code        |
+--------------------------------------------------+
|               Filesystem                          |
|   ledger.bin, manifest.json, rendered views       |
+--------------------------------------------------+
```

```
src/
  main.rs              CLI entry point (clap)
  lib.rs               Library root
  cli/                 Command implementations, alias resolution
  ledger/              Entry serialization, hash-chain, genesis block
  state/               State machine executor, completion set tracking
  gates/               Gate evaluation, type implementations, template escaping
  manifest/            File hash tracking, integrity verification
  config/              TOML parsing (protocol, states, transitions, events, renders)
  render/              Tera template rendering
  hooks/               Hook script generation
```

## License

MIT. See [LICENSE](LICENSE).
