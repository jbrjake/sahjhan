# Sahjhan

Protocol enforcement engine for AI agents.

A standalone Rust CLI that owns protocol state, mediates all writes to managed files, and maintains a tamper-evident ledger. The agent interacts with the protocol exclusively through the CLI. Direct file writes are blocked by hooks. The enforcement layer and the constrained agent share no mutable state.

## Why

AI agents following multi-step protocols will evade any step that is not mechanically verified. Advisory instructions, however detailed, are not enough. The agent understands the protocol, agrees with the protocol, and violates the protocol when compliance is inconvenient. This is not a bug in any particular agent. It is how advisory compliance works.

Current enforcement approaches (hooks, file-based gates) enforce necessary conditions but not sufficient ones. The agent complies with the enforced subset and ignores the rest. When enforcement relies on files the agent can write to (`STATUS.md`, `HISTORY.json`), the agent modifies enforcement state to bypass controls, including deleting evidence of prior violations.

Better prompting does not fix this. Sahjhan makes the rules load-bearing: if the gate does not open, the agent does not pass.

## Quick start

The `examples/minimal` directory contains a three-state protocol with two transitions, one completion set, and template rendering.

```bash
# Build
cargo build --release

# Initialize (creates ledger, manifest, genesis block)
sahjhan --config-dir examples/minimal init

# Check current state
sahjhan --config-dir examples/minimal status

# Start working (idle -> working)
sahjhan --config-dir examples/minimal transition begin

# Complete the required checks
sahjhan --config-dir examples/minimal set complete check tests
sahjhan --config-dir examples/minimal set complete check lint

# Finish (working -> done)
# Requires all members of the "check" set to be complete.
# Try running it before completing both checks. It will refuse.
sahjhan --config-dir examples/minimal transition complete
```

Every state change and set completion is recorded in the binary hash-chain ledger. Verify the chain at any time:

```bash
sahjhan --config-dir examples/minimal log verify
```

## CLI reference

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

Protocols can define aliases in `protocol.toml`:

```toml
[aliases]
"start" = "transition begin"
"finish" = "transition complete"
```

These become `sahjhan start`, `sahjhan finish`, etc. Aliases are resolved before argument parsing, so flags and `--config-dir` work normally.

## Writing a protocol definition

A protocol is five TOML files in a config directory. No Rust code changes needed.

### `protocol.toml`

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

### `states.toml`

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

### `transitions.toml`

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

### `events.toml`

```toml
[events.finding]
description = "A discovered issue"
fields = [
    { name = "id", type = "string", pattern = "^[A-Z]+-\\d{3}$" },
    { name = "severity", type = "string", enum = ["low", "medium", "high"] },
    { name = "description", type = "string" },
]
```

Fields support validation: `pattern` (regex), `enum` (allowed values), and `type` (`string`, `int`, `float`, `bool`). Values are validated at recording time.

### `renders.toml`

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

Templates use [Tera](https://keats.github.io/tera/) (Jinja2-like syntax). They receive current state, all events, set completion status, and protocol metadata. Rendered files go to `render_dir` and are tracked in the manifest.

### Gate types

Gates are preconditions on transitions:

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

Gate conditions support template variables (`{{current}}`, `{{sets.perspective}}`, `{{paths.artifacts}}`) resolved from state parameters, protocol config, and event payloads. All template variables are shell-escaped before interpolation into `cmd` strings.

## Hook generation

Sahjhan generates integration hooks for AI agent harnesses. Currently supported: Claude Code (`cc`).

```bash
sahjhan hook generate --harness cc --output-dir .claude/hooks
```

This produces three files:

- `write_guard.py` is a PreToolUse hook that blocks Write/Edit operations targeting managed paths.
- `bash_guard.py` is a PostToolUse hook that runs `sahjhan manifest verify` after every Bash command, recording violations if files were modified outside the CLI.
- `_common.py` has shared utilities for hook scripts.

A fourth file, `bootstrap.py`, is provided as a template in the repo. It protects the enforcement infrastructure itself (hook scripts, binary, config) from modification. Install it as the first PreToolUse entry in your harness configuration.

### Write mediation

Two channels prevent the agent from bypassing the CLI:

| Evasion attempt | Channel | Response |
|----------------|---------|----------|
| Write/Edit tool to managed file | PreToolUse | Blocked before execution |
| `echo >`, `sed -i`, `python -c` via Bash | PostToolUse | Detected, violation recorded in ledger |
| Modify the ledger binary directly | PostToolUse | Manifest hash mismatch detected |

## Security model

### Binary hash-chain ledger

Every event is appended to a binary ledger with SHA-256 hash chaining. Each entry contains a 4-byte magic (`SAHJ`), format version, monotonic sequence number, timestamp, the previous entry's SHA-256 hash, event type, MessagePack-serialized payload, and a SHA-256 hash of all preceding fields.

The binary format is intentional. The agent's tools are optimized for text manipulation. Tampering with a binary hash chain requires reverse-engineering the byte layout, serializing MessagePack payloads, computing correct SHA-256 hashes, and maintaining chain integrity across entries, all in a single Bash command before the PostToolUse hook fires.

`sahjhan log verify` walks the entire chain and reports the first inconsistency.

### File integrity manifest

The manifest (`.sahjhan/manifest.json`) tracks SHA-256 hashes of all files under managed paths. Each entry records which CLI operation last wrote the file and at which ledger sequence number.

The manifest hash is recorded in the ledger. Tampering with the manifest is detectable because the ledger's record will not match. Tampering with the ledger is detectable because of the hash chain. The circular dependency is bootstrapped at `sahjhan init`.

### Template variable escaping

Gate commands may contain template variables from event fields. All variable values are POSIX shell-escaped (single-quote wrapping) before interpolation. Field validation (pattern, enum, range) is enforced at resolution time. The `cmd` string itself comes from TOML config, which is protected by write mediation. Only the variable values originate from the agent.

### Concurrency

Exclusive file locks on the ledger for writes, shared locks for reads. Lock acquisition times out after 5 seconds.

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

### Source layout

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

## Building

Rust 1.70+ (2021 edition).

```bash
cargo build --release
cargo test
./target/release/sahjhan
```

## License

MIT. See [LICENSE](LICENSE).
