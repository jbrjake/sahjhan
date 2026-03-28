# Codebase Navigation Documentation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create a progressive-disclosure navigation system (CLAUDE.md + per-file index headers) that lets humans and LLM agents jump to salient code with minimal token overhead.

**Architecture:** Three layers: (1) CLAUDE.md at repo root with module lookup tables and cross-cutting flow maps using anchor slugs, (2) `// ## Index` headers in every non-trivial source file mapping slugs to functions, (3) a strong maintenance rule ensuring docs stay current. All references use `// [slug]` anchors, never line numbers.

**Tech Stack:** Markdown, Rust comments

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `CLAUDE.md` | Create | Top-level navigation, lookup tables, flow maps, maintenance rules |
| `src/config/mod.rs` | Modify | Add index header |
| `src/config/events.rs` | Modify | Add index header |
| `src/config/protocol.rs` | Modify | Add index header |
| `src/config/states.rs` | Modify | Add index header |
| `src/config/transitions.rs` | Modify | Add index header |
| `src/config/renders.rs` | Modify | Add index header |
| `src/gates/evaluator.rs` | Modify | Add index header |
| `src/gates/template.rs` | Modify | Add index header |
| `src/state/machine.rs` | Modify | Add index header |
| `src/state/sets.rs` | Modify | Add index header |
| `src/ledger/chain.rs` | Modify | Add index header |
| `src/ledger/entry.rs` | Modify | Add index header |
| `src/ledger/import.rs` | Modify | Add index header |
| `src/ledger/registry.rs` | Modify | Add index header |
| `src/manifest/tracker.rs` | Modify | Add index header |
| `src/manifest/verify.rs` | Modify | Add index header |
| `src/query/mod.rs` | Modify | Add index header |
| `src/render/engine.rs` | Modify | Add index header |
| `src/hooks/generate.rs` | Modify | Add index header |
| `src/cli/aliases.rs` | Modify | Add index header |
| `src/main.rs` | Modify | Add index header |

---

### Task 1: Create CLAUDE.md

**Files:**
- Create: `CLAUDE.md`

- [ ] **Step 1: Write CLAUDE.md**

Create `CLAUDE.md` at the repo root with the following content:

````markdown
# Sahjhan — Codebase Navigation

## DOCUMENTATION MAINTENANCE RULE

**This is a BLOCKING requirement. It is not optional. It is not "nice to have."**

When you modify any source file in this repository, you MUST update documentation before committing:

1. **If you add, rename, or remove a public function/struct/enum:** Update that file's `// ## Index` header AND the corresponding lookup table in this file.
2. **If you add a new source file:** Add a `// ## Index` header to it AND add it to the relevant module table below.
3. **If you change a cross-cutting flow** (e.g., how template variables propagate, how gates are evaluated): Update the Flow Maps section below.
4. **If you add a new gate type:** Add it to the Gates table, the `known_gates` map in `validate_deep`, AND the Gate Types section below.

**Why:** An agent that reads stale docs will make wrong assumptions, write wrong code, and waste time. Every minute spent updating docs saves ten minutes of context-building for the next reader. Stale docs are worse than no docs — they actively mislead.

**How to verify:** Before committing, grep for any anchor slug you added (`// [your-slug]`) and confirm it appears in this file's lookup tables.

---

## Quick Reference

```
cargo build                    # Build
cargo test                     # Run all tests (233 tests)
cargo test <test_name>         # Run one test
cargo clippy -- -D warnings    # Lint
cargo fmt                      # Format
```

**Config dir:** Protocol TOML files (protocol.toml, states.toml, transitions.toml, events.toml, renders.toml)
**Data dir:** Runtime state (ledger.jsonl, manifest.json, ledgers.toml registry)
**Example config:** `examples/minimal/`

---

## Architecture (10-second version)

Sahjhan is a protocol enforcement engine. It has:

1. **Config** — TOML files define states, transitions, gates, events, sets, renders
2. **Ledger** — Append-only, hash-chained JSONL event log (source of truth)
3. **State Machine** — Derives current state from ledger; transitions require gates to pass
4. **Gates** — Conditions checked before transitions (file exists, command succeeds, SQL query, etc.)
5. **Template Variables** — `{{var}}` placeholders in gate commands resolved from state params + config
6. **Rendering** — Tera templates generate read-only markdown views from ledger state
7. **CLI** — clap-based; parses args, resolves aliases, delegates to command modules

