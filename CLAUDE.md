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
| Transition defs | `config/transitions.rs` | `TransitionConfig`, `GateConfig` | transitions.toml; `args` declares positional params; gates are `#[serde(flatten)]` |
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
    → cli/commands.rs [load-config], [open-targeted]
    → state/machine.rs [transition]
      → state/machine.rs [build-state-params]    ← resolves StateParam.source ("current", "last_completed", "values")
      → CLI args merged: positional mapped to transition.args names, key=value as overrides
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
