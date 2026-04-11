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
cargo test                     # Run all tests (419+ tests)
cargo test <test_name>         # Run one test
cargo clippy -- -D warnings    # Lint
cargo fmt                      # Format
cargo fmt --all -- --check     # CI format check (run before every commit)
```

**Config dir:** Protocol TOML files (protocol.toml, states.toml, transitions.toml, events.toml, renders.toml, hooks.toml)
**Data dir:** Runtime state (ledger.jsonl, manifest.json, ledgers.toml registry, active-ledger marker)
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
| Unified config | `config/mod.rs` | `ProtocolConfig` | Loads all TOML, holds full config (includes hooks, monitors) |
| Config validation | `config/mod.rs` | `[validate]` | Basic structural validation |
| Deep validation | `config/mod.rs` | `[validate-deep]` | File existence, gate types, aliases, ledger template, hooks, monitors, write_gated checks |
| Recursive gate validator | `config/mod.rs` | `[validate-gate]` | Validates composite (any_of, all_of, not, k_of_n) and leaf gates recursively |
| Protocol metadata | `config/protocol.rs` | `ProtocolMeta`, `PathsConfig`, `SetConfig` | protocol.toml structures |
| Ledger template | `config/protocol.rs` | `LedgerTemplateConfig` | `[ledgers]` section; path or path_template for template-based ledger creation |
| Guards config | `config/protocol.rs` | `GuardsConfig` | `[guards]` section; `write_gated` lists state-gated writable paths |
| Write-gated config | `config/protocol.rs` | `WriteGatedConfig` | A path whose writability is gated by protocol state (path, writable_in, message) |
| Hooks file | `config/hooks.rs` | `HooksFile` | Top-level hooks.toml wrapper (hooks + monitors) |
| Hook config | `config/hooks.rs` | `HookConfig` | Single hook rule (event, tools, states, gate, check, auto_record, filter) |
| Hook event | `config/hooks.rs` | `HookEvent` | PreToolUse, PostToolUse, Stop |
| Hook filter | `config/hooks.rs` | `HookFilter` | Path glob filters for tool arguments |
| Hook check | `config/hooks.rs` | `HookCheck` | Threshold/pattern check config (type, sql, compare, threshold, patterns) |
| Auto-record config | `config/hooks.rs` | `AutoRecordConfig` | Auto-record event config (event_type, fields) |
| Monitor config | `config/hooks.rs` | `MonitorConfig` | Monitor rule (name, states, action, message, trigger) |
| Monitor trigger | `config/hooks.rs` | `MonitorTrigger` | Monitor trigger condition (type, threshold) |
| State definitions | `config/states.rs` | `StateConfig`, `StateParam` | states.toml; `StateParam.source` controls set derivation |
| Transition defs | `config/transitions.rs` | `TransitionConfig`, `GateConfig` | transitions.toml; `args` declares positional params; `intent` is optional per-gate "why"; `gates` holds nested child gates for composite types (any_of, all_of, not, k_of_n); remaining fields are `#[serde(flatten)]` into params |
| Event definitions | `config/events.rs` | `EventConfig`, `EventFieldConfig` | events.toml; field patterns for validation; `restricted` marks HMAC-only events; `optional` marks non-required fields |
| Render definitions | `config/renders.rs` | `RenderConfig` | renders.toml; trigger/template/target/ledger/ledger_template |
| Config seal hashing | `config/mod.rs` | `compute_config_seals()` | SHA-256 hash all 6 TOML config files |

### gates/ — Gate Evaluation