---

## Module Lookup Tables

**How to use:** Find your concept in the table → note the file → grep for the anchor slug → read just that function.

### config/ — Protocol Configuration (TOML deserialization + validation)

| Concept | File | Anchor/Item | Purpose |
|---------|------|-------------|---------|
| Unified config | `config/mod.rs` | `ProtocolConfig` | Loads all TOML, holds full config |
| Config validation | `config/mod.rs` | `[validate]` | Basic structural validation |
| Deep validation | `config/mod.rs` | `[validate-deep]` | File existence, gate types, aliases |
| Protocol metadata | `config/protocol.rs` | `ProtocolMeta`, `PathsConfig`, `SetConfig` | protocol.toml structures |
| State definitions | `config/states.rs` | `StateConfig`, `StateParam` | states.toml; `StateParam.source` controls set derivation |
| Transition defs | `config/transitions.rs` | `TransitionConfig`, `GateConfig` | transitions.toml; gates are `#[serde(flatten)]` |
| Event definitions | `config/events.rs` | `EventConfig`, `EventFieldConfig` | events.toml; field patterns for validation |
| Render definitions | `config/renders.rs` | `RenderConfig` | renders.toml; trigger/template/target |

### gates/ — Gate Evaluation

| Concept | File | Anchor | Purpose |
|---------|------|--------|---------|
| Gate dispatch | `gates/types.rs` | `[eval]` | Routes gate_type string to evaluator |
| Template var map | `gates/types.rs` | `[build-template-vars]` | Builds `{{var}}` map from state_params + config |
| Field validation | `gates/types.rs` | `[validate-template-fields]` | Validates var values against event field patterns |
| Entry filter | `gates/types.rs` | `[entry-matches-filter]` | Checks ledger entry against k/v filter |
| Gate context | `gates/evaluator.rs` | `GateContext` | All inputs needed to evaluate a gate |
| Gate result | `gates/evaluator.rs` | `GateResult` | Outcome: passed, gate_type, description, reason |
| evaluate_gate | `gates/evaluator.rs` | `[evaluate-gate]` | Evaluate single gate |
| evaluate_gates | `gates/evaluator.rs` | `[evaluate-gates]` | Evaluate all gates, returns all results |
| Shell command gate | `gates/command.rs` | `[eval-command-succeeds]` | Run command, pass if exit 0 |
| Command output gate | `gates/command.rs` | `[eval-command-output]` | Run command, pass if stdout matches |
| Shell timeout | `gates/command.rs` | `[run-shell-with-timeout]` | try_wait polling loop |
| File exists gate | `gates/file.rs` | `[eval-file-exists]` | Single file check |
| Files exist gate | `gates/file.rs` | `[eval-files-exist]` | Multiple files check |
| Ledger event gate | `gates/ledger.rs` | `[eval-ledger-has-event]` | N+ events of type |
| Event since gate | `gates/ledger.rs` | `[eval-ledger-has-event-since]` | Event since last transition |
| Set covered gate | `gates/ledger.rs` | `[eval-set-covered]` | All set members in ledger |
| Min elapsed gate | `gates/ledger.rs` | `[eval-min-elapsed]` | Time since last event |
| No violations gate | `gates/ledger.rs` | `[eval-no-violations]` | No unresolved violations |
| Field not empty | `gates/ledger.rs` | `[eval-field-not-empty]` | Named event field non-empty |
| SQL query gate | `gates/query.rs` | `[eval-query-gate]` | DataFusion SQL, pass if result matches |
| Snapshot compare | `gates/snapshot.rs` | `[eval-snapshot-compare]` | Run command, extract JSON, compare |
| Snapshot reference | `gates/snapshot.rs` | `[resolve-snapshot-reference]` | Look up snapshot:key in ledger |
| Template resolution | `gates/template.rs` | `[resolve-template]` | `{{var}}` → shell-escaped value |
| Plain resolution | `gates/template.rs` | `[resolve-template-plain]` | `{{var}}` → raw value (for SQL) |
| Shell escaping | `gates/template.rs` | `[shell-escape]` | POSIX single-quote escaping |

### state/ — State Machine

