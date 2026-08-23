# Design note — establishing who is on the other end of the socket

Working note, not committed. It records a design direction and the questions
still open on it, so the next session doesn't start cold.

## The puzzle

The daemon holds a few things the surrounding hooks need and the agent should
not have directly: a signing key, a small in-memory vault (the quiz bank), and
enforcement state. Hooks reach these over a Unix socket. So the daemon has to
answer one question every time a connection arrives:

> Is the process on the other end genuinely one of the known hook scripts,
> running its own code faithfully — or is it the agent standing in for a hook,
> or standing between a hook and the socket?

Everything below is about answering that question well. It is a puzzle about
*identity of a running process*, not about any one trick.

## Why today's check doesn't settle it

Today the daemon establishes a fact about a *file*: walk up the process tree,
find a script under `--config-dir` that appears in `trusted-callers.toml`, and
confirm its SHA-256 matches. That fact is true when it passes, but it's the
wrong fact. It describes a file on disk. It says nothing about what code the
connected process is actually running. Three ordinary situations make the file
check pass while the running code is the agent's:

1. **A helper inherits the authority.** The ancestor walk accepts the connection
   if *any* process up to ten levels up is a known script. So anything a known
   hook launches — including a program that happens to be found first on `PATH`
   under the name the hook expected — connects as a trusted descendant.

2. **Extra code loads inside the hook's own process.** The interpreter can be
   pointed at an additional module directory, or a same-named module can sit
   right next to the hook and be picked up ahead of the standard library, or a
   user-site startup file can run, or a loader-level library can load before the
   interpreter even starts. In each case the connected process *is* the known
   hook by path and hash, but the code it's running isn't only the hook's.
   (Confirmed today: a file named `json.py` placed next to a hook is imported in
   place of the standard-library `json`.)

3. **A different program stands in for the interpreter.** `python3 -E -s hook.py`
   where `python3` resolves to a program that ignores the script entirely and
   just opens the socket. The script path and flags look right; the thing running
   them isn't an interpreter.

Hashing the entry script proves the file is unmodified. It doesn't establish the
interpreter, the set of modules that load, the loader, or what the process later
launches — which is the whole of what we actually care about.

## The reframe

Stop trying to establish *who called* by walking lineage. Establish *what is
running* on the connection itself. Concretely: the daemon grants authority only
to the process directly holding the socket, and only when it can show that
process is a known hook running its own code — and that authority extends to
nothing else, in either direction.

This fits how the hooks already work. Every privileged call in the current
consumer opens the socket **directly from the Python hook process** (verified:
`_daemon_request`, `record_authed_event`, `compute_event_proof`). The genuine
peer is the hook's own interpreter, right there on the connection. The ancestor
walk isn't serving that path at all — removing it costs the real flow nothing.

## The design — six things the daemon checks about the direct peer

Each row handles one of the situations above. "Verified" means checked
empirically while writing this.