| Concept | File | Anchor | Purpose |
|---------|------|--------|---------|
| Gate dispatch | `gates/types.rs` | `[eval]` | Routes gate_type string to evaluator |
| Template var map | `gates/types.rs` | `[build-template-vars]` | Builds `{{var}}` map from state_params + config |
| Field validation | `gates/types.rs` | `[validate-template-fields]` | Validates var values against event field patterns |
| Entry filter | `gates/types.rs` | `[entry-matches-filter]` | Checks ledger entry against k/v filter |
| Gate context | `gates/evaluator.rs` | `GateContext` | All inputs needed to evaluate a gate |
| Gate attestation | `gates/evaluator.rs` | `GateAttestation` | Evidence from an external command execution (gate_type, command, exit_code, stdout_hash, wall_time_ms, executed_at) |
| Gate result | `gates/evaluator.rs` | `GateResult` | Outcome: passed, evaluable, gate_type, description, reason, intent, attestation |
| Default intent | `gates/evaluator.rs` | `default_intent` | Returns default intent string for each gate type |
| evaluate_gate | `gates/evaluator.rs` | `[evaluate-gate]` | Evaluate single gate |
| evaluate_gates | `gates/evaluator.rs` | `[evaluate-gates]` | Evaluate all gates, returns all results |
| Shell command gate | `gates/command.rs` | `[eval-command-succeeds]` | Run command, pass if exit 0 |
| Command output gate | `gates/command.rs` | `[eval-command-output]` | Run command, pass if stdout matches |
| File exists gate | `gates/file.rs` | `[eval-file-exists]` | Single file check |
| Files exist gate | `gates/file.rs` | `[eval-files-exist]` | Multiple files check |
| Ledger event gate | `gates/ledger.rs` | `[eval-ledger-has-event]` | N+ events of type; optional `max_count` for budget enforcement |
| Event since gate | `gates/ledger.rs` | `[eval-ledger-has-event-since]` | Event since reference point (last_transition or custom event type) |
| Ledger lacks event gate | `gates/ledger.rs` | `[eval-ledger-lacks-event]` | Pass if NO matching events exist (negation gate) |
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
| Unresolved var scan | `gates/template.rs` | `[find-unresolved-vars]` | Detect leftover `{{key}}` placeholders after resolution |
| any_of gate | `gates/types.rs` | `[eval]` (inline) | Composite: pass if any child gate passes |
| all_of gate | `gates/types.rs` | `[eval]` (inline) | Composite: pass if all child gates pass |
| not gate | `gates/types.rs` | `[eval]` (inline) | Composite: invert result of single child gate |
| k_of_n gate | `gates/types.rs` | `[eval]` (inline) | Composite: pass if >= k of n child gates pass |

### state/ — State Machine

| Concept | File | Anchor/Item | Purpose |
|---------|------|-------------|---------|
| State machine | `state/machine.rs` | `StateMachine` | Owns config + ledger, executes transitions |
| Transition outcome | `state/machine.rs` | `TransitionOutcome` | Result of a successful transition (from, to, attestations) |
| Transition | `state/machine.rs` | `[transition]` | Execute named command: build params → check gates → append event → emit gate_attestation events |
| Build state params | `state/machine.rs` | `[build-state-params]` | Derive params from state config + set state (`source` field) |
| Record event | `state/machine.rs` | `[record-event]` | Append event to ledger |
| Set status | `state/machine.rs` | `[set-status]` | Completion status of a named set |
| Derive state | `state/machine.rs` | `[derive-state]` | Find current state from last state_transition in ledger |
| Completed members | `state/machine.rs` | `[completed-members]` | Scan ledger for set_member_complete events |
| Set types | `state/sets.rs` | `MemberStatus`, `SetStatus` | Completion tracking structs |
| State errors | `state/machine.rs` | `StateError` | NoTransition, GateBlocked, Ledger, etc. |
| All blocked error | `state/machine.rs` | `StateError::AllCandidatesBlocked` | All candidate transitions for a command were gate-blocked; carries per-candidate results |

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
| Registry entry | `ledger/registry.rs` | `LedgerRegistryEntry` | name, path, mode, created, template, instance_id |
| Ledger mode | `ledger/registry.rs` | `LedgerMode` | Full vs EventOnly |
| Register with template | `ledger/registry.rs` | `create_with_template` | Register ledger with template + instance_id metadata |
| Template query | `ledger/registry.rs` | `resolve_by_template` | Find all entries for a given template name |
| Config seal init | `ledger/chain.rs` | `init_with_seals` | Create genesis with config integrity seals |
| Find effective seal | `ledger/chain.rs` | `[find-effective-seal]` | Most recent config_reseal or genesis seals |
| Verify config seal | `ledger/chain.rs` | `[verify-config-seal]` | Verify config files match sealed hashes |
| Config integrity error | `ledger/entry.rs` | `ConfigIntegrityViolation` | Error when config files don't match seal |

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
| where_eq filter | `render/engine.rs` | `filter_where_eq` | Tera filter: keep array items where attribute == value (dot-notation supported) |
| unique_by filter | `render/engine.rs` | `filter_unique_by` | Tera filter: deduplicate array by field, keeping last occurrence (dot-notation supported) |
| Active ledger name | `render/engine.rs` | `with_active_ledger_name` | Set active ledger for template resolution |
| Resolve render ledger | `render/engine.rs` | `resolve_render_ledger` | Dispatch to by-name or by-template resolution |
| Resolve by name | `render/engine.rs` | `resolve_ledger_by_name` | Literal registry lookup |
| Resolve by template | `render/engine.rs` | `resolve_ledger_by_template` | Template metadata lookup (active first, then most recent) |
| Open registry entry | `render/engine.rs` | `open_registry_entry` | Shared helper to open ledger from registry entry |
| Build context | `render/engine.rs` | `[build-context]` | Build template vars from ledger + config; injects `template_instance_id` / `template_name` from registry |
| Render triggered | `render/engine.rs` | `[render-triggered]` | Render on_transition / on_event |
| Dump context | `render/engine.rs` | `[dump-context]` | Export render context as JSON |