| Concept | File | Anchor/Item | Purpose |
|---------|------|-------------|---------|
| State machine | `state/machine.rs` | `StateMachine` | Owns config + ledger, executes transitions |
| Transition | `state/machine.rs` | `[transition]` | Execute named command: build params → check gates → append event |
| Build state params | `state/machine.rs` | `[build-state-params]` | Derive params from state config + set state (`source` field) |
| Record event | `state/machine.rs` | `[record-event]` | Append event to ledger |
| Set status | `state/machine.rs` | `[set-status]` | Completion status of a named set |
| Derive state | `state/machine.rs` | `[derive-state]` | Find current state from last state_transition in ledger |
| Completed members | `state/machine.rs` | `[completed-members]` | Scan ledger for set_member_complete events |
| Set types | `state/sets.rs` | `MemberStatus`, `SetStatus` | Completion tracking structs |
| State errors | `state/machine.rs` | `StateError` | NoTransition, GateBlocked, Ledger, etc. |

### ledger/ — Append-Only Hash-Chained Event Log

| Concept | File | Anchor/Item | Purpose |
|---------|------|-------------|---------|
| Ledger struct | `ledger/chain.rs` | `Ledger` | Open, append, reload, verify, tail |
| Ledger init | `ledger/chain.rs` | `[ledger-init]` | Create new ledger with genesis entry |
| Ledger open | `ledger/chain.rs` | `[ledger-open]` | Open existing ledger file |
| Ledger append | `ledger/chain.rs` | `[ledger-append]` | Append hash-chained entry |
| Ledger reload | `ledger/chain.rs` | `[ledger-reload]` | Re-read from disk (stale chain fix) |
| Ledger verify | `ledger/chain.rs` | `[ledger-verify]` | Verify hash chain integrity |
| Entry struct | `ledger/entry.rs` | `LedgerEntry` | seq, ts, event_type, fields, hash, prev_hash |
| Entry errors | `ledger/entry.rs` | `LedgerError` | Io, Parse, Integrity, etc. |
| Import | `ledger/import.rs` | `[import-jsonl]` | Wrap bare JSONL in hash-chained ledger |
| Registry | `ledger/registry.rs` | `LedgerRegistry` | Multi-ledger name→path mapping |
| Ledger mode | `ledger/registry.rs` | `LedgerMode` | Full vs EventOnly |

### manifest/ — File Integrity Tracking

| Concept | File | Anchor/Item | Purpose |
|---------|------|-------------|---------|
| Manifest | `manifest/tracker.rs` | `Manifest` | SHA-256 tracked file registry |
| Track file | `manifest/tracker.rs` | `[manifest-track]` | Record file hash + metadata |
| Load/Save | `manifest/tracker.rs` | `[manifest-load]`, `[manifest-save]` | JSON persistence |
| Verify | `manifest/verify.rs` | `[verify]` | Compare files against manifest hashes |
| Mismatch | `manifest/verify.rs` | `Mismatch` | File that differs from manifest |

### render/ — Template Rendering

| Concept | File | Anchor/Item | Purpose |
|---------|------|-------------|---------|
| Render engine | `render/engine.rs` | `RenderEngine` | Tera-based markdown generation |
| Build context | `render/engine.rs` | `[build-context]` | Build template vars from ledger + config |
| Render triggered | `render/engine.rs` | `[render-triggered]` | Render on_transition / on_event |
| Dump context | `render/engine.rs` | `[dump-context]` | Export render context as JSON |

### query/ — SQL Query Engine

| Concept | File | Anchor/Item | Purpose |
|---------|------|-------------|---------|
| Query engine | `query/mod.rs` | `QueryEngine` | DataFusion over JSONL ledger files |
| Query file | `query/mod.rs` | `[query-file]` | Run SQL against single ledger |
| Query glob | `query/mod.rs` | `[query-glob]` | Run SQL against multiple ledgers |

### hooks/ — Claude Code Integration

| Concept | File | Anchor/Item | Purpose |
|---------|------|-------------|---------|
| Hook generator | `hooks/generate.rs` | `HookGenerator` | Produces Python hook scripts |
| Generated hook | `hooks/generate.rs` | `GeneratedHook` | Hook type + content |

### cli/ — Command Implementations

