# internals

## layers

```
+--------------------------------------------------+
|             Protocol Definition                   |
|          (TOML config files, per-project)         |
|   states, transitions, gates, events, sets,       |
|   hooks, monitors, ledger templates               |
+--------------------------------------------------+
|              Sahjhan Engine                        |
|           (Rust binary, reusable core)             |
|   state machine, JSONL hash-chain ledger,         |
|   DataFusion query engine, gate evaluator,        |
|   manifest, template renderer, ledger registry,   |
|   hook evaluator, lint                            |
+--------------------------------------------------+
|              Daemon Process                        |
|        (Unix socket, in-memory secrets)            |
|   session key, HMAC signing, vault,               |
|   caller authentication (PID + manifest)          |
+--------------------------------------------------+
|               Hook Bridge                         |
|        (generated scripts, per-harness)           |
|   PreToolUse / PostToolUse / Stop for Claude Code |
+--------------------------------------------------+
|               Filesystem                          |
|   ledger.jsonl, ledgers.toml, manifest.json,      |
|   active-ledger marker, rendered views,           |
|   trusted-callers.toml                            |
+--------------------------------------------------+
```

## the ledger format

One JSON object per line. Each carries a schema version, a monotonic sequence
number, an ISO 8601 timestamp, the event type, the event's fields, the engine
and protocol versions that wrote it, the previous entry's SHA-256 (the `prev`
key), and the SHA-256 of the current entry computed over
[RFC 8785](https://www.rfc-editor.org/rfc/rfc8785) canonical JSON —
alphabetically sorted keys, no whitespace, deterministic encoding. All of those
keys, `engine` and `protocol` included, feed the hash.

Tampering with one entry means recomputing every hash after it. The chain is
unkeyed, though — it proves the file is internally consistent, not who wrote
it — and nothing pins the tail, so truncation from the end is invisible to
`log verify`. What raises the cost of a wholesale rewrite lives outside the chain: the
manifest records the ledger file's hash, and a fresh genesis draws a new
CSPRNG nonce, so a rebuilt ledger is distinguishable from the one it replaced.

```bash
$ sahjhan log verify
Cannot open ledger: hash mismatch at seq 3: expected dac2a8c194250f652df538d379d64311af923ccfb8451d1c38685fcaeb0ebdde, got 7c0190f13e163e8ffe28097c6e04eae8dde4513920f36435e465296ad07a5c7a
```

The format is human-readable on purpose: you should be able to audit what your
agents did. Readability and editability are different properties, and the hash
chain makes the distinction sharp. `cat` the ledger all day. `sed` one character
and sahjhan says so on the next command.

## the manifest

`manifest.json` holds a SHA-256 for every managed file. Touch one through Bash
and the hash stops matching, and `sahjhan manifest verify` says so (exit 2).
Recording a `protocol_violation` for it is your protocol's job — a `hooks.toml`
rule or an explicit event; the engine only ever reads that event type. The
manifest records the ledger file's hash alongside its other entries and hashes
its own contents into a `manifest_hash` field — self-referential bookkeeping,
not an external anchor. The coupling runs one way: the manifest notices a
rewritten ledger, nothing in the ledger notices a rewritten manifest.

Verification reports three kinds of mismatch — `Modified`, `Missing`, and
`Unmanaged` — and only the first two count against a clean result.

## path anchoring

Every manifest key is spelled relative to the **project root**: the ancestor
directory holding the configured `data_dir`. Never the directory a command
happened to run from.

```
config paths.data_dir  (e.g. "docs/holtz/.sahjhan", relative)
  → project_root_from()             walk UP from cwd to the ancestor holding it
      ├→ data_dir                   = project_root.join(data_dir)
      ├→ manifest keys              = file.strip_prefix(project_root)
      ├→ manifest verify base_dir   = project_root
      └→ gate command working_dir   = project_root
```

The anchor is computed once and everything else is expressed in terms of it. That
is what makes `cd` harmless: the same file gets the same key from anywhere in the
tree, and a key landing outside the declared managed paths is refused at write
rather than registered as a file nobody can find. Gate commands run from that
same root, so a `cmd` written relative to the project resolves the same way for
whoever invokes it.

This was a real bug. Two derivations of one fact disagreed — the walk-up found
the root for `data_dir` while manifest keys were re-derived from
`std::env::current_dir()` at five call sites. A `cd` into a subdirectory then
produced a second spelling of an already-tracked file, which resolved to nothing
and read as permanent tampering. If you find yourself calling `current_dir()` on
the path of a managed file, you're reintroducing it.

## paths a user types vs. paths the system owns

Paths the system owns (`data_dir`, `render_dir`, manifest keys, the manifest
verify base, a gate command's working directory) resolve against the project
root. Paths a user types on the command line (`--config-dir`, `ledger create
--path`, `ledger import`) stay relative to the current directory.

## secrets

Session keys exist only in daemon process memory: never written to disk, never
sent over the wire. The daemon generates 32 random bytes at startup, locks the
pages with `mlock` where the OS allows, blocks debugger attachment, and refuses
to start if library injection environment variables are set.

Callers authenticate via kernel-provided socket peer credentials. The daemon
resolves the connecting PID — the direct peer, deliberately no walk up the
process tree — reads its command line, and checks the script's path and SHA-256
against `trusted-callers.toml`, which is itself sealed into the genesis entry —
so an in-place edit trips a config integrity violation instead of silently
reassigning signing authority. Authority is not inheritable and the CLI itself
never authenticates. This is hardening, not the security boundary; the boundary
is the sandbox fuse. Details in [hardening.md](hardening.md).

## template escaping

Template variables in gate commands are POSIX shell-escaped before
interpolation, and field patterns are validated before escaping. The `cmd` string
itself comes from sealed TOML; only the variable values come from the agent.

## locking

Exclusive file locks for writes, shared for reads. The five-second timeout
applies to the exclusive side; shared reads block without one.

## source layout

```
src/
  main.rs              CLI entry point (clap)
  lib.rs               Library root
  cli/                 Command modules (init, status, transition, log, lint,
                       ledger, query, render, manifest, hooks,
                       authed_event, sign, verify, daemon, vault), aliases
  daemon/              Daemon server, caller auth, vault, wire protocol,
                       platform abstraction (macOS + Linux)
  ledger/              JSONL entry, hash chain, registry, checkpoints, import
  state/               State machine executor, completion set tracking
  gates/               Gate evaluation: file, command, ledger, snapshot, query
  lint/                Static integrity analysis: graph, index, checks, similarity
  query/               DataFusion query engine, Arrow table builder
  manifest/            File hash tracking, integrity verification
  paths.rs             Project-root anchoring for system-owned paths
  config/              TOML parsing (protocol, states, transitions, events,
                       renders, hooks, vault_policy)
  render/              Tera template rendering (where_eq, unique_by filters)
  hooks/               Hook script generation and runtime evaluation
  mermaid.rs           stateDiagram-v2 and ASCII tree output
```

`CLAUDE.md` at the repo root is the navigation map: a per-concept lookup table
naming the file and the anchor slug to grep for.