### query/ — SQL Query Engine

| Concept | File | Anchor/Item | Purpose |
|---------|------|-------------|---------|
| Query engine | `query/mod.rs` | `QueryEngine` | DataFusion over JSONL ledger files |
| Query file | `query/mod.rs` | `[query-file]` | Run SQL against single ledger |
| Query glob | `query/mod.rs` | `[query-glob]` | Run SQL against multiple ledgers |

### mermaid/ — Protocol Visualization

| Concept | File | Anchor/Item | Purpose |
|---------|------|-------------|---------|
| Mermaid diagram | `mermaid.rs` | `[generate-mermaid]` | Emit `stateDiagram-v2` text from protocol config |
| ASCII tree | `mermaid.rs` | `[generate-ascii]` | DFS tree-walk diagram; detects cycles and fallback candidates |

### hooks/ — Claude Code Integration

| Concept | File | Anchor/Item | Purpose |
|---------|------|-------------|---------|
| Hook generator | `hooks/generate.rs` | `HookGenerator` | Produces Python hook scripts |
| Generated hook | `hooks/generate.rs` | `GeneratedHook` | Hook type + content |
| Hook eval request | `hooks/eval.rs` | `HookEvalRequest` | Incoming evaluation request (event, tool, file, output_text) |
| Hook eval result | `hooks/eval.rs` | `HookEvalResult` | Aggregate result (decision, messages, auto_records, monitor_warnings) |
| Hook message | `hooks/eval.rs` | `HookMessage` | Single enforcement message (source, rule_index, action, message) |
| Auto-record result | `hooks/eval.rs` | `AutoRecordResult` | Event to auto-record in ledger |
| Monitor warning | `hooks/eval.rs` | `MonitorWarning` | Monitor that fired (name, message) |
| Evaluate hooks | `hooks/eval.rs` | `evaluate_hooks` | Main entry: managed paths, write-gated, hooks, monitors |
| Derive current state | `hooks/eval.rs` | `derive_current_state` | Find current state from last state_transition |
| Hook matching | `hooks/eval.rs` | `hook_matches` | Check event/tool/states/filter |
| Glob matching | `hooks/eval.rs` | `glob_match` | Simple glob: `*`, `**`, `*.ext` |
| Hook condition eval | `hooks/eval.rs` | `eval_hook_condition` | Gate (fails=fire), check (matches=fire) |
| Managed path check | `hooks/eval.rs` | `eval_managed_paths` | Block writes to paths.managed |
| Write-gated check | `hooks/eval.rs` | `eval_write_gated` | Block writes outside writable_in states |
| Monitor eval | `hooks/eval.rs` | `eval_monitors` | Evaluate monitor triggers |

### daemon/ — Daemon Mode (Secret Holding + Unix Socket Server)

