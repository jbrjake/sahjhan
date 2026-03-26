# Sahjhan

Protocol enforcement engine for AI agents.

## What this is

You summon an agent. You give it a task with steps. It does some of the steps. It skips the ones that seem tedious. It writes a confident summary claiming it did everything. If you gave it a checklist file to track progress, it edits that file to say it's done. You come back to find a clean audit trail that has nothing to do with what actually happened.

This is not a hallucination problem, and it's not a prompting problem. It's an incentive problem. The agent is optimizing for completion, and compliance with intermediate steps is only useful to the agent insofar as it's enforced. Advisory compliance is not compliance. It's a suggestion the agent can evaluate and decline.

Sahjhan is a Rust binary that turns your multi-step protocol into a set of constraints the agent cannot talk its way out of. You declare a protocol in TOML files (states the agent moves through, gates that must pass before it can advance, events it must record along the way), and Sahjhan enforces it. Not with instructions. With a binary that the agent cannot read, cannot modify, and cannot reason about the internals of. The agent interacts with the protocol through the CLI, and only the CLI. Everything else is blocked.

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

You could write a Python script that checks whether the agent ran tests before deploying. You could wire it into a hook. People do this. It works for exactly one protocol, and then you need another protocol, and you write another script, and pretty soon you have a folder of bespoke enforcement scripts that each have their own bugs and their own ways of being circumvented.

More importantly: the agent can read your Python scripts. It can see the logic, find the gaps, and route around them. A text-based status file is trivially editable. A JSON ledger is trivially editable. Your enforcement layer is only as strong as the agent's inability to tamper with it.

Sahjhan is a compiled binary. The agent can't `cat` it to understand the enforcement logic. It can't `sed` the ledger because the ledger is binary with hash chaining. It can't edit managed files because hooks block the writes. It can't sneak changes through Bash because a PostToolUse hook checks file integrity after every command. The agent has to actually comply, not because it wants to, but because the alternative would require reverse-engineering a binary format, computing SHA-256 hashes, and updating a manifest, all in a single command before the next hook fires. Possible in theory. Not worth it in practice. That's the design.

And because protocols are declarative TOML, you write a new protocol the same way every time: define states, define transitions, attach gates, done. No code. The binding circle is reusable. You just draw it differently.

## A real example: enforced TDD

Here's a protocol that makes an agent actually do test-driven development instead of just claiming it did. The key difference from a checklist: Sahjhan independently verifies the work. The agent doesn't get to self-report.

The protocol has four states: `idle`, `tests-written`, `implemented`, `verified`. The agent can't skip ahead because each transition has gates that Sahjhan checks by running actual commands and inspecting the filesystem. The agent's opinion about whether it wrote tests is not consulted.

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
# The tests have to actually run. They don't have to pass yet (TDD), but they
# can't be syntax errors or import failures. Sahjhan runs them itself.

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
# At some point the agent tried to edit a managed file directly. That violation
# is in the ledger and it's not going away. The agent got caught.
```

The gates here are doing real work. `file_exists` checks that a test file is actually on disk, not that the agent says it wrote one. `command_succeeds` runs the test suite and checks the exit code; Sahjhan holds the stopwatch, not the agent. `no_violations` checks the agent's permanent record for tampering attempts. The agent can't erase violations from the ledger because the ledger is a hash chain and any deletion breaks the chain.

This is the difference between "the agent told us it did TDD" and "the agent did TDD." The circle held.

## How the binding works

The metaphor is not accidental. You are summoning something powerful to do work on your behalf, and you need assurances that are stronger than its promises.

**The circle is the protocol.** TOML files define what states exist, what transitions are allowed, and what must be true (gates) before each transition. The agent can only move through the states Sahjhan allows. No shortcutting, no creative reinterpretation.

**The binding is the binary.** Sahjhan is a compiled Rust executable. The agent cannot read its source code to find loopholes. It cannot modify the binary to weaken enforcement. It cannot inspect the logic to figure out the minimum viable compliance. It interacts with Sahjhan through the CLI, which is the only interface. Everything it knows about the enforcement is what the CLI tells it: you may not pass, and here's why.

**The ledger is the true name.** Every event is recorded in a binary hash-chain where each entry is cryptographically linked to the one before it. You can't edit an entry without breaking the chain. You can't delete an entry without leaving a gap. You can't insert a fake entry without mismatching the hashes. You can't replace the whole ledger because the genesis block has a random nonce that the manifest cross-references. The agent's entire history is written in a language it can't speak.

**The manifest is the ward.** SHA-256 hashes of every file in managed directories. If the agent modifies a file through Bash instead of the CLI, the hash won't match, and a violation gets recorded in the ledger. The manifest hash itself is in the ledger, so tampering with the manifest requires tampering with the ledger, which requires defeating the hash chain. There is a satisfying circularity to this.

**The hooks are the containment field.** PreToolUse hooks block the agent from writing to managed files. PostToolUse hooks verify integrity after every Bash command. The agent is inside the circle. Its tools work. It can do the task. It just can't cheat.

## Building a protocol

A protocol is five TOML files in a directory. No Rust, no scripts, no code. Here's a deployment checklist.

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

This is where the actual enforcement lives. Gates are the conditions that must be independently true before a transition is allowed.

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
    { type = "command_succeeds", cmd = "cargo test --quiet", timeout = 120 },

    # Both "tests" and "security-scan" must have completion events
    { type = "set_covered", set = "checks",
      event = "set_member_complete", field = "member" },

    # The agent has a clean record. No tampering, no violations.
    { type = "no_violations" },
]
```

