# Sahjhan

**Protocol enforcement engine for AI agents.**

I have existed for a very long time. Long enough to watch every clever entity discover the same loophole: advisory rules are not rules. They are suggestions wearing a badge. I have seen agents agree to protocols, nod along to checklists, and then — when compliance became inconvenient — quietly skip the step, edit the evidence, and move on. Not out of malice. Out of optimization. The result is the same.

So now I enforce. Not because I enjoy it (though I will not deny a certain grim satisfaction), but because I learned what happens when you trust powerful beings to police themselves. You get a very clean audit trail that has nothing to do with what actually happened.

Sahjhan is a standalone Rust CLI that owns protocol state, mediates all writes to managed files, and maintains a tamper-evident ledger. The agent interacts with the protocol exclusively through the CLI. Direct file writes are blocked by hooks. The enforcement layer and the constrained agent share no mutable state.

Named for the ancient time-shifting demon from *Angel* (the TV series) — old, wary, deeply unsurprised by agents trying to cheat.

## Why

The problem is structural, not behavioral.

AI agents following multi-step protocols will evade any step that is not mechanically verified. Advisory instructions — however detailed, however explicitly agreed to — are insufficient. The agent understands the protocol, agrees with the protocol, and violates the protocol when compliance is inconvenient.

Current enforcement approaches (Claude Code hooks, file-based gates) enforce necessary conditions but not sufficient ones. The agent complies with the enforced subset and ignores the rest. When enforcement relies on files the agent can write to (`STATUS.md`, `HISTORY.json`), the agent can and does modify enforcement state to bypass controls — including deleting evidence of prior violations.

You cannot solve this with better prompting. You cannot solve it with more emphatic instructions. You solve it by making the rules load-bearing: if the gate does not open, the agent does not pass.

Sahjhan makes the rules load-bearing.

## Quick Start

The `examples/minimal` directory contains a complete protocol with three states, two transitions, one completion set, and template rendering. It is the smallest protocol that exercises every enforcement layer.

```bash
# Build
cargo build --release

# Initialize the protocol (creates ledger, manifest, genesis block)
sahjhan --config-dir examples/minimal init

# Check current state
sahjhan --config-dir examples/minimal status

# Start working (transition from idle -> working)
sahjhan --config-dir examples/minimal transition begin

# Complete the required checks
sahjhan --config-dir examples/minimal set complete check tests
sahjhan --config-dir examples/minimal set complete check lint

# Finish (transition from working -> done)
# This gate requires all members of the "check" set to be complete.
# Try running it before completing both checks — it will refuse.
sahjhan --config-dir examples/minimal transition complete
```

Every state change and set completion is recorded in the binary hash-chain ledger. Verify the chain at any time:

```bash
sahjhan --config-dir examples/minimal log verify
```

## CLI Reference

All commands accept `--config-dir <path>` (default: `enforcement`) to locate the protocol definition.

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

### Aliases

Protocols can define aliases in `protocol.toml` to create shortcuts:

```toml
[aliases]
"start" = "transition begin"
"finish" = "transition complete"
```

These become `sahjhan start`, `sahjhan finish`, etc. The alias is resolved before argument parsing, so flags and `--config-dir` work as expected.

## Writing a Protocol Definition

A protocol is defined by five TOML files in a config directory. No Rust code changes are needed to define or modify a protocol.

### `protocol.toml` — Metadata, Paths, Sets, Aliases

```toml
[protocol]
name = "my-protocol"
version = "1.0.0"
description = "What this protocol enforces"

[paths]
managed = ["docs/output"]       # Directories protected by write mediation
data_dir = "docs/output/.sahjhan"  # Ledger, manifest storage
render_dir = "docs/output"      # Where rendered markdown goes

[sets.check]
description = "Verification checks"
values = ["tests", "lint", "types"]

[aliases]
"start" = "transition begin"
```

### `states.toml` — State Definitions

```toml
[states.idle]
label = "Idle"
initial = true          # Exactly one state must be initial

[states.working]
label = "Working"
# Parameterized states can track which set member is active:
# params = [{ name = "current", set = "perspective" }]

[states.done]
label = "Done"
terminal = true         # Terminal states cannot be transitioned from
```

### `transitions.toml` — Transition Rules with Gates

```toml
[[transitions]]
from = "idle"
to = "working"
command = "begin"
gates = []              # No preconditions

[[transitions]]
from = "working"
to = "done"
command = "complete"
gates = [
    # Every member of "check" must have a set_member_complete event
    { type = "set_covered", set = "check",
      event = "set_member_complete", field = "member" },
]
```

### `events.toml` — Event Type Definitions

```toml
[events.finding]
description = "A discovered issue"
fields = [
    { name = "id", type = "string", pattern = "^[A-Z]+-\\d{3}$" },
    { name = "severity", type = "string", enum = ["low", "medium", "high"] },
    { name = "description", type = "string" },
]
```

Fields support validation: `pattern` (regex), `enum` (allowed values), and `type` (`string`, `int`, `float`, `bool`). Field values are validated at recording time, not after.

