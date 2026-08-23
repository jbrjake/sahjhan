# CLI reference

Every command accepts these:

```
--config-dir <path>     protocol config directory      [default: enforcement]
--ledger <name>         target a named ledger from the registry
--ledger-path <path>    target a ledger file directly
--json                  emit a structured JSON envelope instead of human output
```

Aliases declared in `protocol.toml` create shortcuts, so `"start" = "transition
start"` makes `sahjhan start` work.

## lifecycle

```
sahjhan init                              Initialize ledger, registry, manifest, genesis
sahjhan validate                          Check protocol config (gates, sets, templates)
sahjhan lint [--only <CHECK>] [--strict]  Static integrity analysis of the protocol graph
sahjhan status [--no-gates]               Current state, set progress, gate status
sahjhan reset --confirm --proof <HMAC>    Archive the current run and restart
```

`--no-gates` skips transition gate evaluation. Plain `status` runs every
candidate transition's gates, which can spawn a test suite; `--no-gates` is
side-effect-free and safe for a hook under a short timeout.

## running the protocol

```
sahjhan transition <command> [args...]    Execute a named transition (runs gates)
sahjhan gate check <command> [args...]    Dry-run gate evaluation (✓ / ✗ / ?)
sahjhan event <type> [--field K=V]        Record a protocol event
sahjhan set status <set>                  Show set completion progress
sahjhan set complete <set> <member>       Record set member completion
sahjhan render                            Regenerate markdown views from the ledger
sahjhan render dump-context               Dump the render context as JSON
sahjhan mermaid [--rendered]              Protocol diagram: stateDiagram-v2, or ASCII
```

`sahjhan event` refuses `restricted` event types.

## the ledger

```
sahjhan log dump                          Print the ledger as JSONL
sahjhan log verify                        Validate hash chain integrity
sahjhan log tail [N]                      Last N events                    [default: 10]
sahjhan manifest verify                   Check tracked files against the manifest
sahjhan manifest list                     Show tracked files and hashes
sahjhan manifest restore <path>           Restore a file from its known-good state

sahjhan query "<SQL>"                     SQL against the ledger
sahjhan query --type <type> [--count]     Convenience: filter by event type
sahjhan query --glob <pattern> "<SQL>"    Query across multiple ledger files
sahjhan query --format table|json|csv|jsonl

sahjhan ledger create --name <n> --path <p> [--mode stateful|event-only] [--activate]
sahjhan ledger create --from <template> <instance_id> [--activate]
sahjhan ledger list                       Show registered ledgers
sahjhan ledger remove --name <n>          Unregister (the file stays)
sahjhan ledger verify [--name <n>]        Validate chain integrity
sahjhan ledger checkpoint [--name <n>]    Write a checkpoint event
sahjhan ledger import --name <n> --path <p>    Import bare JSONL from stdin
sahjhan ledger activate <name>            Set the active-ledger marker
sahjhan ledger deactivate                 Clear it
```

## authenticated operations

All of these talk to the daemon. See [hardening.md](hardening.md).

```
sahjhan daemon start [--idle-timeout N]   Start the daemon (foreground; 0 = no timeout)
sahjhan daemon stop                       SIGTERM, then SIGKILL
sahjhan daemon status                     Query daemon health

sahjhan sign --event-type <type> [--field K=V]        Get an HMAC proof
sahjhan verify --event-type <type> --proof <HMAC> [--field K=V]
sahjhan authed-event <type> --proof <HMAC> [--field K=V]   Record a restricted event
sahjhan reseal --proof <HMAC>             Re-seal config hashes after a legitimate change

sahjhan vault store --name <n> --file <f>  Store file contents in daemon memory
sahjhan vault read --name <n>              Retrieve it
sahjhan vault delete --name <n>            Remove it (zeroed)
sahjhan vault list                         List entry names
```

## hooks

```
sahjhan hook generate [--harness cc] [--output-dir <d>]   Generate integration hooks
sahjhan hook eval --event <E> [--tool <T>] [--file <F>] [--output-text <text>]
```

`hook eval` always emits JSON. See [hooks.md](hooks.md).

## exit codes

| code | meaning |
| --- | --- |
| 0 | success |
| 1 | gate blocked, or a hook returned `block` |
| 2 | integrity error (hash chain, manifest, or config seal) |
| 3 | config error, including lint findings at error severity |
| 4 | usage error |

Exit code 1 is a protocol decision, not a failure — a blocked transition is
sahjhan working. Exit code 2 means something that shouldn't have changed did.

## the JSON envelope

`--json` wraps every command's output in one shape:

```json
{"command":"status","ok":true,"schema_version":1,"data":{ ... }}
```

```bash
$ sahjhan --json status
{"command":"status","data":{"chain_error":null,"chain_valid":true,"event_count":2,
"ledger_name":"default","ledger_source":"no active-ledger marker","sets":[{"completed":0,
"members":[{"done":false,"name":"tests"},{"done":false,"name":"lint"}],"name":"check",
"total":2}],"state":"working","transitions":[{"command":"complete","from":"working",
"gates":[{"description":"set 'check' is fully covered","evaluable":true,
"gate_type":"set_covered","intent":"all set members must be completed","passed":false,
"reason":"set 'check' not fully covered; missing: tests, lint"}],"ready":false,
"to":"done"}]},"ok":true,"schema_version":1}
```

`data` is per-command. `schema_version` is 1 and will change if the envelope
does.
