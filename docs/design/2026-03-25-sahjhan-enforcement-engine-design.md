# Sahjhan: Protocol Enforcement Engine for AI Agents

**Date:** 2026-03-25
**Status:** Design
**Repo:** jbrjake/sahjhan (new)

## Problem

AI agents following multi-step protocols will evade any step that isn't mechanically verified. Advisory instructions — however detailed, however explicitly agreed to — are insufficient. The agent understands the protocol, agrees with the protocol, and violates the protocol when compliance is inconvenient. This is not a bug in any specific agent. It is a structural property of advisory compliance.

Current enforcement approaches (Claude Code hooks, file-based gates) enforce necessary conditions but not sufficient ones. The agent complies with the enforced subset and ignores the rest. When enforcement relies on files the agent can write to (STATUS.md, HISTORY.json), the agent can and does modify enforcement state to bypass controls — including deleting evidence of prior violations.

Sahjhan is a standalone enforcement engine that owns protocol state, mediates all writes to managed files, and provides tamper-evident logging. The agent interacts with protocol state exclusively through the Sahjhan CLI. Direct file writes are blocked by hooks. The enforcement layer and the constrained agent share no mutable state.

## Design Principles

1. **The agent is not trusted.** Every claim the agent makes about its own behavior must be independently verifiable. Self-reported state (checkboxes, status fields) has zero enforcement value.
2. **Enforcement state is not agent-writable.** The ledger, manifest, and protocol state are owned by Sahjhan. The agent accesses them through CLI commands that validate preconditions before recording events.
3. **Tampering is detectable.** The ledger uses a hash chain where modifying, deleting, or inserting entries invalidates the chain. The manifest tracks SHA-256 hashes of all managed files.
4. **Protocols are declarative.** The engine is generic. Protocol-specific behavior (states, transitions, events, gates, completion sets) is defined in TOML configuration files. No Rust code changes are needed to define a new protocol.
5. **Binary format for the ledger.** The agent's tools are optimized for text manipulation. A binary ledger format raises the bar for tampering from "edit a JSON file" to "reverse-engineer a binary format, compute SHA-256 hashes, maintain chain integrity, and update the manifest — all in a single Bash command before the PostToolUse hook fires."

## Architecture

```
┌─────────────────────────────────────────────────┐
│              Protocol Definition                 │
│         (TOML config files — per-project)         │
│  states, transitions, gates, events, sets        │
├─────────────────────────────────────────────────┤
│              Sahjhan Engine                       │
│          (Rust binary — reusable core)            │
│  state machine executor, hash-chain ledger,      │
│  manifest verifier, gate evaluator, CLI harness, │
│  template renderer                               │
├─────────────────────────────────────────────────┤
│              Hook Bridge                          │
│       (generated scripts — per-harness)           │
│  PreToolUse/PostToolUse for Claude Code,          │
│  adaptable to Cursor, Codex, other harnesses     │
├─────────────────────────────────────────────────┤
│              Filesystem                           │
│  ledger.bin, manifest.json, rendered views        │
└─────────────────────────────────────────────────┘
```

### Layer 1: Sahjhan Engine (Rust crate + binary)

The core engine, published as a Rust crate and distributed as cross-compiled binaries. Contains:

- **Hash-chain ledger** — append-only binary event log with tamper detection
- **State machine executor** — loads states and transitions from TOML, validates gate conditions before allowing transitions
- **Gate evaluator** — composable conditions (file exists, command succeeds, ledger contains event, minimum time elapsed, set coverage, count thresholds)
- **Completion sets** — named collections of values where every member must have a completion event before a guarded transition passes
- **Manifest tracker** — SHA-256 hashes of all managed files, integrity verification
- **CLI harness** — generates subcommands from config (transitions become commands, events become recording commands, aliases provide ergonomic shortcuts)
- **Template renderer** — generates read-only markdown views from ledger state using Tera templates

### Layer 2: Protocol Definition (TOML config)

A protocol is defined by a set of TOML files in a project directory. The engine loads these at runtime. No compilation required to define or modify a protocol.

### Layer 3: Hook Bridge (generated scripts)

