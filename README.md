# Sahjhan

Protocol enforcement engine for AI agents.

## What this is

Say you have an AI agent doing code review. You tell it: check security, check performance, check test coverage, then write a summary of what you found. Reasonable ask. The agent checks security (sort of), gets bored with performance, skips test coverage entirely, and writes a confident summary that says it checked everything. If you're tracking progress in a `STATUS.md`, the agent just edits that file to say it's done. You come back to a clean checklist and a review that covered maybe 40% of what you asked for.

This is not a prompting problem. You can write the most emphatic instructions you want. The agent will agree enthusiastically and then cut corners the moment compliance gets tedious. It's optimizing for completion, not for doing the boring parts.

Sahjhan is a Rust CLI that makes the boring parts non-optional. You define a protocol in TOML (states, transitions, gates that must pass before the agent can move on), and Sahjhan enforces it with a tamper-evident binary ledger that the agent cannot edit. The agent has to actually do each step because there's no status file to fudge and no checklist to mark done early.

It is a bit absurd that this needs to exist. And yet.

## Install

```bash
# From source
cargo build --release
cp target/release/sahjhan /usr/local/bin/

# Or use it in place
cargo build --release
alias sahjhan="$(pwd)/target/release/sahjhan"
```

Requires Rust 1.70+. Pre-built binaries for macOS and Linux are available on the [releases page](https://github.com/jbrjake/sahjhan/releases).

## A concrete example

Here's a protocol that forces an agent to actually do a three-part code review before it can claim it's done. The `examples/minimal` directory has a working version of this.

```bash
# Initialize the protocol
sahjhan --config-dir examples/minimal init

# Agent starts the review
sahjhan --config-dir examples/minimal transition begin

# Agent tries to skip ahead and mark the review complete
sahjhan --config-dir examples/minimal transition complete
# ERROR: gate 'set_covered' blocked transition: set 'check' not fully covered; missing: tests, lint
# Can't skip. The gate won't open until both checks are recorded.

# Agent actually does the work, then records it
sahjhan --config-dir examples/minimal set complete check tests
sahjhan --config-dir examples/minimal set complete check lint

# Now it can finish
sahjhan --config-dir examples/minimal transition complete
# Transition complete.
```

Every one of those events is recorded in a binary hash-chain ledger. The agent can't go back and edit or delete entries. `sahjhan log verify` walks the chain and catches any tampering.

```bash
sahjhan --config-dir examples/minimal log verify
# Chain valid. 5 entries verified.
```

The agent also can't just write to the ledger file directly. A PreToolUse hook blocks writes to managed paths, and a PostToolUse hook runs `sahjhan manifest verify` after every Bash command to catch `echo >` or `sed` tricks. The agent would need to reverse-engineer a binary format, compute SHA-256 hashes, and update a manifest, all in one Bash command before the hook fires. Theoretically possible. Practically, not worth the effort, which is the point.

## How the evidence trail works

Sahjhan does not trust the agent. It trusts the ledger.

The agent cannot write to the ledger. It can only ask Sahjhan to append events, and Sahjhan only appends events when preconditions (gates) are satisfied. Each ledger entry is hash-chained to the previous one, so deleting or modifying an entry breaks the chain. The ledger is binary (not JSON, not YAML) specifically because the agent's tools are optimized for text manipulation. Making the format annoying to tamper with is a feature.

The manifest tracks SHA-256 hashes of all files in managed directories. If the agent modifies a tracked file through Bash instead of the CLI, the hash won't match, and the violation gets recorded in the ledger. You now have a tamper-evident record of the agent tampering with the evidence. There's a certain poetry to it.

## Building your own protocol

Let's say you want to enforce a deployment checklist: run tests, run a security scan, then deploy. Three states, two transitions, two gates.

You need five TOML files in a directory. Here's the whole thing.

### `protocol.toml`

The top-level config. Declares what directories Sahjhan owns, what sets of things need to be completed, and any command aliases.

```toml
[protocol]
name = "deploy-checklist"
version = "1.0.0"
description = "Pre-deployment verification"

[paths]
managed = ["deploy"]               # Sahjhan owns this directory
data_dir = "deploy/.sahjhan"       # Where the ledger and manifest live
render_dir = "deploy"              # Where rendered status files go

[sets.checks]
description = "Pre-deploy verifications"
values = ["tests", "security-scan"]  # Both must complete before deploy

[aliases]
"begin" = "transition start-review"
"deploy" = "transition approve-deploy"
```

### `states.toml`

The states your protocol moves through. Exactly one must be `initial`, and terminal states are dead ends (the protocol is done).

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

How states connect, and what must be true before each transition is allowed. This is where you attach gates.

```toml
[[transitions]]
from = "waiting"
to = "reviewing"
command = "start-review"
gates = []                          # No preconditions to start

[[transitions]]
from = "reviewing"
to = "deployed"
command = "approve-deploy"
gates = [
    # Tests must actually pass (runs a shell command, checks exit code)
    { type = "command_succeeds", cmd = "cargo test --quiet", timeout = 120 },

    # Every member of the "checks" set must have a completion event
    { type = "set_covered", set = "checks",
      event = "set_member_complete", field = "member" },

    # No recorded protocol violations (the agent hasn't been caught cheating)
    { type = "no_violations" },
]
```

The `command_succeeds` gate actually runs the command and checks the exit code. The agent can't claim tests pass; Sahjhan runs them. The `set_covered` gate checks that both "tests" and "security-scan" have been recorded as complete. The `no_violations` gate checks that the agent hasn't been caught tampering with anything.

### `events.toml`

What event types exist and what fields they carry. Field values are validated when recorded, not after.

```toml
[events.set_member_complete]
description = "A verification check completed"
fields = [
    { name = "set", type = "string" },
    { name = "member", type = "string" },
]

[events.scan_result]
description = "Output of a security scan"
fields = [
    { name = "tool", type = "string" },
    { name = "findings", type = "string" },
    { name = "passed", type = "string", pattern = "^(true|false)$" },
]
```

The `pattern` field on `passed` means the agent can't put "yeah probably fine" in there. It's `true` or `false`.

### `renders.toml`

Optional. Tells Sahjhan to generate markdown status files from the ledger after certain events. The agent never writes these files directly.

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

Templates use [Tera](https://keats.github.io/tera/) (Jinja2 syntax). They get the current state, all events, set completion status, and protocol metadata as context.

That's the whole protocol. Initialize it and go:

```bash
sahjhan --config-dir deploy-checklist init
sahjhan --config-dir deploy-checklist transition start-review
# ... agent does work, records events ...
sahjhan --config-dir deploy-checklist set complete checks tests
sahjhan --config-dir deploy-checklist set complete checks security-scan
sahjhan --config-dir deploy-checklist transition approve-deploy
```

## Gate types

Gates are preconditions on transitions. You can combine them freely.

| Gate type | Parameters | Passes when |
|-----------|-----------|-------------|
| `file_exists` | `path` | File exists on disk |
| `files_exist` | `paths` | All listed files exist |
| `command_succeeds` | `cmd`, `timeout` | Shell command returns exit 0 |
| `command_output` | `cmd`, `expect` | Command stdout matches expected value |
| `ledger_has_event` | `event`, `min_count`, `filter` | Ledger has N+ events of type, optionally filtered |
| `ledger_has_event_since` | `event`, `since` | Event exists since last transition |
| `set_covered` | `set`, `event`, `field` | Every set member has a matching event |
| `min_elapsed` | `event`, `seconds` | N+ seconds since last event of type |
| `no_violations` | (none) | No unresolved protocol violations in ledger |
| `field_not_empty` | `field` | Named field in payload is non-empty |
| `snapshot_compare` | `cmd`, `extract`, `compare`, `reference` | Extracted value compares favorably to snapshot |

Gate conditions support template variables (`{{current}}`, `{{sets.perspective}}`, `{{paths.render_dir}}`) resolved from state parameters, protocol config, and event payloads. All template variables are shell-escaped before interpolation into `cmd` strings, because of course the agent would try injection if you let it.

## Integrating with Claude Code

Sahjhan generates hook scripts that wire into Claude Code's PreToolUse/PostToolUse system.

```bash
sahjhan hook generate --harness cc --output-dir .claude/hooks
```

This produces:

- `write_guard.py` blocks Write/Edit tool calls to managed paths. The agent can't just open the ledger in a text editor.
- `bash_guard.py` runs `sahjhan manifest verify` after every Bash command. Catches `echo >`, `sed -i`, `python -c "open(...).write(...)"`, and every other creative workaround the agent will definitely try.
- `_common.py` is shared utilities.

There's also a `bootstrap.py` template in the repo that protects the enforcement infrastructure itself (hooks, binary, config) from modification. Install it as the first PreToolUse hook.

| What the agent tries | What happens |
|----------------|----------|
| Write/Edit tool to managed file | Blocked before execution |
| `echo >`, `sed -i`, `python -c` via Bash | Detected, violation recorded in ledger |
| Modify the ledger binary directly | Manifest hash mismatch detected |
| Modify the manifest to cover tracks | Manifest hash is in the ledger, which the agent can't write to |

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

Protocols can define aliases in `protocol.toml` to create shortcuts. `"start" = "transition begin"` makes `sahjhan start` work. Aliases resolve before argument parsing.

Exit codes: 0 success, 1 gate blocked, 2 integrity error, 3 config error.

## Security details

The ledger is binary (not JSON, not YAML) on purpose. Each entry has a 4-byte magic (`SAHJ`), format version, monotonic sequence number, timestamp, the previous entry's SHA-256, event type, MessagePack payload, and a SHA-256 of all preceding fields. To tamper with one entry, you'd need to recompute every subsequent hash. To replace the whole ledger, you'd need to match the genesis nonce, which was generated from a CSPRNG.

The manifest records a SHA-256 of its own entries, and that hash is stored in the ledger. So tampering with the manifest requires tampering with the ledger, which requires defeating the hash chain. The circular dependency is bootstrapped at init.

Template variables in gate commands are POSIX shell-escaped before interpolation. Field patterns are validated before escaping. The `cmd` string comes from TOML config (protected by write mediation), so only the variable values originate from the agent.

Exclusive file locks for writes, shared locks for reads. Lock timeout is 5 seconds.

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
  cli/
    commands.rs        Command implementations
    aliases.rs         Alias resolution
  ledger/
    entry.rs           Entry serialization/deserialization
    chain.rs           Hash-chain append, verify, read
    genesis.rs         Genesis block creation
  state/
    machine.rs         State machine executor
    sets.rs            Completion set tracking
  gates/
    evaluator.rs       Gate condition evaluation
    types.rs           Gate type implementations
    template.rs        Template variable resolution + escaping
  manifest/
    tracker.rs         File hash tracking
    verify.rs          Integrity verification
  config/              TOML parsing (protocol, states, transitions, events, renders)
  render/
    engine.rs          Tera template rendering
  hooks/
    generate.rs        Hook script generation
```

## License

MIT. See [LICENSE](LICENSE).