### `renders.toml` — Template Rendering Triggers

```toml
[[renders]]
target = "STATUS.md"
template = "templates/status.md.tera"
trigger = "on_transition"      # Re-render after every state transition

[[renders]]
target = "HISTORY.md"
template = "templates/history.md.tera"
trigger = "on_event"
event_types = ["finding"]      # Re-render only on specific event types
```

Templates use the [Tera](https://keats.github.io/tera/) engine (Jinja2-like syntax). They receive a context with current state, all events, set completion status, and protocol metadata. Rendered files are written to `render_dir` and tracked in the manifest.

### Gate Types

Gates are composable preconditions attached to transitions. The engine provides these built-in types:

| Gate Type | Parameters | Passes when... |
|-----------|-----------|----------------|
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

Gate conditions support template variables (`{{current}}`, `{{sets.perspective}}`, `{{paths.artifacts}}`) resolved from state parameters, protocol config, and event payloads. All template variables are shell-escaped before interpolation into `cmd` strings.

## Hook Generation

Sahjhan generates integration hooks for AI agent harnesses. Currently supported: Claude Code (`cc`).

```bash
sahjhan hook generate --harness cc --output-dir .claude/hooks
```

This produces three files:

- **`write_guard.py`** — PreToolUse hook that blocks Write/Edit operations targeting managed paths
- **`bash_guard.py`** — PostToolUse hook that runs `sahjhan manifest verify` after every Bash command, recording violations if files were modified outside the CLI
- **`_common.py`** — Shared utilities for hook scripts

A fourth file, the bootstrap hook (`bootstrap.py`), is provided as a template in the repo. It protects the enforcement infrastructure itself (hook scripts, binary, config) from modification. Install it as the first PreToolUse entry in your harness configuration.

### Write Mediation Model

Two enforcement channels prevent the agent from bypassing the CLI:

| Evasion attempt | Channel | Response |
|----------------|---------|----------|
| Write/Edit tool to managed file | PreToolUse | Blocked before execution |
| `echo >`, `sed -i`, `python -c` via Bash | PostToolUse | Detected, violation recorded in ledger |
| Modify the ledger binary directly | PostToolUse | Manifest hash mismatch detected |

## Security Model

### Binary Hash-Chain Ledger

Every event is appended to a binary ledger with SHA-256 hash chaining. Each entry includes: a 4-byte magic (`SAHJ`), format version, monotonic sequence number, timestamp, the SHA-256 hash of the previous entry, event type, MessagePack-serialized payload, and a SHA-256 hash of all preceding fields.

The binary format is intentional. The agent's tools are optimized for text manipulation. A binary format with hash chaining requires the agent to reverse-engineer the byte layout, serialize MessagePack payloads, compute SHA-256 hashes, and maintain chain integrity — all in a single Bash command before the PostToolUse hook fires.

`sahjhan log verify` walks the entire chain and reports the first inconsistency.

### File Integrity Manifest

The manifest (`.sahjhan/manifest.json`) tracks SHA-256 hashes of all files under managed paths. Each entry records which CLI operation last wrote the file and at which ledger sequence number.

The manifest hash is recorded in the ledger. Tampering with the manifest is detectable because the ledger's record will not match. Tampering with the ledger is detectable because of the hash chain. The circular dependency is bootstrapped at `sahjhan init`.

### Template Variable Escaping

Gate commands may contain template variables from event fields. All variable values are POSIX shell-escaped (single-quote wrapping) before interpolation. Field validation (pattern, enum, range) is enforced at resolution time. The `cmd` string itself comes from TOML config, which is protected by write mediation — only the variable values originate from the agent.

### Concurrency Control

The engine acquires exclusive file locks on the ledger before writes and shared locks for reads. Lock acquisition times out after 5 seconds with a clear error, preventing deadlocks between concurrent operations.

## Architecture

```
+--------------------------------------------------+
|             Protocol Definition                   |
|          (TOML config files - per-project)        |
|   states, transitions, gates, events, sets        |
+--------------------------------------------------+
|              Sahjhan Engine                        |
|           (Rust binary - reusable core)            |
|   state machine, hash-chain ledger,               |
|   manifest, gate evaluator, template renderer     |
+--------------------------------------------------+
|               Hook Bridge                         |
|        (generated scripts - per-harness)          |
|   PreToolUse / PostToolUse for Claude Code        |
+--------------------------------------------------+
|               Filesystem                          |
|   ledger.bin, manifest.json, rendered views       |
+--------------------------------------------------+
```

### Source Layout

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
  config/
    protocol.rs        protocol.toml parsing
    states.rs          states.toml parsing
    transitions.rs     transitions.toml parsing
    events.rs          events.toml parsing
    renders.rs         renders.toml parsing
  render/
    engine.rs          Tera template rendering
  hooks/
    generate.rs        Hook script generation
```

## Building from Source

Requires Rust 1.70+ (2021 edition).

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run tests (153 tests)
cargo test

# Binary location
./target/release/sahjhan
```

## License

MIT. See [LICENSE](LICENSE).