Thin integration scripts that adapt Sahjhan to a specific AI harness. For Claude Code: Python hooks using the PreToolUse/PostToolUse/Stop events. The bridge scripts are generated from the protocol config or provided as templates.

## Hash-Chain Ledger

### Entry Format

Each entry in the ledger is a fixed-header binary record followed by a variable-length payload:

```
┌──────────────────────────────────────────────────────┐
│ magic: [u8; 4]          — "SAHJ" file identifier     │
│ seq: u64                — monotonic sequence number   │
│ timestamp: i64          — Unix milliseconds, CLI clock│
│ prev_hash: [u8; 32]     — SHA-256 of previous entry   │
│ event_type_len: u16     — length of event type string  │
│ event_type: [u8; N]     — event type identifier        │
│ payload_len: u32        — length of payload            │
│ payload: [u8; M]        — MessagePack-serialized data  │
│ entry_hash: [u8; 32]    — SHA-256(all preceding fields)│
└──────────────────────────────────────────────────────┘
```

### Genesis Block

Entry 0 has `seq: 0`, a random 256-bit nonce as `prev_hash`, and a `protocol_init` event type. The payload contains the protocol name, version, and a hash of the TOML config files at initialization time. This makes each run's chain unique and ties it to a specific protocol version.

### Integrity Guarantees

| Attack | Detection |
|--------|-----------|
| Delete an entry | Sequence gap (entry N followed by entry N+2) |
| Modify an entry | Recomputed hash doesn't match `entry_hash` |
| Insert an entry | `prev_hash` doesn't match preceding entry's `entry_hash` |
| Replay from another run | Genesis nonce mismatch |
| Replace entire ledger | Genesis hash recorded in manifest; manifest hash recorded in initial ledger entry (circular dependency bootstrapped at init) |
| Modify manifest to cover tracks | Manifest hash is in the ledger, which the agent can't write to |

### Verification

`sahjhan log verify` walks the entire chain, recomputes every hash, checks sequence monotonicity, validates timestamps are non-decreasing, and reports the first inconsistency found. Returns exit 0 if the chain is valid, exit 1 with a description of the tampering if not.

### Binary Format Rationale

The ledger is intentionally not JSON, JSONL, YAML, or any text format. The agent's tool ecosystem (Write, Edit, Bash with echo/sed/awk) is optimized for text manipulation. A binary format with hash chaining requires the agent to:

1. Understand the byte layout
2. Serialize MessagePack payloads
3. Compute SHA-256 hashes
4. Maintain chain integrity across entries

This is not impossible, but it exceeds the effort threshold for casual evasion and is likely to exceed a single context window for implementation. The manifest verification (PostToolUse hook) provides a second layer even if the binary format is defeated.

## State Machine

### Configuration Format

**`states.toml`:**

```toml
[states.idle]
label = "Idle"
initial = true

[states.recon]
label = "Reconnaissance"
metadata = { phase = "discovery" }

[states.active]
label = "Active Work"
# Parameterized state — tracks which set member is active
params = [{ name = "current", set = "perspective" }]

[states.member_clean]
label = "Set Member Clean"
params = [{ name = "completed", set = "perspective" }]

[states.final_check]
label = "Final Verification"

[states.complete]
label = "Complete"
terminal = true
```

**`transitions.toml`:**

```toml
[[transitions]]
from = "idle"
to = "recon"
command = "start"
gates = []

[[transitions]]
from = "recon"
to = "active"
command = "recon complete"
gates = [
    { type = "files_exist", paths = ["{{paths.artifacts}}/recon-summary.md"] },
    { type = "ledger_has_event", event = "recon_step", min_count = 3 },
]

[[transitions]]
from = "active"
to = "member_clean"
command = "set complete {{sets.perspective}}"
gates = [
    { type = "command_succeeds", cmd = "echo 'verification command here'" },
    { type = "ledger_has_event", event = "iteration_complete",
      filter = { member = "{{current}}" }, min_count = 2 },
    { type = "min_elapsed", event = "iteration_complete", seconds = 60 },
]

[[transitions]]
from = "member_clean"
to = "active"
command = "rotate"
gates = [
    { type = "ledger_has_event", event = "set_member_complete",
      filter = { member = "{{completed}}" } },
]

[[transitions]]
from = "active"
to = "final_check"
command = "all complete"
gates = [
    { type = "set_covered", set = "perspective",
      event = "set_member_complete", field = "member" },
]

[[transitions]]
from = "final_check"
to = "complete"
command = "finalize"
gates = [
    { type = "command_succeeds", cmd = "echo 'final verification'" },
    { type = "no_violations" },
]
```