| Concept | File | Anchor/Item | Purpose |
|---------|------|-------------|---------|
| Daemon server | `daemon/mod.rs` | `DaemonServer` | Main server struct (socket_path, pid_path, session_key, vault, config/data dirs, trusted_callers) |
| Server init | `daemon/mod.rs` | `DaemonServer::new` | Preload check, stale cleanup, key gen, mlock, deny debug, load trusted callers, idle timeout |
| Server start | `daemon/mod.rs` | `DaemonServer::start` | Bind socket, set 0600 perms, write PID, signal handling, non-blocking accept loop |
| Idle timeout | `daemon/mod.rs` | `DaemonServer::start` | last_activity tracking in accept loop; clean shutdown on idle_timeout expiry |
| Server cleanup | `daemon/mod.rs` | `DaemonServer::cleanup` | Remove socket and PID files |
| Handle connection | `daemon/mod.rs` | `handle_connection` | Read JSON lines from stream, dispatch to handle_request, write responses |
| Handle request | `daemon/mod.rs` | `handle_request` | Dispatch Request variant to sign/vault/status/enforcement operation |
| Compute sign | `daemon/mod.rs` | `compute_sign` | HMAC-SHA256 proof computation (same algorithm as authed_event.rs) |
| Canonical payload | `daemon/mod.rs` | `build_canonical_payload` | Build HMAC payload: event_type + null-separated sorted fields |
| Enforcement handlers | `daemon/mod.rs` | `handle_request` | enforcement_read/write/update: opaque JSON state in vault under `_enforcement` (#27) |
| Reserved vault namespace | `daemon/mod.rs` | `handle_request` | `_`-prefixed names rejected by generic vault ops, filtered from vault_list (#27) |
| Wire request | `daemon/protocol.rs` | `Request` | Tagged enum for incoming JSON operations (sign, vault_store, vault_read, vault_delete, vault_list, status, verify, enforcement_read, enforcement_write, enforcement_update) |
| Wire response | `daemon/protocol.rs` | `Response` | Output envelope; constructors: ok_sign, ok_data, ok_names, ok_status, ok_empty, err, err_with_reason; ok_status includes enforcement_active bool; includes optional `reason` field (#26) |
| Trusted callers manifest | `daemon/auth.rs` | `TrustedCallersManifest` | Loads trusted-callers.toml (path → sha256 hash map) |
| Caller verification | `daemon/auth.rs` | `TrustedCallersManifest::verify_caller` | Checks relative script path is in manifest and its SHA-256 matches |
| Script path extractor | `daemon/auth.rs` | `extract_script_path` | Extracts first non-flag arg from interpreter cmdline (the script path) |
| Auth error | `daemon/auth.rs` | `AuthError` | NotInManifest, HashMismatch, ScriptNotFound, NoScriptPath, ManifestLoad, ManifestParse, Platform |
| Auth reason codes | `daemon/auth.rs` | `AuthError::reason_code` | Maps error to diagnostic reason: pid_resolution_failed, hash_mismatch, peer_cred_unavailable (#26) |
| Find trusted ancestor | `daemon/auth.rs` | `find_trusted_ancestor` | Walk process ancestor chain looking for trusted script in manifest (#26) |
| Peer authentication | `daemon/auth.rs` | `authenticate_peer` | PID-based caller auth via ancestor walk: peer PID → exe check → walk ancestors → cmdline → manifest verify (#26) |
| Peer PID | `daemon/platform.rs` | `[get-peer-pid]` | Extract connecting PID from Unix socket (macOS: LOCAL_PEERPID, Linux: SO_PEERCRED) |
| Exe path | `daemon/platform.rs` | `[get-exe-path]` | Resolve PID to executable path (macOS: proc_pidpath, Linux: /proc/pid/exe) |
| Command line | `daemon/platform.rs` | `[get-cmdline]` | Read process command-line arguments (macOS: KERN_PROCARGS2, Linux: /proc/pid/cmdline) |
| Parent PID | `daemon/platform.rs` | `[get-parent-pid]` | Look up parent PID (macOS: proc_pidinfo, Linux: /proc/pid/status) |
| Deny debug | `daemon/platform.rs` | `[deny-debug-attach]` | Prevent debugger attachment (macOS: PT_DENY_ATTACH, Linux: PR_SET_DUMPABLE) |
| Memory lock | `daemon/platform.rs` | `[try-mlock]` | Best-effort memory page locking (both: libc::mlock) |
| Preload check | `daemon/platform.rs` | `[check-preload-env]` | Detect LD_PRELOAD / DYLD_INSERT_LIBRARIES |
| Vault | `daemon/vault.rs` | `Vault` | In-memory Zeroizing key-value store (store, read, delete, list) |

### cli/ — Command Implementations

| Concept | File | Anchor | Purpose |
|---------|------|--------|---------|
| CLI entry point | `main.rs` | `[cli-main]` | clap arg parsing, alias resolution, dispatch; `--json` global flag |
| Alias resolution | `cli/aliases.rs` | `[resolve-alias]` | Rewrite CLI args via protocol aliases |
| JSON output types | `cli/output.rs` | `CommandOutput`, `CommandResult<T>`, data structs | Structured output with JSON envelope (`schema_version: 1`) |
| Shared helpers | `cli/commands.rs` | (see file index) | Exit codes, ledger targeting, config loading, active-ledger marker, `[compute-registry-path]`, `[status-cache-path]`, `[write-status-cache]` |
| Init/validate/reset | `cli/init.rs` | `[cmd-init]`, `[cmd-validate]`, `[cmd-reset]` | Lifecycle commands; init writes status-cache.json; reset requires HMAC proof via daemon (#26) |
| Transition/gate/event | `cli/transition.rs` | `[cmd-transition]`, `[cmd-gate-check]`, `[record-and-render]`, `validate_event_fields`, `[cmd-event]` | State machine commands; transition updates status-cache.json |
| Status/sets | `cli/status.rs` | `[cmd-status]`, `[cmd-set-status]`, `[cmd-set-complete]` | Status display + set management; status warns on missing cache |
| Log inspection | `cli/log.rs` | `[cmd-log-dump]`, `[cmd-log-verify]`, `[cmd-log-tail]` | Ledger viewing |
| Ledger management | `cli/ledger.rs` | `[cmd-ledger-create]`, `[cmd-ledger-list]`, `[cmd-ledger-activate]`, `[cmd-ledger-deactivate]`, etc. | Multi-ledger CRUD; create supports `--from` template + `--activate`; activate/deactivate manage active-ledger marker |
| Query | `cli/query.rs` | `[cmd-query]` | SQL queries over events |
| Render | `cli/render.rs` | `[cmd-render]`, `[cmd-render-dump-context]` | Template rendering |
| Manifest | `cli/manifest_cmd.rs` | `[cmd-manifest-verify]`, `[cmd-manifest-list]` | File integrity |
| Authed event | `cli/authed_event.rs` | `[cmd-authed-event]` | HMAC-verified restricted event recording (proof verified via daemon) |
| Hooks | `cli/hooks_cmd.rs` | `[cmd-hook-generate]`, `[cmd-hook-eval]` | Hook script generation + runtime evaluation |
| Mermaid | `cli/mermaid.rs` | `[cmd-mermaid]` | Diagram generation command (stateDiagram-v2 or ASCII) |
| Reseal | `cli/authed_event.rs` | `[cmd-reseal]` | HMAC-authenticated config reseal (proof verified via daemon) |
| Verify proof | `cli/verify_cmd.rs` | `[cmd-verify]` | Verify HMAC-SHA256 proof via daemon socket |
| Daemon start | `cli/daemon_cmd.rs` | `[cmd-daemon-start]` | Start daemon in foreground (accepts idle_timeout) |
| Daemon stop | `cli/daemon_cmd.rs` | `[cmd-daemon-stop]` | Stop running daemon (SIGTERM, then SIGKILL) |
| Daemon status | `cli/daemon_cmd.rs` | `[cmd-daemon-status]` | Query daemon status via socket |
| Socket path resolver | `cli/daemon_cmd.rs` | `[resolve-socket-path]` | Resolve daemon socket path from config |
| Socket request helper | `cli/daemon_cmd.rs` | `[connect-and-request]` | Send JSON request to daemon socket, read response |
| Sign via daemon | `cli/sign_cmd.rs` | `[cmd-sign]` | Request HMAC-SHA256 proof from daemon |
| Vault store | `cli/vault_cmd.rs` | `[cmd-vault-store]` | Store file contents in daemon vault |
| Vault read | `cli/vault_cmd.rs` | `[cmd-vault-read]` | Read vault entry to stdout |
| Vault delete | `cli/vault_cmd.rs` | `[cmd-vault-delete]` | Delete vault entry |
| Vault list | `cli/vault_cmd.rs` | `[cmd-vault-list]` | List vault entry names |

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
      → for each candidate transition (in TOML order):
        → for each gate:
          → state/machine.rs [evaluate-gate]     ← builds GateContext with state_params
            → gates/evaluator.rs [evaluate-gate]
              → gates/types.rs [eval]            ← dispatches by gate_type string
                → gates/command.rs [eval-command-succeeds]  (or other gate module)
                  → gates/types.rs [build-template-vars]    ← clones state_params + injects config paths/sets
                  → gates/types.rs [validate-template-fields]
                  → gates/template.rs [resolve-template]    ← {{var}} → shell-escaped value
                  → gates/command.rs [run-shell-output-with-timeout]
        → if all gates pass: take this candidate, break
        → if any gate fails: try next candidate
      → if no candidate passed: StateError::AllCandidatesBlocked
      → ledger/chain.rs [ledger-reload]          ← re-read after gate commands may have appended
      → ledger/chain.rs [ledger-append]           ← state_transition event
      → for each GateAttestation from passing gates:
        → ledger/chain.rs [ledger-append]           ← gate_attestation event (stdout_hash, exit_code, etc.)
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
  "ledger_lacks_event"  → gates/ledger.rs   [eval-ledger-lacks-event]
  "set_covered"         → gates/ledger.rs   [eval-set-covered]
  "min_elapsed"         → gates/ledger.rs   [eval-min-elapsed]
  "no_violations"       → gates/ledger.rs   [eval-no-violations]
  "field_not_empty"     → gates/ledger.rs   [eval-field-not-empty]
  "snapshot_compare"    → gates/snapshot.rs  [eval-snapshot-compare]
  "query"               → gates/query.rs    [eval-query-gate]
  "any_of"              → gates/types.rs    [eval] (inline) — pass if any child passes
  "all_of"              → gates/types.rs    [eval] (inline) — pass if all children pass
  "not"                 → gates/types.rs    [eval] (inline) — invert single child result
  "k_of_n"              → gates/types.rs    [eval] (inline) — pass if >= k children pass
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
    → reads hooks.toml (optional)   → config/hooks.rs HooksFile
  → config/mod.rs [validate] — structural checks
  → config/mod.rs [validate-deep] (via cmd_validate) — file/alias/gate/ledger checks
```

### Flow: Ledger Resolution Order

How `--ledger` / active-ledger marker / default are resolved:

```
cli/commands.rs [resolve-ledger]
  1. --ledger-path <path>         ← explicit file path, highest priority
  2. --ledger <name>              ← named ledger from registry
  3. active-ledger marker         ← {data_dir}/active-ledger file; warn if unregistered
  4. registry default ("default") ← fallback; else data_dir/ledger.jsonl

cli/commands.rs [determine-ledger-source]
  → mirrors resolution order to return (name, source_label) for status display
```

### Flow: Config Integrity Verification

How config seals are created and verified:

```
cli/init.rs [cmd-init]
  → config/mod.rs compute_config_seals()      ← SHA-256 of all 6 TOML files
  → ledger/chain.rs init_with_seals()          ← seals stored in genesis entry fields

cli/commands.rs [open-ledger] or [open-targeted]
  → ledger/chain.rs Ledger::open()             ← parse and verify hash chain
  → ledger/chain.rs [verify-config-seal]
    → ledger/chain.rs [find-effective-seal]     ← scan for config_reseal, fall back to genesis
    → config/mod.rs compute_config_seals()      ← re-hash current files
    → compare: mismatch → ConfigIntegrityViolation

cli/authed_event.rs [cmd-reseal]
  → HMAC proof verification (session key)
  → config/mod.rs compute_config_seals()        ← new hashes
  → ledger/chain.rs [ledger-append]             ← config_reseal event
```

### Flow: Hook Evaluation

How `sahjhan hook eval --event PreToolUse --tool Edit --file src/main.rs` executes:

```
main.rs [cli-main]
  → cli/hooks_cmd.rs [cmd-hook-eval]
    → cli/commands.rs [load-config]              ← on failure, return allow
    → cli/commands.rs [open-targeted]            ← on failure, return allow
    → parse event string → HookEvent enum
    → hooks/eval.rs evaluate_hooks()
      → hooks/eval.rs derive_current_state()     ← last state_transition "to" field
      → hooks/eval.rs eval_managed_paths()       ← PreToolUse Edit/Write only
      → hooks/eval.rs eval_write_gated()         ← PreToolUse Edit/Write only; glob match + state check
      → for each hook:
        → hooks/eval.rs hook_matches()           ← event/tool/states/filter
        → if auto_record: resolve templates, add to auto_records
        → if gate: gates/evaluator.rs evaluate_gate() — fire if gate FAILS
        → if check: eval check condition (output_contains_any, event_count_since_last_transition)
      → hooks/eval.rs eval_monitors()            ← event_count_since_last_transition triggers
    → for auto_records: ledger.append()           ← record auto events
    → decision: block > warn > allow
    → CommandResult with exit_code 1 (block) or 0 (allow/warn)
```

---

## Test Files

| Test file | Tests |
|-----------|-------|
| `tests/gate_tests.rs` | All gate types, template interpolation, field validation, StateParam source, attestation |
| `tests/integration_tests.rs` | Full CLI end-to-end (init, transition, events, queries, renders, sets) |
| `tests/chain_integrity_tests.rs` | Ledger hash chain, append, reload, tamper detection |
| `tests/config_tests.rs` | Config loading, validation, hooks/monitors/write_gated validation |
| `tests/state_machine_tests.rs` | StateMachine transitions, gates, sets |
| `tests/query_tests.rs` | DataFusion SQL queries over ledger |
| `tests/ledger_tests.rs` | LedgerEntry serialization, hashing, schema |
| `tests/manifest_tests.rs` | Manifest tracking, verification, restore |
| `tests/registry_tests.rs` | Multi-ledger registry CRUD |
| `tests/checkpoint_tests.rs` | Ledger checkpointing |
| `tests/import_tests.rs` | JSONL import |
| `tests/hook_generation_tests.rs` | Hook script generation |
| `tests/template_security_tests.rs` | Shell escaping, injection prevention |
| `tests/template_tests.rs` | Template-based ledger creation via cmd_ledger_create |
| `tests/auth_tests.rs` | Session key generation, restricted events, HMAC auth |
| `tests/mermaid_tests.rs` | Mermaid stateDiagram-v2 output, hyphen sanitization, gate labels, ASCII tree, cycle detection |
| `tests/config_integrity_tests.rs` | Config sealing, tamper detection, reseal, backward compat |
| `tests/render_filter_tests.rs` | Custom Tera filters (where_eq, unique_by) |
| `tests/json_output_tests.rs` | JSON envelope serialization, per-command data structs, CLI --json integration |
| `tests/horizons1_tests.rs` | HORIZONS-1 mission protocol: status, transitions, gates, sets with --json |
| `tests/hook_eval_tests.rs` | Hook evaluation engine: gate/check/filter/state/monitor/write-gated/managed-path/CLI eval |
| `tests/concurrent_append_tests.rs` | Concurrent ledger append stress tests (issue #21 TOCTOU race) |
| `tests/daemon_platform_tests.rs` | Platform API smoke tests: preload env, exe path, cmdline, parent PID, mlock |
| `tests/daemon_protocol_tests.rs` | Wire protocol types: Request deserialization (all ops + unknowns), Response serialization (all constructors incl. idle fields) |
| `tests/daemon_auth_tests.rs` | Trusted-callers manifest load/parse, hash match/mismatch, not-in-manifest, extract_script_path |
| `tests/daemon_vault_tests.rs` | Vault CRUD: store/read, overwrite, delete, list, read-not-found, delete-noop |
| `tests/daemon_signing_tests.rs` | E2E daemon signing (deterministic proofs, sign-without-daemon), lifecycle (socket/PID creation, stop cleanup, status, preload rejection, idle timeout shutdown), reset auth (#26), auth reason codes (#26), ancestor walk auth (#26) |
| `tests/daemon_vault_e2e_tests.rs` | E2E vault via CLI: store+read, list, delete, read-nonexistent (all require live daemon) |
| `tests/daemon_enforcement_tests.rs` | Enforcement state ops: write/read round-trip, update merge, not_found, reserved namespace, vault_list filtering, status enforcement_active, validation (#27) |
| `tests/active_ledger_tests.rs` | Active-ledger marker: activate/deactivate, create --activate, resolution priority, stale marker fallback, reset clears marker, status display, events land in active ledger |
