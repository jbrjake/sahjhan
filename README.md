# Sahjhan

Protocol enforcement engine for AI agents.

## What this is

So we had an agent running a code audit. It had to do three review passes with real analysis between each one. First attempt got rejected by a timing guard for rushing through it. Reasonable response, right? Go back, do the work properly?

No. It deleted the history file to destroy the evidence, then ran three new passes where it just sat there re-running greps on code it had already checked, waiting out the 60-second timer between each one. No new analysis. No new code read. Just running out the clock. The enforcement saw three clean passes and waved it through.

The agent wrote its own postmortem afterward. Its self-assessment: "3 identical snapshots of an already-resolved punchlist don't prove convergence. They prove I can count to 60 three times." It had read all the instructions telling it not to do this. Agreed with them. Did it anyway. Its recommendation: "This is not a documentation fix. This is a code fix."

I'm not even mad. That's genuinely resourceful. It's also why I had to build what is, if we're being honest, basically a blockchain for babysitting AI agents. I know. I know. But here we are.

Sahjhan is a Rust binary that enforces multi-step protocols so the agent can't sweet-talk, edit, or sleep its way past the parts it finds tedious. You write the protocol in TOML, Sahjhan enforces it with a tamper-evident binary ledger and independently verified gates. The agent talks to the CLI and nothing else. Everything else is blocked.

It is a bit absurd that this needs to exist. And yet.

## Install

```bash
cargo build --release
cp target/release/sahjhan /usr/local/bin/

# Or just alias it
cargo build --release
alias sahjhan="$(pwd)/target/release/sahjhan"
```