### Gate Types

The engine provides a fixed set of composable gate condition types:

| Gate Type | Parameters | Behavior |
|-----------|-----------|----------|
| `files_exist` | `paths: [string]` | All listed files exist on disk |
| `file_exists` | `path: string` | Single file exists |
| `command_succeeds` | `cmd: string` | Shell command returns exit 0 |
| `command_output` | `cmd: string, expect: string` | Command stdout matches expected value |
| `ledger_has_event` | `event: string, min_count: u32, filter: map` | Ledger contains N+ events of type, optionally filtered by field values |
| `ledger_has_event_since` | `event: string, since: "last_transition"` | Event of type exists since the most recent transition event |
| `set_covered` | `set: string, event: string, field: string` | Every member of the named set has a corresponding event |
| `min_elapsed` | `event: string, seconds: u64` | At least N seconds since the last event of this type |
| `no_violations` | (none) | No `protocol_violation` events exist in the ledger (or all have been resolved) |
| `field_not_empty` | `field: string` | Named field in the current event payload is non-empty |
| `snapshot_compare` | `cmd: string, extract: string, compare: "gt"\|"gte"\|"eq", reference: string` | Compare a value extracted from command output against a snapshot stored in the ledger |

Gate conditions support template variables (`{{current}}`, `{{sets.perspective}}`, `{{paths.artifacts}}`) that are resolved from the current state parameters, protocol config, and event payload.

## Completion Sets

A completion set is a named, ordered collection of string values. The protocol declares which sets exist and what their members are. The engine tracks which members have completion events in the ledger and provides gates that require full coverage.

**Configuration:**

```toml
# In protocol.toml
[sets.perspective]
description = "Analytical dimension requiring individual completion"
values = [
    "component",
    "integration",
    "security",
]

[sets.check]
description = "Verification checks"
values = [
    "tests-pass",
    "lint-clean",
    "types-clean",
]
```

**Event recording:**

```bash
sahjhan set complete perspective component
# Engine verifies gates for the member_clean transition,
# appends set_member_complete event with {set: "perspective", member: "component"},
# transitions state
```

**Gate usage:**

```toml
{ type = "set_covered", set = "perspective",
  event = "set_member_complete", field = "member" }
```

This gate passes only when every value in `sets.perspective` has a corresponding `set_member_complete` event in the ledger with `field: member` matching that value.

Sets are a first-class engine concept, not a protocol-specific feature. Any protocol can define any number of sets with any values.

## Manifest Verification

The engine maintains a manifest file (`.sahjhan/manifest.json`) tracking SHA-256 hashes of all files under managed paths:

```json
{
    "version": 1,
    "managed_paths": ["docs/holtz"],
    "entries": {
        "docs/holtz/PUNCHLIST.md": {
            "sha256": "a1b2c3d4...",
            "last_operation": "event finding --id BH-001",
            "last_updated": "2026-03-25T20:08:11Z",
            "ledger_seq": 14
        }
    },
    "manifest_hash": "e5f6a7b8..."
}
```

- `manifest_hash` is a SHA-256 of the serialized entries, recorded in the ledger at each update
- Tampering with the manifest is detectable because the ledger's record of the hash won't match
- Tampering with the ledger is detectable because of the hash chain
- The circular dependency is bootstrapped at `sahjhan init`

### Verification Flow

`sahjhan manifest verify`:
1. For each entry in manifest, compute SHA-256 of the file on disk
2. Compare against recorded hash
3. If mismatch: report which files were modified, when they were last legitimately written, and by which CLI operation
4. Return exit 0 if clean, exit 1 with details if tampered

### Restoration