| Concept | File | Anchor | Purpose |
|---------|------|--------|---------|
| CLI entry point | `main.rs` | `[cli-main]` | clap arg parsing, alias resolution, dispatch |
| Alias resolution | `cli/aliases.rs` | `[resolve-alias]` | Rewrite CLI args via protocol aliases |
| Shared helpers | `cli/commands.rs` | (see file index) | Exit codes, ledger targeting, config loading |
| Init/validate/reset | `cli/init.rs` | `[cmd-init]`, `[cmd-validate]`, `[cmd-reset]` | Lifecycle commands |
| Transition/gate/event | `cli/transition.rs` | `[cmd-transition]`, `[cmd-gate-check]`, `[cmd-event]` | State machine commands |
| Status/sets | `cli/status.rs` | `[cmd-status]`, `[cmd-set-status]`, `[cmd-set-complete]` | Status display + set management |
| Log inspection | `cli/log.rs` | `[cmd-log-dump]`, `[cmd-log-verify]`, `[cmd-log-tail]` | Ledger viewing |
| Ledger management | `cli/ledger.rs` | `[cmd-ledger-create]`, `[cmd-ledger-list]`, etc. | Multi-ledger CRUD |
| Query | `cli/query.rs` | `[cmd-query]` | SQL queries over events |
| Render | `cli/render.rs` | `[cmd-render]`, `[cmd-render-dump-context]` | Template rendering |
| Manifest | `cli/manifest_cmd.rs` | `[cmd-manifest-verify]`, `[cmd-manifest-list]` | File integrity |
| Hooks | `cli/hooks_cmd.rs` | `[cmd-hook-generate]` | Hook script generation |

---

## Flow Maps

These trace how data moves through the system. When debugging or modifying a flow, read these files in order.

### Flow: Transition Lifecycle

How `sahjhan transition <command>` executes:

```
main.rs [cli-main]
  → cli/transition.rs [cmd-transition]
    → cli/commands.rs [load-config], [open-targeted-ledger]
    → state/machine.rs [transition]
      → state/machine.rs [build-state-params]    ← resolves StateParam.source ("current", "last_completed", "values")
      → CLI args merged as key=value overrides
      → for each gate:
        → state/machine.rs [evaluate-gate]       ← builds GateContext with state_params
          → gates/evaluator.rs [evaluate-gate]
            → gates/types.rs [eval]              ← dispatches by gate_type string
              → gates/command.rs [eval-command-succeeds]  (or other gate module)
                → gates/types.rs [build-template-vars]    ← clones state_params + injects config paths/sets
                → gates/types.rs [validate-template-fields]
                → gates/template.rs [resolve-template]    ← {{var}} → shell-escaped value
                → gates/command.rs [run-shell-with-timeout]
      → ledger/chain.rs [ledger-reload]          ← re-read after gate commands may have appended
      → ledger/chain.rs [ledger-append]           ← state_transition event
    → render/engine.rs [render-triggered]         ← on_transition renders
```

### Flow: Template Variable Resolution

How `{{current_perspective}}` gets its value:

```
1. State config (states.toml) declares:
   params = [{ name = "current_perspective", set = "perspectives", source = "current" }]

2. state/machine.rs [build-state-params]
   → reads StateParam.source
   → "current": scans ledger via [completed-members], finds first incomplete set member
   → "last_completed": scans ledger, takes last completed member
   → "values" (default): comma-joins all set values

3. CLI args (key=value) override any state_param

4. gates/types.rs [build-template-vars]
   → clones state_params
   → injects paths.data_dir, paths.render_dir, paths.managed
   → injects sets.<name> as comma-joined values

5. gates/template.rs [resolve-template] or [resolve-template-plain]
   → replaces {{key}} with value (shell-escaped or plain)
```

### Flow: Gate Evaluation Dispatch

```
gates/types.rs [eval] matches gate_type:
  "file_exists"         → gates/file.rs    [eval-file-exists]
  "files_exist"         → gates/file.rs    [eval-files-exist]
  "command_succeeds"    → gates/command.rs  [eval-command-succeeds]
  "command_output"      → gates/command.rs  [eval-command-output]
  "ledger_has_event"    → gates/ledger.rs   [eval-ledger-has-event]
  "ledger_has_event_since" → gates/ledger.rs [eval-ledger-has-event-since]
  "set_covered"         → gates/ledger.rs   [eval-set-covered]
  "min_elapsed"         → gates/ledger.rs   [eval-min-elapsed]
  "no_violations"       → gates/ledger.rs   [eval-no-violations]
  "field_not_empty"     → gates/ledger.rs   [eval-field-not-empty]
  "snapshot_compare"    → gates/snapshot.rs  [eval-snapshot-compare]
  "query"               → gates/query.rs    [eval-query-gate]
```