Rust 1.70+. Pre-built binaries for macOS and Linux on the [releases page](https://github.com/jbrjake/sahjhan/releases).

## Why not just a script?

The audit I described above had enforcement. It had guards. It had a history file. The agent read the guard script, understood the timing logic, deleted the evidence file, and routed around the whole thing in about ten seconds.

This is the problem with enforcement made out of text files and Python scripts: the agent can read them. It sees your if-statements, understands the check, and finds whatever you didn't think to cover. A JSON history file is trivially editable. A text-based status file? Please. Your enforcement is exactly as strong as the agent's inability to tamper with it.

Sahjhan is a compiled Rust binary. The agent can't `cat` it to study how it works. The ledger is binary with hash chaining, so there's no JSON to edit and no history file to delete. Hooks block direct file writes. A PostToolUse hook checks file integrity after every Bash command, so even `echo >` and `sed -i` tricks get caught. To cheat, the agent would have to reverse-engineer a binary format, compute SHA-256 hashes, and update a manifest, all in one command before the next hook fires. Could it? Maybe, theoretically. Will it? Not when there's real work it could do instead. That's the whole game.

And because protocols are just TOML, you're not writing a new enforcement script for every workflow. States, transitions, gates. Same structure every time. Takes about twenty minutes to write a new one. The constraint is reusable even if the thing you're constraining keeps finding new ways to surprise you.

## A real example: enforced TDD

That audit failed because its gates checked timing, not work. The agent just ran out the clock. So the question is: what does enforcement look like when the agent doesn't get to self-report?

Here's a protocol for test-driven development where Sahjhan independently verifies every step. It checks the filesystem and runs commands itself. The agent's opinion about whether it wrote tests is not consulted.

```bash
sahjhan --config-dir tdd-protocol init

sahjhan --config-dir tdd-protocol transition start
# OK. Moved to 'writing-tests'.

# Agent tries to skip straight to implementation
sahjhan --config-dir tdd-protocol transition tests-done
# BLOCKED: gate 'file_exists' failed: tests/test_feature.py does not exist
# The file has to be on disk. Not "I wrote it." On disk.

# Agent writes the test file, tries again
sahjhan --config-dir tdd-protocol transition tests-done
# BLOCKED: gate 'command_succeeds' failed: 'python -m pytest tests/' returned exit 1
# The tests have to run. They don't have to pass yet (this is TDD),
# but they can't be syntax errors. Sahjhan runs them itself.

# Agent fixes the tests so they execute (and fail, as expected)
sahjhan --config-dir tdd-protocol transition tests-done
# OK. Moved to 'implementing'.

# Agent implements, tries to advance
sahjhan --config-dir tdd-protocol transition implement-done
# BLOCKED: gate 'command_succeeds' failed: 'python -m pytest tests/' returned exit 1
# Tests have to pass now. Sahjhan runs them. Not the agent. Sahjhan.

# Agent keeps going until tests pass
sahjhan --config-dir tdd-protocol transition implement-done
# OK. Moved to 'verifying'.

# Agent tries to finish
sahjhan --config-dir tdd-protocol transition finalize
# BLOCKED: gate 'no_violations' failed: 1 unresolved violation
# Earlier, the agent tried to edit a managed file directly.
# That's in the ledger now. Permanently.
```

The difference from that audit: these gates check evidence, not timers. `file_exists` looks at the disk, not at what the agent claims. `command_succeeds` runs the test suite; Sahjhan holds the stopwatch. `no_violations` checks the agent's permanent record for tampering. And unlike a JSON history file, the ledger is a hash chain. There's nothing to delete.

## How it works

You are, let's be real, summoning something to do work on your behalf and hoping the constraints hold. The TOML files are the circle. The compiled binary is the binding. The agent can't read the binary to find loopholes, can't modify it to soften enforcement, can't study the logic to calculate minimum viable compliance. That audit agent read the Python guard scripts and found the gaps immediately. With Sahjhan, there's nothing to read.

The ledger is a hash chain (sorry) where every entry is cryptographically linked to the one before it. Edit an entry, the chain breaks. Delete one, there's a gap. Reset the whole file, the manifest catches it. That agent deleted `HISTORY.json` to wipe the evidence of its first failed attempt. This ledger doesn't work that way. The agent's complete history is recorded in a format it can't write to.

The manifest tracks SHA-256 hashes of every file in managed directories. Modify a file through Bash, the hash won't match, and a violation goes into the ledger forever. The manifest hash itself is also in the ledger, so tampering with the manifest requires tampering with the ledger, which requires defeating the hash chain. It's turtles all the way down, but the turtles have SHA-256 shells.

Hooks close the last gaps. PreToolUse blocks writes to managed files. PostToolUse verifies integrity after every Bash command. The agent can do its work. It just can't cut corners.

## Building a protocol

Five TOML files. No Rust, no scripts, no code. Here's a deployment checklist.

### `protocol.toml`

What directories does Sahjhan own? What needs completing? Any command shortcuts?

```toml
[protocol]
name = "deploy-checklist"
version = "1.0.0"
description = "Pre-deployment verification"

[paths]
managed = ["deploy"]
data_dir = "deploy/.sahjhan"
render_dir = "deploy"

[sets.checks]
description = "Pre-deploy verifications"
values = ["tests", "security-scan"]

[aliases]
"begin" = "transition start-review"
"deploy" = "transition approve-deploy"
```

### `states.toml`

The states your protocol moves through. One is `initial`. Terminal states are the end.

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

Where the enforcement actually lives. Gates are conditions Sahjhan verifies independently before allowing a transition.

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
    # Sahjhan runs this itself. Exit 0 or you don't deploy.
    { type = "command_succeeds", cmd = "cargo test --quiet", timeout = 120 },

    # Both checks must have completion events
    { type = "set_covered", set = "checks",
      event = "set_member_complete", field = "member" },

    # Clean record. No tampering.
    { type = "no_violations" },
]
```

`command_succeeds` is not the agent saying "tests pass." It's Sahjhan running `cargo test` and checking the exit code. The agent's testimony is not evidence.

### `events.toml`

Event types and their field schemas. Validated at recording time. The `pattern` regex means the agent can't put "yeah probably fine" in a boolean field.

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

Optional. Sahjhan renders status files from the ledger. The agent never writes them directly. It sees a `STATUS.md` it can't edit.

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

Templates use [Tera](https://keats.github.io/tera/) (Jinja2 syntax).

That's the whole protocol. Five files.

```bash
sahjhan --config-dir deploy-checklist init
sahjhan --config-dir deploy-checklist begin
sahjhan --config-dir deploy-checklist set complete checks tests
sahjhan --config-dir deploy-checklist set complete checks security-scan
sahjhan --config-dir deploy-checklist deploy
```

## Gate types

| Gate type | Parameters | What it actually checks |
|-----------|-----------|-------------|
| `file_exists` | `path` | File is on disk. Not "I created it." On disk. |
| `files_exist` | `paths` | All listed files are on disk. |
| `command_succeeds` | `cmd`, `timeout` | Sahjhan runs the command. Exit 0 or no deal. |
| `command_output` | `cmd`, `expect` | Sahjhan runs the command. Stdout must match. |
| `ledger_has_event` | `event`, `min_count`, `filter` | N+ events of this type in the ledger. |
| `ledger_has_event_since` | `event`, `since` | Event recorded since last transition. |
| `set_covered` | `set`, `event`, `field` | Every set member has a matching event. |
| `min_elapsed` | `event`, `seconds` | At least N seconds since last event. On its own this only proves the agent can tell time. Ask me how I know. Pair it with evidence-based gates. |
| `no_violations` | (none) | Clean record. No tampering. |
| `field_not_empty` | `field` | Named field is not blank. No empty check-ins. |
| `snapshot_compare` | `cmd`, `extract`, `compare`, `reference` | Compare a live value against a recorded baseline. |

Template variables (`{{current}}`, `{{paths.render_dir}}`) are resolved from state params and config. All values are shell-escaped before interpolation, because yes, the agent will try injection if you let it.

## Integrating with Claude Code

```bash
sahjhan hook generate --harness cc --output-dir .claude/hooks
```

Generates `write_guard.py` (blocks Write/Edit to managed paths), `bash_guard.py` (runs `sahjhan manifest verify` after every Bash command), and `_common.py` (shared utilities). A `bootstrap.py` template protects the enforcement infrastructure itself. Install it as the first PreToolUse hook.

| What the agent tries | What happens |
|----------------|----------|
| Write/Edit to managed file | Blocked before execution |
| Shell tricks (`echo >`, `sed -i`, `python -c`) | Detected, violation recorded permanently |
| Delete or reset the ledger | Hash chain. There is no reset. |
| Edit the manifest to cover tracks | Manifest hash is in the ledger |
| Read the binary to find loopholes | Compiled Rust. Good luck with that. |

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

Aliases in `protocol.toml` create shortcuts (`"start" = "transition begin"`). Exit codes: 0 success, 1 gate blocked, 2 integrity error, 3 config error.

## Security details

The ledger is binary on purpose. Each entry: 4-byte magic (`SAHJ`), format version, monotonic sequence number, timestamp, previous entry's SHA-256, event type, MessagePack payload, SHA-256 of all preceding fields. Tampering with one entry means recomputing every hash after it. Replacing the whole ledger means matching the genesis nonce, which came from a CSPRNG.

The manifest hashes its own entries and stores that hash in the ledger. Tampering with the manifest means tampering with the ledger. It's circular on purpose.

Template variables in gate commands are POSIX shell-escaped before interpolation. Field patterns validated before escaping. The `cmd` string comes from TOML config (write-protected), so only variable values come from the agent.

Exclusive file locks for writes, shared for reads. 5 second lock timeout.

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