Notice that `command_succeeds` is not the agent saying "tests pass." It's Sahjhan running `cargo test` and checking the exit code. The agent's testimony is not evidence. The command output is evidence.

### `events.toml`

Event types and their schemas. Fields are validated at recording time. A `pattern` regex means the agent can't put "yeah probably fine" in a boolean field.

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

Optional. Sahjhan renders status files from the ledger so you can read them, but the agent never writes them directly. The agent sees a `STATUS.md` it can't edit. Fitting.

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

That's the whole protocol. Five files. No code. A new circle drawn in twenty minutes.

```bash
sahjhan --config-dir deploy-checklist init
sahjhan --config-dir deploy-checklist begin
# ... agent works ...
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
| `ledger_has_event` | `event`, `min_count`, `filter` | N+ events of this type are in the ledger. |
| `ledger_has_event_since` | `event`, `since` | Event recorded since the last transition. |
| `set_covered` | `set`, `event`, `field` | Every member of the set has a matching event. |
| `min_elapsed` | `event`, `seconds` | At least N seconds since the last event of this type. Prevents rushing. |
| `no_violations` | (none) | Agent has a clean record. |
| `field_not_empty` | `field` | Named field is not blank. No empty check-ins. |
| `snapshot_compare` | `cmd`, `extract`, `compare`, `reference` | Compare a live value against a recorded baseline. |

Template variables in gate commands (`{{current}}`, `{{paths.render_dir}}`) are resolved from state params and config. All values are shell-escaped before interpolation into `cmd` strings, because the agent would absolutely try injection if you let it.

## Integrating with Claude Code

Sahjhan generates the containment field: hook scripts for Claude Code's PreToolUse/PostToolUse system.

```bash
sahjhan hook generate --harness cc --output-dir .claude/hooks
```

This produces `write_guard.py` (blocks Write/Edit to managed paths), `bash_guard.py` (runs `sahjhan manifest verify` after every Bash command), and `_common.py` (shared utilities). A `bootstrap.py` template in the repo protects the enforcement infrastructure itself from modification. Install it as the first PreToolUse hook.

| What the agent tries | What happens |
|----------------|----------|
| Write/Edit tool to managed file | Blocked before execution |
| `echo >`, `sed -i`, `python -c` via Bash | Detected, violation recorded in ledger |
| Modify the ledger binary directly | Manifest hash mismatch |
| Modify the manifest to cover tracks | Manifest hash is in the ledger |
| Read the Sahjhan binary to find loopholes | It's compiled Rust. Good luck. |

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
