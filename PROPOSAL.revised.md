# Working note — protecting the daemon secret in a standalone plugin

Not committed. This is a **handoff / knowledge base** for anyone picking this
problem up fresh. Its job is to carry forward the problem and everything verified
this session so you don't re-derive it. Read top to bottom once; the design
attempts and their dead ends are recorded as *inputs*, not as a plan to build.

## Required reading before you start

- **`docs/archive/2026-08-19-trusted-caller-postmortem.md`** and **`HANDOFF.md`**
  — a **first attempt at fixing this was built and then reverted**; the
  postmortem is why. Both describe work superseded by the v0.21.0 fix.
  Read them before writing any code. (They were deliberately *not* read while
  writing this note, so their specifics aren't reflected here — they are prior art
  you must reconcile with.)
- **`PROPOSAL.md`** (repo root, Jon's original working note) — the original framing
  of the problem, focused on the daemon's per-connection "who is on the socket"
  question. This note supersedes its *approach* but not its problem statement.
- **`CLAUDE.md`** — codebase map (module lookup tables, flow maps, anchors). Use it
  to find the code referenced below.
- Do **not** edit Jon's prose (`PROPOSAL.md`, README, `docs/`). Propose changes;
  he decides.

---

## The problem

The sahjhan daemon holds secrets the surrounding hooks need and the agent under
audit must not get: an HMAC **signing key**, a small in-memory **vault** (the quiz
bank), and **enforcement state**. Hooks reach these over a Unix domain socket.

Constraints that shape every option:

- The agent runs **as the same OS user** as the daemon and the hooks. Unix
  ownership draws no line between them; the socket's `0600` perms exclude *other*
  users, not the agent.
- It must work as a **standalone Claude Code plugin** (holtz). The secret stays
  **inside the run** — no separate user, no remote scorer.
- The consumer (holtz) should change **as little as possible**, ideally nothing.
- Practically: whoever installs the plugin is trusted to set it up once; the
  **agent** is the adversary and must not be able to undo the protection.

## The original reported gap (what started this)

Today the daemon authenticates a caller by an **ancestor walk**: from the socket
peer's PID it walks up to **10 process ancestors**, and accepts the connection if
*any* of them is a script under the config dir that appears in `trusted-callers.toml`
with a matching SHA-256 (`auth.rs` `find_trusted_ancestor`/`authenticate_peer`).
The reported bug: **any descendant of a trusted hook inherits its authority** — a
helper a hook launches (or a program found first on `PATH` under the expected name)
connects as a trusted descendant. Three concrete situations the original note lays
out, all making the file-check pass while the running code is the agent's:

1. **A helper inherits authority** (the ancestor walk accepts any ancestor).
2. **Extra code loads inside the hook's own process** — a same-named module beside
   the hook shadowing stdlib, an added module dir, a user-site startup, or a
   loader-level library. **Verified: a `json.py` placed next to a hook is imported
   in place of the standard-library `json`.**
3. **A different program stands in for the interpreter** — `python3 …hook.py`
   where `python3` is a program that ignores the script and just opens the socket.

---

## What we've established (the knowledge base)

### 1. The daemon cannot win this alone — proven

Everything the daemon can observe about a connection is either forgeable by a
same-user process or describes a genuine hook the agent is riding. The killer:

> **The `fork()`+`execve()` attack.** A process `connect()`s the socket, `fork()`s
> a child that keeps a dup of the socket fd, then `execve`s *itself* into a real
> `python3 …hook.py`. When the daemon reads the peer's identity it sees a genuine
> hook — right path, hash, module tree, flags, interpreter — so **every** peer
> check passes. The forked child (still the agent's code) holds the same
> connection and issues the privileged request. No `ptrace`, no `task_for_pid`, no
> privilege. The PID the daemon measures and the process reading the served bytes
> are not the same thing, and nothing on a `SOCK_STREAM` connection binds them.

Established consequences:

- Any **out-inspect-the-caller** design (ancestor walk, hash entry script, hash the
  whole module tree, check `-E -s`, check start-time env, verify the interpreter —
  the "six-check" design) raises the cost of crude impersonation and then loses to
  fork+execve. It is a dead end *as a boundary*. (Detailed proof of each sub-check's
  weakness is in the appendix.)
- The daemon authenticates the **peer, not the channel, and not the bytes.** The
  only thing that would bind bytes-to-identity is **per-message kernel credentials**
  (Linux `SO_PASSCRED`/`SCM_CREDENTIALS`, or a reuse-proof `SO_PEERPIDFD`; macOS
  `LOCAL_PEERTOKEN` audit token with `pidversion`) re-checked on every request —
  clean on Linux, hard on macOS over a stream socket. This is the *only* daemon-side
  lever that even touches fork+execve; it was not pursued.
- **Socket-redirect via env is NOT a viable attack** (one flank is closed, don't
  worry about it): the agent cannot set a hook's launch environment — settings `env`
  is read once at session start, the agent can't write settings, non-interactive
  `sh -c` hooks don't source shell profiles, and Bash-tool `export` doesn't persist
  to the Claude Code parent. `CLAUDE_PLUGIN_ROOT` is system-provided and
  non-overridable; `SAHJHAN_DAEMON_SOCKET` isn't set in prod.

### 2. Claude Code Bash sandbox — verified against the binary (v2.1.235), the live docs, and a direct `sandbox-exec` test

- **"Sandbox" is two different things.** (a) *Running as a plugin / in a Claude Code
  session* — always true, gated by the permission + hook layer. (b) *The Bash OS
  sandbox* (macOS **Seatbelt**, Linux **bubblewrap** + optional **seccomp**) — a
  separate, **opt-in** setting that OS-confines the agent's Bash commands. Only (b)
  can deny the agent the socket, and it is **off by default** — Claude Code runs
  commands unsandboxed when it's not enabled. This session's Bash reported no
  `CLAUDE_CODE_SANDBOXED`, and holtz enables no sandbox → **holtz audits currently
  run with the agent's Bash unconfined at the OS level**, gated only by holtz's own
  (bypassable) command-inspection hooks.
- **Hooks run *outside* the Bash sandbox; the agent's Bash and its children run
  *inside* it.** (Docs, verbatim rationale for protected paths: a sandboxed command
  must be stopped from adding "a hook or MCP server that Claude Code runs *outside
  the sandbox*.")
- **How a Unix-socket `connect()` is gated — tested with `/usr/bin/sandbox-exec` on
  macOS:**
  - deny `network-outbound`, filesystem allowed → connect **BLOCKED** (`EPERM`).
  - allow network, deny **file** read/write on the socket path → connect **SUCCEEDS**.
  - ⇒ **macOS gates UDS connect at the network layer, not the filesystem.** Control
    is `sandbox.network.allowUnixSockets` — a **path allowlist, default-deny**
    (binary schema: `allowAllUnixSockets` = "If true, allow all Unix sockets —
    *disables blocking on both platforms*"). Filesystem placement/`denyRead` does
    **not** deny a socket on macOS; simply not allowlisting it does.
  - **Linux/WSL2:** `allowUnixSockets` is *ignored* (binary: "seccomp cannot filter
    by path"). Blocking is either the **optional** seccomp filter
    (`npm i -g @anthropic-ai/sandbox-runtime`, a class-block) or **bubblewrap mount
    visibility** (a socket outside the mounted working dir → `ENOENT`). **Not
    locally verified — no Linux machine this session; Linux claims are from the
    binary schema + docs.**
  - **Uniform operational rule** (different layers underneath): *socket lives
    outside the working directory and is never added to `allowUnixSockets`.*
- **Protected paths — the agent can't disable hooks or the sandbox.** The sandbox
  denies writes to `.claude` settings files, `.claude/{hooks,skills,agents,commands}`,
  `.mcp.json`, etc., **with no exemption** ("The only way to turn the protection off
  is `filesystem.disabled`," which is itself **ignored from project settings**). The
  permission system has a matching protected-paths check. So a sandboxed agent can't
  set `disableAllHooks`, edit `hooks.json`, add a hook, or flip the sandbox off.
- **★ The load-bearing limitation — this is why the sandbox approach is a
  non-starter for a plugin:** `sandbox.enabled`, `sandbox.failIfUnavailable`,
  `sandbox.allowUnsandboxedCommands`, and `filesystem.disabled` are honored **only
  from user settings (`~/.claude/settings.json`), managed (system) settings, or the
  `--settings` CLI flag — NOT from project/local settings.** There is **no
  per-project scope**, a **plugin cannot ship them**, and enabling them in user
  settings is **global to every project and session** the user runs (sandboxes and
  can hard-fail *all* their Claude Code usage). Verified in the sandboxing docs and
  the binary.
- **Settings precedence:** Managed > CLI (`--settings`) > Local > Project > User.
  Managed path: macOS `/Library/Application Support/ClaudeCode/managed-settings.json`,
  Linux `/etc/claude-code/managed-settings.json` (root-owned; a same-user agent
  can't write it). `allowManagedHooksOnly` (managed-only) would *disable* plugin
  hooks, so it's not a lever for a plugin.
- **Launch flags matter:** `claude --bare` skips hooks/plugins/settings entirely,
  and `--dangerously-skip-permissions` bypasses checks. The audited agent doesn't
  control its own launch flags, but *whoever launches the session* does — part of
  the trust root.
- **Env facts:** `CLAUDE_CODE_SANDBOXED` (set inside sandboxed commands — but a hook
  runs *outside* the sandbox so it won't see it for the agent), `CLAUDE_CODE_FORCE_SANDBOX`,
  `CLAUDE_CODE_BASH_SANDBOX_SHOW_INDICATOR` exist. `-E` clears only `PYTHON*` env.
  `claude config` did **not** expose the effective merged settings, so a hook that
  wants to check the sandbox config must read/merge the settings files itself.

### 3. Where the socket lives (and why a "managed dir" doesn't protect it)

- **Python** (`_common.py` `_get_daemon_socket_path`): `SAHJHAN_DAEMON_SOCKET` if set
  (test-only today), else **`cwd/docs/holtz/.sahjhan/daemon.sock` — inside the
  working directory.**
- **Rust** daemon: `data_dir/daemon.sock` (`daemon_cmd.rs:63,146`, `mod.rs:95`) —
  also inside the project. **The Rust side does NOT honor `SAHJHAN_DAEMON_SOCKET`;
  only Python does.** They agree today because both resolve to `docs/holtz/.sahjhan/`.
- `docs/holtz` **is** in holtz's `managed = ["docs/holtz","enforcement/"]`, but that
  does **not** protect the socket: the managed-path block is a PreToolUse hook that
  denies **Edit/Write tool calls only** (bypassable; a socket `connect` or a Bash
  `rm` is neither); the *sandbox's* protected paths are `.claude`-specific; and cwd
  is sandbox-writable by default (so a sandboxed `rm`+re-bind = squat).
- macOS `AF_UNIX` path limit is **104 chars** — any moved socket path must be short.

### 4. Consumer (holtz) behavior — especially the fail-open architecture

- **18** `.py` modules in `enforcement/hooks/`; `trusted-callers.toml` lists **11**;
  the **7** unlisted are `_daemon_lifecycle`, `_protocol_cache`, `_resolve`,
  `_sahjhan_bootstrap`, `lens_evidence`, `quiz_vault`, `verify_hooks`. `quiz_vault`
  and `_protocol_cache` are imported by privileged hooks; `_sahjhan_bootstrap` is
  itself a privileged entry hook whose `enforcement_read` the daemon **rejects
  today** (unlisted → `NotInManifest`), silently degrading its gates.
- **All privileged calls are socket-direct** from the Python hook via `_common.py`
  `_daemon_request` (`socket.socket(AF_UNIX)`): `sign`, `record_event`,
  `vault_store/read`, `enforcement_read/write/update`. None CLI-mediated; the old
  `sahjhan authed-event` courier is already dead (bare binary → no script under
  plugin root → rejected), which is *why* the consumer uses `record_event`.
- **The daemon is launched by a hook** (`_daemon_lifecycle.py`), not by agent Bash.
  (Also relevant: a sandboxed agent denied the socket couldn't *bind* it either — so
  hook-launch is required if the socket is ever denied to the agent.)
- **Hooks launch bare** `python3 "…hook.py"` — **15** launch lines in
  `hooks/hooks.json`, no `-E -s`.
- **★ The consumer fails OPEN whenever the daemon can't serve — no daemon-only
  fail-closed is possible without a consumer change:**
  - `read_cache` (`_protocol_cache.py:54-75`) catches **every** daemon error incl.
    `RuntimeError` (a refusal) → returns `None` (docstring: "fail-open").
  - `is_enforcement_fresh(None)` → `False` → `pre_tool_hook` (lines 91-93) →
    `exit_ok` (**allow**).
  - `exit_enforcement_error` (`_common.py:129`) is *also* fail-open — it re-reads
    the cache and only blocks when `is_enforcement_fresh` is true.
  - **To fail closed you must call `exit_block` directly** on the fatal reason.
  - Asymmetry to know: `write_cache`/`update_cache` (`_protocol_cache.py`) **raise**
    `RuntimeError` on daemon error (they don't swallow) — unlike `read_cache`.
- Enforcement is **active only when `last_sahjhan_cmd` is fresh** (30-min window,
  `_ENFORCEMENT_FRESHNESS_MINUTES`) — opt-in per session, stamped by
  `protocol_tracker` when a sahjhan command runs. `pre_tool_hook` only fires on
  `Write|Edit`; Bash is gated by `_sahjhan_bootstrap`/`commit_gate`/`bash_guard`.

### 5. Other vulnerabilities, independent of the socket boundary

These the red-team surfaced; they are *not* solved by any socket/sandbox fix and a
fresh solver must weigh them separately.

- **Confused deputy — the quiz bank can be poisoned without reading it.**
  `quiz_capture.py` (a genuine, authenticated PostToolUse/Bash hook) parses
  attacker-controlled **Bash stdout** for `QUIZ-QUESTION:` markers and writes them
  to the vault (`vault_store`). The agent can seed the quiz bank with questions it
  already knows the answers to. A trusted hook forwarding attacker-chosen input to a
  privileged op is indistinguishable, to the daemon, from the agent doing it.
- **Single-connection DoS → fail-open.** The daemon is single-threaded with **no
  read timeout**; a client that connects and never sends a line wedges it for the
  rest of the run. The consumer then fails open (no quiz, gates pass). Availability
  of this daemon is an integrity property, not a nicety.
- **Empty/absent manifest = allow-all, and the daemon is agent-launchable.**
  `callers.is_empty()` ⇒ authenticate everyone (`mod.rs:309-310`), and the manifest
  is loaded at startup without a seal check on that path. Recon prescribes the agent
  running `nohup sahjhan … daemon start`. So "who starts the daemon, with which
  `trusted-callers.toml` and `--config-dir`" is part of the trust root, currently
  unguarded. Plus **socket squat**: predictable in-project path, `0600` excludes
  other users not the same one, so the agent can pre-bind the path or pre-write the
  PID file (daemon refuses to start when `kill(pid,0)` succeeds).
- **Binary-resolution trust (`_resolve.py`).** A present `sahjhan` binary with no
  `.sahjhan-version` file is treated as trusted; bootstrap writes a plugin-relative
  `bin/sahjhan` symlink. If the agent can write the plugin `bin/`, it can plant a
  `sahjhan` the hooks `exec` for CLI queries — in-hook code exec on the non-socket
  paths. A stepping stone, not a direct socket break.

### 6. sahjhan code anchors

- Socket resolution: `resolve_socket_path` `daemon_cmd.rs:142` (socket at :146),
  daemon-start socket `:63`; `DaemonServer::new` `mod.rs:89` (socket at :95); daemon
  PID at `data_dir/daemon.pid`.
- Auth: `handle_connection` `mod.rs:294`; auth block `:309-320` — **empty manifest ⇒
  allow-all at `:309-310`**; `authenticate_peer` + `find_trusted_ancestor`
  `auth.rs:126-231` (the 10-deep walk); `plugin_root` passed to auth is
  `&self.config_dir` (`mod.rs:224`) so the real plugin root is `config_dir.parent()`.
  Single-threaded blocking accept loop (`start`, `mod.rs:168-261`).
- Dispatch: `handle_request` `mod.rs:390` (takes `config_dir`); short-circuit point
  is the top of its body. `Status` is auth-exempt (`mod.rs:343`).
- Wire: `Response::err_with_reason(error, message, reason)` `protocol.rs:211` →
  `{ok:false, error, message, reason}`; Python `_daemon_request` maps
  `error`→`DaemonError.error`, `reason`→`DaemonError.reason`.
- Layout: `config_dir` = `$CLAUDE_PLUGIN_ROOT/enforcement`; `hooks.json` at
  `config_dir.parent()/hooks/hooks.json`; `trusted-callers.toml` at
  `config_dir/trusted-callers.toml` (a sealed config file).

---

## Approaches tried, and why each died (inputs, not the focus)

1. **Daemon out-inspects the caller ("six checks": peer-only, whole-module hash,
   `-E -s`, clean start-time env, real-interpreter).** Dead — fork+execve (§1)
   defeats all of it; every sub-check is also independently weak (appendix).
2. **Move the secret out of the run** (separate user / remote scorer). Dead —
   violates the standalone-plugin, secret-stays-in-run constraint.
3. **Make the Bash OS sandbox the boundary** (agent confined + socket denied; daemon
   as defense-in-depth + a fuse that refuses when the sandbox isn't locked, with a
   small consumer change so `pre_tool_hook` fails closed). Dead as a drop-in plugin:
   the sandbox locks are **user/managed-only, never project** (§2 ★), so enabling
   them is **global** to all the user's Claude Code work, a **plugin can't set
   them**, and without them the fuse *blocks the agent from editing during the
   audit*. Choices collapse to "globally lock down all Claude Code usage" or "the
   audit can't run." Non-starter for one-project, one-run.

## What's independently useful (survives regardless of the boundary question)

- **Rust honors `SAHJHAN_DAEMON_SOCKET`** — one helper (`daemon_socket_path(data_dir)`
  = env override else `data_dir/daemon.sock`) used at the three socket sites
  (`mod.rs:95`, `daemon_cmd.rs:63,146`); leaves `daemon.pid` in place. Lets the
  socket move out of cwd via config, matching Python. Harmless cleanup.
- **Direct-peer auth replacing the ancestor walk** — require the peer's script to
  canonicalize under `config_dir.parent()`. Drops the fork/execve-via-*ancestor*
  surface (not the direct fork+execve), *and* fixes the present-day bug where
  unlisted genuine hooks (e.g. `_sahjhan_bootstrap.py`) are rejected, *and* makes the
  empty-manifest allow-all fail closed.

## Open lead, and the real open question

- **Lead — `--settings` is per-invocation, not global.** The sandbox locks are also
  honored from `claude --settings <file>`. So: can a holtz audit be launched via a
  plugin-provided wrapper/command that starts Claude Code with a sandbox config for
  **that session only**, leaving `~/.claude/settings.json` untouched? Verify, in
  order: (1) does `--settings` actually enable the Bash sandbox for one session and
  deny the socket (test via `sandbox-exec` / `CLAUDE_CODE_SANDBOXED`)? (2) **can a
  plugin drive that launch at all**, or is it at best a one-liner the user must run
  to start an audit — the crux, since plugins don't normally control how `claude`
  starts? (3) `failIfUnavailable`/`allowUnsandboxedCommands` would then degrade only
  the audit session (acceptable — the audit *should* be sandboxed), not other work.
- **If the lead fails:** confront the honest question — can a same-user secret be
  protected *at all* by a drop-in, single-project plugin, or is the truthful
  deliverable "not fully," a clear statement of residual risk, and what enforcement
  it does/doesn't buy without a real OS boundary? Also reconcile with whatever
  `docs/archive/2026-08-19-trusted-caller-postmortem.md` concluded about the
  reverted first attempt.

---

## Appendix — daemon-side peer-identity findings (why out-inspection is a dead end, in detail)

Kept so no one re-derives it. All of this is *why* approach #1 fails beyond the
headline fork+execve.

**Kernel / syscall (macOS + Linux):**
- `SO_PEERCRED` (Linux) / `LOCAL_PEERPID` (macOS) freeze the peer **PID at connect**;
  exe/cmdline/env/module reads are **later, separate, non-atomic** syscalls against
  that PID (TOCTOU). Auth runs **once per connection** (`mod.rs:312`), before the
  request loop, so one authenticated connection serves everything after.
- **fds survive `execve` only if not CLOEXEC.** Rust/Python sockets default to
  close-on-exec, so the fork+execve attack keeps the fd in the *forked child* (which
  doesn't exec) — that's why it's fork **and** exec, not exec alone.
- **Reading a genuine hook's memory/fd** (the "attack the real hook" path): on
  **Linux** governed by Yama `ptrace_scope` (0 = any same-uid can attach and
  `pidfd_getfd` the socket; default 1 = parent-only, but hooks run in the agent's
  process tree so the agent is often the parent). On **stock macOS** SIP/taskgated
  **deny** `task_for_pid` to a same-uid, unentitled, non-root caller, so this is
  *mostly not available* — the realistic macOS path is the agent launching the
  interpreter under its own control. The daemon calls `deny_debug_attach` on
  *itself* only; hooks are unprotected.
- **Reuse-proof primitives we don't use:** Linux `SO_PEERPIDFD` (pidfd), per-message
  `SCM_CREDENTIALS`; macOS `LOCAL_PEERTOKEN` (audit token carrying `pidversion`).
  These are the only things that could catch fork+execve (per-message re-check).
- **`KERN_PROCARGS2` argv/env is user-mutable stack memory**, not kernel-authoritative
  (a process can rewrite its own argv/env); only `proc_pidpath`/`/proc/pid/exe` is an
  authoritative *path* (still not authoritative *bytes* on macOS, which has no
  fd-to-exe; Linux `/proc/pid/exe` pins the inode). The envp→`apple[]` boundary in
  `KERN_PROCARGS2` has no delimiter and both look like `key=value`, so the process
  environment can't be cleanly read from it at all.
- `extract_script_path` (`auth.rs:110`) returns the first non-`-` arg, so
  `python3 -W ignore hook.py` reads `ignore` as the script — too loose for any
  strict flag/entry-mode check (fine for the real bare `python3 hook.py` launches).

**CPython import surface (why "verify the module tree" can't establish "only the
hook's code ran"):** under `python3 -E -s hook.py` the script's own directory stays
first on `sys.path` (that's what lets hooks import siblings; `-I` would drop it and
break them). That directory is agent-writable in an in-tree layout, and Python loads
code from far more than `.py`: **sourceless `.pyc`** (no `.py`), **extension modules**
(`foo.cpython-3xx.so`) and **package `__init__.py` dirs** both *outrank* a same-named
`.py`, and a **`__pycache__/*.pyc` with a replayed mtime header** runs while the `.py`
hashes clean (`-B` stops *writing*, not *reading*, the cache). **`.pth` and
`sitecustomize.py` in a site dir execute at startup under `-s`** (only `-S` stops
them) — and holtz's `.venv` already ships an executing `a1_coverage.pth` in
agent-writable site-packages. A connect-time **disk** scan also can't see what a
process imported earlier and then restored. And hashing the tree misses code the
hooks `exec` from *outside* it (the plugin-root `hooks/_common.py` via
`importlib.util.spec_from_file_location`).

**Interpreter binding was also unsolved:** "require the real `python3` under an
unwritable dir" is **false on this machine** — Apple-Silicon Homebrew's
`/opt/homebrew` (its `bin/python3` *and* its `site-packages`, where `.pth`/`sitecustomize`
run) is owned by the run's uid.

---

## Environment & how to re-verify (so you can trust-but-check)

- Verified on: **Claude Code v2.1.235** (native binary at
  `~/.local/share/claude/versions/2.1.235`), **macOS** (Darwin 25.5), Apple Silicon,
  single-user Homebrew. **Linux behavior is inferred from the binary + docs, not run.**
- **Sandbox config keys** were confirmed by grepping the binary's embedded schema
  strings (e.g. `grep -a -o allowUnixSockets <binary>`), plus the live docs at
  `https://code.claude.com/docs/en/sandboxing` and `.../settings`.
- **macOS UDS gating** was proved with `/usr/bin/sandbox-exec`: run a python client
  under `sandbox-exec -p '(version 1)(allow default)(deny network-outbound)'`
  connecting to a listener socket → `EPERM`; under a profile that instead denies
  `file-read*`/`file-write*` on the socket path → connects fine. Reproduce before
  trusting the macOS "network layer, not filesystem" claim, and build the Linux
  equivalent before trusting the Linux claims.