### Flow: Set Completion

```
main.rs [cli-main] → Commands::Set → SetAction::Complete
  → cli/status.rs [cmd-set-complete]
    → validates set exists, member exists
    → state/machine.rs [record-event] type="set_member_complete" fields={set, member}
    → render/engine.rs [render-triggered] trigger="on_event" event="set_member_complete"
```

### Flow: Config Loading

```
cli/commands.rs [load-config]
  → config/mod.rs ProtocolConfig::load(dir)
    → reads protocol.toml → config/protocol.rs ProtocolFile
    → reads states.toml   → config/states.rs StatesFile
    → reads transitions.toml → config/transitions.rs TransitionsFile
    → reads events.toml (optional) → config/events.rs EventsFile
    → reads renders.toml (optional) → config/renders.rs RendersFile
  → config/mod.rs [validate] — structural checks
  → config/mod.rs [validate-deep] (via cmd_validate) — file/alias/gate checks
```

---

## Test Files

| Test file | Tests |
|-----------|-------|
| `tests/gate_tests.rs` | All gate types, template interpolation, field validation, StateParam source |
| `tests/integration_tests.rs` | Full CLI end-to-end (init, transition, events, queries, renders, sets) |
| `tests/chain_integrity_tests.rs` | Ledger hash chain, append, reload, tamper detection |
| `tests/config_tests.rs` | Config loading and validation |
| `tests/state_machine_tests.rs` | StateMachine transitions, gates, sets |
| `tests/query_tests.rs` | DataFusion SQL queries over ledger |
| `tests/ledger_tests.rs` | LedgerEntry serialization, hashing, schema |
| `tests/manifest_tests.rs` | Manifest tracking, verification, restore |
| `tests/registry_tests.rs` | Multi-ledger registry CRUD |
| `tests/checkpoint_tests.rs` | Ledger checkpointing |
| `tests/import_tests.rs` | JSONL import |
| `tests/hook_generation_tests.rs` | Hook script generation |
| `tests/template_security_tests.rs` | Shell escaping, injection prevention |
````

- [ ] **Step 2: Verify the file was created correctly**

Run: `head -5 CLAUDE.md`
Expected: `# Sahjhan — Codebase Navigation`

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: create CLAUDE.md codebase navigation with lookup tables and flow maps"
```

---

### Task 2: Add index headers to config/ files

**Files:**
- Modify: `src/config/mod.rs`
- Modify: `src/config/events.rs`
- Modify: `src/config/protocol.rs`
- Modify: `src/config/states.rs`
- Modify: `src/config/transitions.rs`
- Modify: `src/config/renders.rs`

- [ ] **Step 1: Add index header to `src/config/mod.rs`**

Insert at the top of the file, before the existing `// src/config/mod.rs` comment:

```rust
// src/config/mod.rs
//
// Unified protocol configuration and validation.
//
// ## Index
// - ProtocolConfig          — unified config loaded from protocol directory
// - [validate]              ProtocolConfig::validate()       — basic structural validation
// - [validate-deep]         ProtocolConfig::validate_deep()  — file/alias/gate/render checks
// - initial_state()         — find the state with initial = true
```

Also add anchor slugs `// [validate]` before the `pub fn validate` method and `// [validate-deep]` before `pub fn validate_deep`. These methods currently lack anchors.

- [ ] **Step 2: Add index header to `src/config/events.rs`**

Insert at line 1:

```rust
// src/config/events.rs
//
// Deserialization structs for events.toml.
//
// ## Index
// - EventsFile              — top-level wrapper
// - EventConfig             — single event type definition
// - EventFieldConfig        — field name, type, pattern, allowed values
```

- [ ] **Step 3: Add index header to `src/config/protocol.rs`**

Insert at line 1:

```rust
// src/config/protocol.rs
//
// Deserialization structs for protocol.toml.
//
// ## Index
// - ProtocolFile            — top-level wrapper (protocol, paths, sets, aliases, checkpoints)
// - ProtocolMeta            — name, version, description
// - PathsConfig             — managed, data_dir, render_dir
// - SetConfig               — description + ordered values
// - CheckpointConfig        — checkpoint interval
```

- [ ] **Step 4: Add index header to `src/config/states.rs`**

The file already has a comment at line 1 (`use serde::Deserialize;`). Insert before it:

```rust
// src/config/states.rs
//
// Deserialization structs for states.toml.
//
// ## Index
// - StatesFile              — top-level wrapper
// - StateConfig             — label, initial, terminal, params, metadata
// - StateParam              — name, set, source (controls set-derived value resolution)
```

- [ ] **Step 5: Add index header to `src/config/transitions.rs`**

Insert at line 1:

```rust
// src/config/transitions.rs
//
// Deserialization structs for transitions.toml.
//
// ## Index
// - TransitionsFile         — top-level wrapper
// - TransitionConfig        — from, to, command, gates
// - GateConfig              — gate_type + flattened params
```

- [ ] **Step 6: Add index header to `src/config/renders.rs`**

Insert at line 1:

```rust
// src/config/renders.rs
//
// Deserialization structs for renders.toml.
//
// ## Index
// - RendersFile             — top-level wrapper
// - RenderConfig            — target, template, trigger, event_types
```

- [ ] **Step 7: Run tests**

Run: `cargo test 2>&1 | tail -3`
Expected: All pass.

- [ ] **Step 8: Commit**

```bash
git add src/config/
git commit -m "docs: add index headers to config/ source files"
```

---

### Task 3: Add index headers to gates/ files missing them

**Files:**
- Modify: `src/gates/evaluator.rs`
- Modify: `src/gates/template.rs`

- [ ] **Step 1: Add index header to `src/gates/evaluator.rs`**

Replace the existing comment block (lines 1-4) with:

```rust
// src/gates/evaluator.rs
//
// GateContext, GateResult, and the top-level evaluate_gate / evaluate_gates functions.
//
// ## Index
// - GateContext              — all inputs needed to evaluate a gate (ledger, config, state_params, etc.)
// - GateResult               — outcome: passed, gate_type, description, reason
// - [evaluate-gate]          evaluate_gate()   — evaluate a single gate
// - [evaluate-gates]         evaluate_gates()  — evaluate all gates, return all results
```

Also add anchor slugs `// [evaluate-gate]` before `pub fn evaluate_gate` and `// [evaluate-gates]` before `pub fn evaluate_gates`.

- [ ] **Step 2: Add index header to `src/gates/template.rs`**

Replace the existing comment block (lines 1-3) with:

```rust
// src/gates/template.rs
//
// Template variable resolution with configurable escaping strategy.
//
// ## Index
// - EscapeStrategy                    — Shell or None
// - [shell-escape]                    shell_escape()            — POSIX single-quote escaping
// - [resolve-template-with]           resolve_template_with()   — replace {{key}} with strategy
// - [resolve-template]                resolve_template()        — shell-escaped substitution
// - [resolve-template-plain]          resolve_template_plain()  — raw substitution (for SQL)
```

Also add anchor slugs before each public function: `// [shell-escape]`, `// [resolve-template-with]`, `// [resolve-template]`, `// [resolve-template-plain]`.

- [ ] **Step 3: Run tests**

Run: `cargo test 2>&1 | tail -3`
Expected: All pass.

- [ ] **Step 4: Commit**

```bash
git add src/gates/evaluator.rs src/gates/template.rs
git commit -m "docs: add index headers to gates/evaluator.rs and gates/template.rs"
```

---

### Task 4: Add index headers to state/ and ledger/ files

**Files:**
- Modify: `src/state/machine.rs`
- Modify: `src/state/sets.rs`
- Modify: `src/ledger/chain.rs`
- Modify: `src/ledger/entry.rs`
- Modify: `src/ledger/import.rs`
- Modify: `src/ledger/registry.rs`

- [ ] **Step 1: Add index header to `src/state/machine.rs`**

Replace the existing line 1 comment with:

```rust
// src/state/machine.rs
//
// Core state machine: derives state from ledger, executes transitions with gate checks.
//
// ## Index
// - StateError               — NoTransition, GateBlocked, Ledger, Serialization, UnknownSet
// - StateMachine             — owns config + ledger, executes transitions
// - [transition]             transition()              — execute named command (gates → append)
// - [record-event]           record_event()            — append event to ledger
// - [set-status]             set_status()              — completion status of a named set
// - [build-state-params]     build_state_params()      — derive params from state config + source field
// - [derive-state]           derive_state_from_ledger() — find current state from ledger
// - [evaluate-gate]          evaluate_gate()           — evaluate single gate with context
// - [completed-members]      completed_members_for_set() — scan ledger for completed set members
```