| # | Check | Situation it handles | Change for the consumer |
|---|-------|----------------------|-------------------------|
| 1 | **Peer only.** Authenticate the process holding the socket; don't extend authority to what it launched or what launched it (remove the ancestor walk). | 1 — a helper inheriting authority | None. Privileged calls are already socket-direct (verified). |
| 2 | **Peer is a known entry** (path + SHA-256), checked on the peer itself. | baseline identity | None. |
| 3 | **Whole module set matches.** Confirm every `.py` in the hook tree matches the manifest and no unlisted `.py` is present — on every privileged connect, not just the entry file. | 2 — a same-named or swapped-in module loading next to the hook (verified real) | The manifest must list the *entire* importable tree. Today 7 modules are unlisted, including two that privileged hooks import. |
| 4 | **Started in isolated-import mode.** The interpreter was launched `-E -s` (ignore outside module paths and user-site) and handed the hook as a script — not `-c`, `-m`, or stdin. | 2 — an outside module directory or user-site startup loading ahead of the hook | One added flag pair per hook command. |
| 5 | **No loader-level module variables.** Read the process's start-time environment from the kernel; none of the loader's library-loading variables were set. | 2 — a library loaded before the interpreter starts (which flags can't reach) | None (the consumer sets none — verified). |
| 6 | **A real interpreter is running.** The executable actually running, read from the kernel rather than from what the process reports about itself, is a genuine interpreter. | 3 — a stand-in program in place of the interpreter | Depends on how identity is bound — see open questions. |

Rows 1 and 3 are the simple, high-leverage core. Rows 4–6 are the fiddly
platform layer. Row 5 needs a new kernel reader for the start-time environment;
the existing code already parses the neighbouring command-line block on macOS, so
the same call site extends to it (the environment sits just before the trailing
lowercase `apple[]` entries — that boundary needs care in the parse).

Two facts checked today that keep row 4 from breaking the consumer: `-E`
prevents a module that lives only on an outside path from loading, and `-E -s`
still lets a hook import its siblings (the script's own directory stays first on
the path). `-I` does **not** — it drops the script directory and the sibling
imports fail — so `-E -s`, not `-I`.

## What the consumer would change

Small and mechanical, but real — this isn't a drop-in:

- **Hook launch:** each hook command gains `-E -s` (e.g. `python3 -E -s ".../hook.py"`).
- **Manifest completeness:** `trusted-callers.toml` must cover every importable
  `.py` in the hook tree, then be regenerated. The tree has 18 modules; the
  manifest lists 11. The seven currently unlisted include `quiz_vault.py` and
  `_protocol_cache.py`, both imported by privileged hooks — so completing the
  manifest also closes a gap that exists right now.

Because the auth contract changes and the consumer has to change with it, this
is a minor/major release with a migration note, not a patch.

## Open questions

These are the parts not yet settled. Some may not have a clean answer from the
daemon alone; that's what makes them open rather than done.

1. **Binding the interpreter's identity (row 6).** The executable the kernel
   reports for the peer has to be a genuine interpreter, or row 4's flags are
   just decoration on a stand-in program. Two ways to bind it:
   - *By location* — require the real executable to sit under directories the
     agent can't write to (`/usr/bin`, `/opt/homebrew/bin`, the framework path,
     …). Portable and no manifest work, but it leans on "the run can't write
     there," which I haven't confirmed for a real audit run and won't assume.
   - *By pin* — record the interpreter's absolute path and hash in sealed config.
     No assumption about writability, but brittle across machines and interpreter
     versions (the consumer uses the system `python3`, which varies).

   Which one holds depends on a fact about how audits run. That fact is the thing
   to establish before committing to row 6.

2. **Reaching a genuine hook after it's connected.** Suppose all six checks pass
   and the real hook is running and connected. Is there anything stopping another
   same-user process from reaching into it — reading its memory, or taking over
   the open connection — after the fact? I don't have a way to rule that out from
   the daemon side today. Open: is there one, or does this have to come from the
   surrounding environment (for example, the run disallowing that kind of
   process-to-process access), and if the latter, what exactly do we require of
   the environment?

3. **One connection at a time.** The daemon handles a single connection
   sequentially, so a caller that holds one open stalls the others. Independent
   of everything above and pre-existing. Open: does that matter for this use, or
   is it fine to leave as-is?

## Alternatives weighed

- **Hand the hook a secret at launch that it presents on connect.** Appealing,
  but a secret placed in the environment is visible to the agent's own shell,
  which the surrounding tool launches from the same environment; and a secret
  passed as an inherited handle needs the launcher itself (Claude Code) to hand
  it only to hooks. That's an upstream change we don't control. Right answer *if*
  we owned the launcher; we don't.
- **Move the held secret somewhere the run can't reach at all** (a separate user,
  a remote scorer). The most durable answer, and the only one that sidesteps
  open question 2 entirely — but it's a change to how the consumer is built, not
  to this daemon.

The six-check design is the most the daemon can establish on its own, and it
settles situations 1–3 — the whole class the reported gap belongs to. Open
question 2 is the honest edge of what it reaches.

## Decision needed

1. Build the six-check design, or take a different direction?
2. For open question 1 (interpreter identity): by-location or by-pin — which
   depends on whether an audit run can write to system executable directories.
   That's the fact I need.

No code, no `hooks.json` edits, and nothing touched in the prose until there's a
direction. When it's built, the daemon changes and any consumer changes land as
separate commits, each shown first.