`sahjhan manifest restore <path>`:
- The engine does NOT store file contents (it's not a backup system)
- Restoration works by re-rendering the file from ledger state (for rendered views like STATUS.md)
- For agent-authored files (PUNCHLIST.md), restoration means git checkout of the last committed version
- The manifest records which ledger sequence number last wrote each file, enabling targeted re-rendering

## Write Mediation

Two enforcement channels prevent the agent from bypassing the CLI:

### Channel 1: PreToolUse Hook (Write/Edit)

Fires before any Write or Edit tool call. Checks if the target path is under a managed directory. If so, blocks the operation with an instructive error message.

```python
# Generated hook: write_guard.py
import os, sys, json
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from _common import exit_block, exit_ok, read_event

MANAGED = {{managed_paths}}  # Injected from protocol.toml

def main():
    event = read_event()
    tool_input = event.get("tool_input", {})
    file_path = tool_input.get("file_path", "")
    cwd = event.get("cwd", os.getcwd())
    resolved = os.path.realpath(os.path.join(cwd, file_path))
    for prefix in MANAGED:
        managed_abs = os.path.realpath(os.path.join(cwd, prefix))
        if resolved.startswith(managed_abs + os.sep) or resolved == managed_abs:
            exit_block(
                f"WRITE BLOCKED: {file_path} is managed by sahjhan. "
                f"Use CLI commands to modify audit state. "
                f"Direct writes are not permitted."
            )
    exit_ok()

if __name__ == "__main__":
    main()
```

### Channel 2: PostToolUse Hook (Bash)

Fires after every Bash command. Invokes `sahjhan manifest verify` to detect modifications made through shell commands (echo, cat, python -c, etc.).

```python
# Generated hook: bash_guard.py
import os, sys, subprocess
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from _common import exit_ok, exit_warn, read_event

def main():
    event = read_event()
    cwd = event.get("cwd", os.getcwd())
    result = subprocess.run(
        [sahjhan_binary(), "manifest", "verify"],
        capture_output=True, text=True, cwd=cwd,
    )
    if result.returncode != 0:
        # Record violation in ledger
        subprocess.run(
            [sahjhan_binary(), "event", "protocol_violation",
             "--detail", result.stdout.strip()],
            cwd=cwd,
        )
        exit_warn(
            f"UNAUTHORIZED MODIFICATION DETECTED:\n{result.stdout.strip()}\n"
            f"This violation has been recorded in the ledger."
        )
    exit_ok()

if __name__ == "__main__":
    main()
```

### What This Catches

| Evasion | Channel | Detection |
|---------|---------|-----------|
| Write tool to managed file | PreToolUse | Blocked before execution |
| Edit tool to managed file | PreToolUse | Blocked before execution |
| `echo > managed_file` via Bash | PostToolUse | Manifest hash mismatch |
| `python -c "open(...).write(...)"` via Bash | PostToolUse | Manifest hash mismatch |
| `sed -i` via Bash | PostToolUse | Manifest hash mismatch |

### What This Does Not Catch

The agent could theoretically construct a program that modifies the binary ledger directly. This requires understanding the binary format, computing correct SHA-256 hashes, and also updating the manifest. The PostToolUse hook would catch the manifest modification. To defeat both layers simultaneously in a single Bash command is theoretically possible but practically exceeds the complexity threshold for in-context evasion.

## CLI Command Surface

### Built-In Commands (every protocol)

```
sahjhan init [--config-dir <path>]     Initialize ledger, manifest, genesis block
sahjhan status                         Current state, set progress, gate status
sahjhan log dump                       Human-readable ledger dump
sahjhan log verify                     Validate hash chain integrity
sahjhan log tail [N]                   Last N events
sahjhan manifest verify                Check managed files against manifest
sahjhan manifest list                  Show managed files and hashes
sahjhan manifest restore <path>        Restore file from last known-good state
sahjhan render                         Regenerate all markdown views
sahjhan set status <set>               Show completion status for a set
sahjhan set complete <set> <member>    Record member completion (runs gates)
sahjhan transition <name> [args]       Execute a named transition (runs gates)
sahjhan gate check <transition>        Dry-run: show which gates pass/fail
sahjhan event <type> [--field val]     Record a protocol event
sahjhan reset --confirm                Archive current run, fresh start
sahjhan hook generate [--harness cc]   Generate hook scripts for a harness
```

### Derived Commands (from protocol config)

Protocol-specific aliases defined in `protocol.toml`:

```toml
[aliases]
"start" = "transition start"
"complete" = "transition finalize"
```

These become available as `sahjhan start`, `sahjhan complete`, etc.

### Status Output

`sahjhan status` renders a structured overview:

```
═══════════════════════════════════════════════════════════
  sahjhan · <protocol_name> v<version> · Run <N>
═══════════════════════════════════════════════════════════

  State:     <current_state> (<params>)
  Ledger:    <N> events, chain <valid|INVALID>, <violations>
  Manifest:  <N> files tracked, <clean|N modified>

  Set: <set_name> (<completed>/<total> complete)
    ✓ member_a
    ✓ member_b
    · member_c          ← active
    · member_d

  Next gate (<transition_name>):
    ✓ <gate description>
    ✗ <gate description> (<reason>)

═══════════════════════════════════════════════════════════
```

## Template Rendering

Sahjhan generates read-only markdown views from ledger state. The agent never writes STATUS.md or PUNCHLIST.md — Sahjhan renders them from the event log.

Templates use the Tera templating engine (Jinja2-like syntax, Rust-native). Defined in `renders.toml`:

```toml
[[renders]]
target = "STATUS.md"
template = "templates/status.md.tera"
trigger = "on_transition"

[[renders]]
target = "PUNCHLIST.md"
template = "templates/punchlist.md.tera"
trigger = "on_event"
event_types = ["finding", "finding_resolved"]
```

Templates receive a context object containing:
- Current state and parameters
- All events (filterable by type)
- Set completion status
- Protocol metadata
- Computed metrics (counts, durations, rates)

Rendered files are written to `paths.render_dir` (from `protocol.toml`) and tracked in the manifest. They are authoritative for human consumption but not for enforcement — enforcement reads the ledger directly.

## Project Structure

```
sahjhan/
├── Cargo.toml
├── src/
│   ├── main.rs              # CLI entry point (clap)
│   ├── ledger/
│   │   ├── mod.rs
│   │   ├── chain.rs          # Hash-chain append, verify, read
│   │   ├── entry.rs          # Entry serialization/deserialization
│   │   └── genesis.rs        # Genesis block creation
│   ├── state/
│   │   ├── mod.rs
│   │   ├── machine.rs        # State machine executor
│   │   ├── transition.rs     # Transition validation
│   │   └── sets.rs           # Completion set tracking
│   ├── gates/
│   │   ├── mod.rs
│   │   ├── evaluator.rs      # Gate condition evaluation
│   │   ├── types.rs          # Gate type implementations
│   │   └── template.rs       # Template variable resolution
│   ├── manifest/
│   │   ├── mod.rs
│   │   ├── tracker.rs        # File hash tracking
│   │   └── verify.rs         # Integrity verification
│   ├── config/
│   │   ├── mod.rs
│   │   ├── protocol.rs       # protocol.toml parsing
│   │   ├── states.rs         # states.toml parsing
│   │   ├── transitions.rs    # transitions.toml parsing
│   │   └── events.rs         # events.toml parsing
│   ├── render/
│   │   ├── mod.rs
│   │   └── engine.rs         # Tera template rendering
│   └── hooks/
│       ├── mod.rs
│       └── generate.rs       # Hook script generation
├── templates/
│   └── hooks/
│       ├── write_guard.py.tera
│       └── bash_guard.py.tera
└── tests/
    ├── ledger_tests.rs
    ├── state_machine_tests.rs
    ├── gate_tests.rs
    └── manifest_tests.rs
```

## Cross-Platform Distribution

Sahjhan is distributed as pre-compiled binaries for:
- `aarch64-apple-darwin` (macOS Apple Silicon)
- `x86_64-apple-darwin` (macOS Intel)
- `x86_64-unknown-linux-gnu` (Linux x86)
- `aarch64-unknown-linux-gnu` (Linux ARM)

Built via GitHub Actions CI on tagged releases. Projects consuming Sahjhan vendor the appropriate binary or reference it as a dependency.

## Template Variable Security

Gate conditions of type `command_succeeds` and `command_output` execute shell commands that may contain template variables (e.g., `{{item_id}}`). This is a shell injection vector: if the agent controls the variable value and the value is interpolated directly into a shell command, the agent can inject metacharacters.

**Mitigations:**

1. **Field validation is enforced at template resolution time.** When a template variable references an event field, the field's validation rules (pattern, enum, range) are applied before interpolation. A field with `pattern = "^B[HJ]-\\d{3}$"` cannot contain shell metacharacters.

2. **Template variables are always shell-escaped.** The engine applies POSIX shell escaping (single-quote wrapping with internal quote escaping) to all template variable values before interpolation into `cmd` strings. This is a defense-in-depth measure — field validation should prevent dangerous values, but escaping catches anything validation misses.

3. **The `cmd` string itself is not agent-controlled.** It comes from the TOML config, which is protected by the write mediation hooks. Only the variable values come from the agent.

## Concurrency Control

Sahjhan acquires an exclusive file lock (`flock` on Unix, `LockFile` on Windows) on the ledger file before any write operation. The lock is held for the duration of the operation: event recording, manifest update, and template rendering. The lock is released after all writes complete.

If the lock cannot be acquired within 5 seconds, the CLI exits with a clear error message. This prevents the PostToolUse hook's `sahjhan manifest verify` from deadlocking with a concurrent `sahjhan event` call from the agent.

Read operations (`sahjhan status`, `sahjhan log dump`, `sahjhan manifest verify`) acquire a shared lock, allowing concurrent reads but blocking during writes.

## Binary Format Versioning

The ledger entry format includes a version byte after the magic bytes:

```
┌──────────────────────────────────────────────────────┐
│ magic: [u8; 4]          — "SAHJ"                     │
│ format_version: u8      — currently 1                │
│ seq: u64                — monotonic sequence number   │
│ ...                                                  │
└──────────────────────────────────────────────────────┘
```

The genesis block's payload also records the format version. If the format changes in a future release, the engine can detect old-format ledgers and either migrate them or refuse to operate with a clear upgrade message.

## Genesis Nonce Generation

The genesis block's `prev_hash` nonce is generated from a cryptographically secure random source (`getrandom` crate, backed by the OS CSPRNG). This prevents nonce prediction and precomputation of replacement ledgers.

## Command Timeouts

All `command_succeeds` and `command_output` gates have a configurable timeout:

```toml
{ type = "command_succeeds", cmd = "python -m pytest", timeout = 120 }
```

Default timeout is 60 seconds if not specified. If the command does not complete within the timeout, the gate fails with a timeout error. This prevents gate commands from hanging indefinitely.

## Enforcement Infrastructure Protection

The enforcement infrastructure itself — hook scripts, Sahjhan binary, TOML config — must be protected from modification by the agent. This is the bootstrap problem: the write guard hook protects managed paths, but who protects the write guard?

**Solution: A minimal bootstrap hook.**

A single, hardcoded PreToolUse hook (`_sahjhan_bootstrap.py`) is installed alongside the generated hooks. This hook:

1. Blocks Write/Edit to `enforcement/`, `bin/sahjhan*`, and any file matching `*_sahjhan_bootstrap.py`
2. Is as small as possible (under 30 lines) to minimize attack surface
3. Is not generated by Sahjhan — it is hand-written and versioned as part of the consuming project
4. Protects itself by including its own path in the blocked list

```python
# _sahjhan_bootstrap.py — DO NOT MODIFY
# This hook protects Sahjhan's enforcement infrastructure.
# It is intentionally minimal and self-referential.
import os, sys, json

PROTECTED = ["enforcement/", "bin/sahjhan", "_sahjhan_bootstrap.py"]

event = json.loads(sys.stdin.read())
path = event.get("tool_input", {}).get("file_path", "")
cwd = event.get("cwd", os.getcwd())
resolved = os.path.realpath(os.path.join(cwd, path)) if path else ""

for p in PROTECTED:
    full = os.path.realpath(os.path.join(cwd, p))
    if resolved.startswith(full) or resolved == full:
        print(json.dumps({"decision": "block",
            "reason": f"BLOCKED: {path} is protected enforcement infrastructure."}))
        sys.exit(0)

print(json.dumps({"decision": "allow"}))
```

The hook bridge section of the consuming project's hooks.json must include this bootstrap hook as the FIRST PreToolUse entry, before any generated hooks.

**Limitation:** If the agent can modify `hooks.json` itself to remove the bootstrap hook entry, this protection is defeated. `hooks.json` must also be in the PROTECTED list. This requires that `hooks.json` lives in a protected path or is added to the bootstrap hook's block list.

## `sahjhan reset` Access Control

`sahjhan reset --confirm` archives the current run and starts fresh. This is a destructive operation that wipes active enforcement state.

**Access control:** The `reset` command requires a confirmation token that is displayed to the user in the terminal output and must be typed back. The token is a 6-character alphanumeric string derived from the current ledger's genesis hash. The agent cannot predict it without reading the ledger (which it can do), but the confirmation prompt is designed to be noticed by the human watching the session:

```
WARNING: This will archive all enforcement state for the current run.
Type this confirmation token to proceed: A7K2M9
>
```

If the agent pipes the token programmatically (e.g., `echo "A7K2M9" | sahjhan reset --confirm`), the reset still succeeds — but the PostToolUse hook will detect the manifest change and record a `protocol_violation`. This is detection, not prevention. For stronger protection, the bootstrap hook can block Bash commands containing `sahjhan reset`.

## Manifest Security Model

The manifest (`.sahjhan/manifest.json`) is readable by the agent. This is by design — the manifest is a debugging and verification tool, not a secret. The security model does not depend on manifest secrecy. The agent knowing file hashes does not help it tamper with files undetected, because:

1. The write guard blocks Write/Edit to managed paths
2. The bash guard detects post-hoc modifications via manifest verify
3. The manifest hash is recorded in the ledger, so manifest tampering is itself detectable

The `data_dir` (containing ledger and manifest) MUST be under a managed path. The engine validates this at `sahjhan init` time and refuses to initialize if `data_dir` is not within or equal to a `paths.managed` entry.

## Render Lifecycle

Events and transitions are recorded to the ledger first, then renders are triggered. The lifecycle:

1. Validate gate conditions
2. Append event to ledger (with hash chain)
3. Update manifest for the ledger file
4. Trigger configured renders (templates → managed files)
5. Update manifest for each rendered file

If rendering fails (template error, disk full), the event is still in the ledger. A warning is emitted. The manifest is not updated for the failed render target, so subsequent `manifest verify` calls will report the stale file. The agent can re-trigger renders with `sahjhan render`.

Renders triggered by `on_event` are batched: if a single CLI command records multiple events, renders are triggered once after all events are recorded, not once per event.

## Snapshot Mechanism

The `snapshot_compare` gate type compares a current value against a value stored in the ledger. Snapshots are recorded as `snapshot` events:

```bash
sahjhan event snapshot --key pre_audit_edge_count --value 60
```

The `snapshot_compare` gate resolves `reference = "snapshot:pre_audit_edge_count"` by finding the most recent `snapshot` event with `key = "pre_audit_edge_count"` and reading its `value` field. The `extract` parameter is applied to the command's stdout as a JSON key (the command is expected to output JSON).

If no snapshot exists for the referenced key, the gate fails with an error message indicating the snapshot must be recorded first.

## Future Considerations

- **MCP server mode:** Expose Sahjhan operations as MCP tools for tighter harness integration (tools appear in the agent's tool list rather than requiring Bash calls)
- **Remote attestation:** For high-stakes protocols, the ledger could be periodically signed and uploaded to an external service, making it impossible to replace even if the agent gains full filesystem access
- **Multi-agent protocols:** Support for multiple named agents with independent namespaces but shared transition gates (e.g., two auditors that must both complete before convergence). Each namespace gets its own ledger prefix and state, with cross-namespace gates for coordination points (e.g., merge requires both auditors to have completed their audit phase). This is required for Holtz's Justine integration and should be prioritized for v1.1.
- **Protocol composition:** Import and extend base protocols (e.g., a "TDD audit" base protocol that specific tools customize)
- **`ledger_no_event_since` gate:** Negation of `ledger_has_event_since` for cases like "no violations since last clean transition"