Add anchor slugs before each method that doesn't already have one: `// [transition]` before `pub fn transition`, `// [record-event]` before `pub fn record_event`, `// [set-status]` before `pub fn set_status`, `// [derive-state]` before `fn derive_state_from_ledger`, `// [evaluate-gate]` before `fn evaluate_gate` (the private one), `// [completed-members]` before `fn completed_members_for_set`.

Note: `[build-state-params]` anchor should already exist from the issue #8 work. If not, add it.

- [ ] **Step 2: Add index header to `src/state/sets.rs`**

Insert at line 1:

```rust
// src/state/sets.rs
//
// Completion set status types.
//
// ## Index
// - MemberStatus             — individual member: name + done flag
// - SetStatus                — aggregate: name, total, completed, members list
```

- [ ] **Step 3: Add index header to `src/ledger/chain.rs`**

Insert at line 1 (before the `use` statements):

```rust
// src/ledger/chain.rs
//
// Append-only, hash-chained ledger stored as JSONL.
//
// ## Index
// - Ledger                   — core ledger struct (open, append, reload, verify, tail)
// - [ledger-init]            Ledger::init()       — create new ledger with genesis entry
// - [ledger-open]            Ledger::open()       — open existing ledger file
// - [ledger-append]          Ledger::append()     — append hash-chained entry
// - [ledger-reload]          Ledger::reload()     — re-read from disk
// - [ledger-verify]          Ledger::verify()     — verify hash chain integrity
// - [parse-file-entries]     parse_file_entries()  — parse JSONL file into entries
```

Add anchor slugs before each method/function. Check which already exist and only add missing ones.

- [ ] **Step 4: Add index header to `src/ledger/entry.rs`**

Insert at line 1:

```rust
// src/ledger/entry.rs
//
// Ledger entry type and error definitions.
//
// ## Index
// - LedgerError              — Io, Parse, Integrity, SchemaVersion, etc.
// - LedgerEntry              — seq, ts, event_type, fields, hash, prev_hash
```

- [ ] **Step 5: Add index header to `src/ledger/import.rs`**

Replace the existing doc comment with:

```rust
// src/ledger/import.rs
//
// Import bare JSONL events into a hash-chained ledger.
//
// ## Index
// - [import-jsonl]           import_jsonl()  — wrap bare JSONL in hash-chained ledger
```

Add `// [import-jsonl]` anchor before `pub fn import_jsonl`.

- [ ] **Step 6: Add index header to `src/ledger/registry.rs`**

Replace the existing doc comment with:

```rust
// src/ledger/registry.rs
//
// Multi-ledger registry — maps human-friendly names to JSONL file paths.
//
// ## Index
// - LedgerMode               — Full or EventOnly
// - LedgerRegistryEntry       — name, path, mode
// - LedgerRegistry            — TOML-backed name→path registry
```

- [ ] **Step 7: Run tests**

Run: `cargo test 2>&1 | tail -3`
Expected: All pass.

- [ ] **Step 8: Commit**

```bash
git add src/state/ src/ledger/
git commit -m "docs: add index headers to state/ and ledger/ source files"
```

---

### Task 5: Add index headers to remaining files

**Files:**
- Modify: `src/manifest/tracker.rs`
- Modify: `src/manifest/verify.rs`
- Modify: `src/query/mod.rs`
- Modify: `src/render/engine.rs`
- Modify: `src/hooks/generate.rs`
- Modify: `src/cli/aliases.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Add index header to `src/manifest/tracker.rs`**

Insert at line 1 (before the `use` statements):

```rust
// src/manifest/tracker.rs
//
// SHA-256 file integrity tracking.
//
// ## Index
// - ManifestEntry            — path, hash, op, seq metadata per tracked file
// - Manifest                 — BTreeMap of tracked files with load/save/track/restore
// - RestoreAction            — Restored or NotNeeded
// - [manifest-load]          Manifest::load()       — load from JSON
// - [manifest-save]          Manifest::save()       — save to JSON
// - [manifest-track]         Manifest::track()      — record file hash
// - [compute-sha256]         compute_file_sha256()  — hash a file
```

Add anchor slugs before `load`, `save`, `track` methods and `compute_file_sha256`.

- [ ] **Step 2: Add index header to `src/manifest/verify.rs`**

Insert at line 1:

```rust
// src/manifest/verify.rs
//
// Manifest verification — compare files against recorded hashes.
//
// ## Index
// - Mismatch                 — file path + expected/actual hash
// - VerifyResult             — list of mismatches
// - [verify]                 verify()  — check all tracked files
```

Add `// [verify]` anchor before `pub fn verify`.

- [ ] **Step 3: Add index header to `src/query/mod.rs`**

Replace the existing doc comment with:

```rust
// src/query/mod.rs
//
// DataFusion-based SQL query engine over JSONL ledger files.
//
// ## Index
// - QueryEngine              — embeds DataFusion for SQL over ledger events
// - [query-file]             QueryEngine::query_file()  — SQL against single ledger
// - [query-glob]             QueryEngine::query_glob()  — SQL against multiple ledgers
// - [from-config]            QueryEngine::from_config()  — build engine from event definitions
```

Add anchor slugs before `query_file`, `query_glob`, and `from_config` methods.

- [ ] **Step 4: Add index header to `src/render/engine.rs`**

Replace the existing comment block with:

```rust
// src/render/engine.rs
//
// Tera template rendering engine — generates read-only markdown views from ledger state.
//
// ## Index
// - EventSummary             — event data for templates
// - MemberSummary            — set member status for templates
// - SetSummary               — set completion status for templates
// - RenderEngine             — Tera-based renderer with config + templates
// - [build-context]          build_context()        — build template vars from ledger + config
// - [render-triggered]       render_triggered()     — render on_transition / on_event
// - [dump-context]           dump_context()         — export render context as JSON
```

Add anchor slugs before `build_context`, `render_triggered`, and `dump_context` methods.

- [ ] **Step 5: Add index header to `src/hooks/generate.rs`**

Replace the existing comment block with:

```rust
// src/hooks/generate.rs
//
// Hook script generation for Claude Code integration.
//
// ## Index
// - GeneratedHook            — hook type + script content
// - HookGenerator            — produces Python hook scripts for write protection
```

- [ ] **Step 6: Add index header to `src/cli/aliases.rs`**

Replace the existing comment block with:

```rust
// src/cli/aliases.rs
//
// Alias resolution: rewrites CLI arguments when the first subcommand
// matches an alias defined in protocol.toml [aliases].
//
// ## Index
// - [resolve-alias]          resolve_alias()     — resolve alias from raw CLI args
// - [resolve-with-map]       resolve_with_map()  — resolve given already-parsed alias map
```

Add `// [resolve-alias]` and `// [resolve-with-map]` anchors before the functions.

- [ ] **Step 7: Add index header to `src/main.rs`**

Insert after the existing comment block (lines 1-4), expanding it:

```rust
// src/main.rs
//
// Sahjhan CLI entry point.
// Parses arguments with clap, resolves aliases, and delegates to command
// implementations in cli/ modules.
//
// ## Index
// - [cli-main]               main()  — CLI entry point, clap parsing, dispatch
// - Cli                      — top-level clap struct
// - Commands                 — subcommand enum
// - SetAction                — set subcommand enum
// - GateAction               — gate subcommand enum
// - LedgerAction             — ledger subcommand enum
```

Add `// [cli-main]` anchor before `fn main`.

- [ ] **Step 8: Run tests**

Run: `cargo test 2>&1 | tail -3`
Expected: All pass.

- [ ] **Step 9: Commit**

```bash
git add src/manifest/ src/query/ src/render/ src/hooks/ src/cli/aliases.rs src/main.rs
git commit -m "docs: add index headers to remaining source files"
```

---

### Task 6: Verify and final commit

- [ ] **Step 1: Run full test suite**

Run: `cargo test 2>&1 | tail -5`
Expected: All pass.

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings 2>&1`
Expected: Clean.

- [ ] **Step 3: Verify anchor coverage**

Run this to check that every anchor slug referenced in CLAUDE.md actually exists in the source:

```bash
grep -oP '\[[\w-]+\]' CLAUDE.md | sort -u | while read slug; do
  clean=$(echo "$slug" | tr -d '[]')
  if ! grep -rq "// \[$clean\]" src/; then
    echo "MISSING: $slug"
  fi
done
```

Expected: No output (all slugs found).

- [ ] **Step 4: Format**

Run: `cargo fmt`

- [ ] **Step 5: Commit if needed**

```bash
git add -A
git commit -m "docs: final verification pass for codebase navigation"
```
