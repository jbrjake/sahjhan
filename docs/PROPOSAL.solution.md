# A solution — make the OS sandbox the boundary, and make the daemon refuse to run without it

Draft for review. Not committed. This is my proposed answer to `PROPOSAL.revised.md`;
it does not modify that note or any of your prose. **No code has been written and none
will be until you pick a direction.**

---

## The answer in one sentence

Stop asking the daemon "who is on the socket" — it provably cannot tell — and instead
put the agent's Bash inside the Claude Code OS sandbox with the socket denied, so the
only processes that can reach the daemon are the hooks Claude Code runs *outside* the
sandbox. The daemon's new job is not to authenticate callers but to **refuse to serve
unless it can confirm that boundary is in place.**

The one fact that unlocks this — and that the prior handoff got wrong — is that the
sandbox can be turned on for **one project**, not just globally. You remembered that
correctly.

---

## Why the daemon can't be the boundary (established, not re-litigated)

The prior sessions proved this and I'm not going to re-derive it. Every property the
daemon can read about a connection is either forgeable by a same-user process or
describes a genuine hook the agent is riding:

- **fork()+execve():** the agent `connect()`s, forks a child holding a dup of the socket
  fd, and the child `execve`s itself into a real `python3 hook.py`. The daemon measures a
  perfect hook; the bytes are read by the agent's forked child. Nothing on a stream
  socket binds the measured PID to the process reading the served bytes.
- **In-process injection:** a genuine hook, invoked normally, loads agent code —
  `json.py` beside the hook (verified), a `.pth`/`sitecustomize.py` in the venv, a
  `__pycache__` replay. Path+hash of the entry script is clean; the process is not.

Consequence: the ancestor walk, and every "inspect the caller harder" variant (the
six-check design in `PROPOSAL.md`), raise the cost of crude impersonation and then lose.
They are a Maginot line. `enforcement/vault.toml` even documents its temporal quiz
protection as *"defence in depth on top of the trusted-caller auth that already blocks
the agent"* — but that auth **does not** block the agent. That sentence is the load-bearing
assumption of the whole system, and it is false today.

The corollary is the uncomfortable part: **a same-user secret cannot be protected by any
amount of daemon cleverness.** If the checker and the checked run in the same trust domain
with no OS line between them, the checked can always reach the checker. So the fix has to
introduce exactly one OS-level line. There is only one available to a drop-in, in-run
plugin.

---

## The boundary that already exists: the Bash sandbox

Claude Code's Bash sandbox (macOS Seatbelt, Linux bubblewrap + optional seccomp) draws a
line in precisely the right place:

- The **agent's Bash commands and their children run inside** the sandbox.
- **Hooks run outside** it — the docs say so explicitly, giving the reason that a
  sandboxed command must not be able to "add a hook … that Claude Code runs *outside the
  sandbox*" (sandboxing docs, "Protected paths").

That is the holtz trust line, exactly. The quiz answers, the HMAC key, and the enforcement
state must be reachable by hooks and unreachable by the agent. If the agent's Bash is
sandboxed with the daemon socket denied, then:

- The agent cannot `connect()` the socket. The `HANDOFF.md` reproduction (run the trusted
  hook with a poisoned `PATH`) dies here: running `python3 hooks/lens_quiz.py` from the
  agent's Bash is a *sandboxed* command, socket denied. Riding a hook only works when
  Claude Code invokes it as a hook — outside the sandbox — which the agent can't cause.
- fork()+execve() gains nothing: the forked child is still in the sandbox.
- The agent can't disable the protection: protected paths deny writes to `.claude`
  settings and `.claude/hooks`, "there is no way to exempt one of these paths," and the
  one key that lifts the protection (`filesystem.disabled`) is ignored from project
  settings.

The daemon's own auth becomes almost irrelevant once this holds — the only things that
reach the socket are genuine hooks. That is the whole point: move the security off the
lever that can't bear it and onto the one that can.

---

## The correction: this is per-project, not global

The prior handoff killed this approach on the belief that the sandbox can only be enabled
in user/managed settings, so enabling it is "global to all the user's Claude Code work"
and "a plugin can't set it." That is not what the current binary and docs say.

Verified first-hand against the live docs (`code.claude.com/docs/en/sandboxing`) and the
installed 2.1.235 binary:

1. **Per-project enablement is a first-class, supported flow.** The `/sandbox` panel saves
   the mode to `.claude/settings.local.json`, which "apply to the current project" (docs).
   Enabling it project-wide does not touch `~/.claude/settings.json`. Your memory was right.

2. **Scope rules are asymmetric by design — any scope may *strengthen*, only
   user/managed/`--settings` may *weaken*.** A project settings file can turn the sandbox
   on and *add* `deny` rules, because "a `deny` entry only ever narrows access, so any
   scope can add one, but no scope can remove one that another scope added." Conversely
   `filesystem.disabled`, `strictAllowlist`, `allowAppleEvents`, credential masking, etc.
   are "ignored" from a repo's settings — a checked-out project can't switch isolation off.

3. **The protected-paths lock makes a project-scoped sandbox agent-proof.** Once the
   sandbox is on with filesystem isolation on, the sandboxed agent cannot rewrite
   `.claude/settings.json` / `.claude/settings.local.json` / `.claude/hooks` to weaken it,
   and cannot lift that protection from project scope.

The binary also carries a settings *scanner* ("operator-posture override") that flags
`sandbox.enabled`, `disableAllHooks`, `sandbox.allowUnsandboxedCommands`, and
`sandbox.failIfUnavailable` when they appear in a **committed** repo settings file, on the
reasoning that "projectSettings overrides userSettings in the merged read" so a repo value
could *negate the operator's* posture. For our use (strengthening) it is functionally
fine, but the clean home for the enable+deny config is `.claude/settings.local.json`
(project-local, auto-gitignored), not a committed `.claude/settings.json`.

So the delivery is not "a plugin silently sandboxes itself" (impossible, and pretending
otherwise is the trap the last attempt fell into) and not "globally lock down all your
Claude Code work" (what the handoff feared). It is: **a per-project sandbox config the
installer applies once** — squarely inside the stated constraint that "whoever installs
the plugin is trusted to set it up once; the agent must not be able to undo it."

---

## What must be true — the sandbox config

The audited session must run with, at project scope (`.claude/settings.local.json`):

- `sandbox.enabled: true`.
- The daemon socket **not** in the Unix-socket allowlist (default-deny). Combined with
  moving the socket out of the working directory (below), the agent's Bash cannot reach it.
- `denyWrite` for the plugin install tree and its `.venv`/site-packages, so the sandboxed
  agent can't plant an injection file (`sitecustomize.py`, a sibling `json.py`) that a
  genuine hook would later load. Deny entries are allowed from project scope.

And, for a **hard** guarantee, in the same project-local file (independent verification
corrected my earlier claim here — these two are *not* operator-scope-only; they follow
normal precedence and are honored from `.claude/settings.local.json`):

- `sandbox.failIfUnavailable: true` — the session refuses to start rather than silently
  running unsandboxed if the sandbox can't initialize.
- `sandbox.allowUnsandboxedCommands: false` — the agent can't retry a denied socket
  connection outside the sandbox.

So the enable, the confinement, *and* the two fail-closed locks can all live in one
project-local file the installer drops. The only sandbox key project scope genuinely
*cannot* set is `filesystem.disabled` — and that restriction runs in our favour, because
it means a checked-out project can never switch filesystem isolation *off*. The one thing
to keep in mind is ordinary precedence: managed settings, if an org deploys them, win over
everything, and a `--settings` launch file overrides the project-local one for that
session. The daemon fuse (below) is still worth having as a backstop for the case where
the sandbox silently failed to start on a machine that lacks `failIfUnavailable`.

---

## The changes

> **Settled 2026-08-20:** Jon accepted the daemon fuse (D1), the consumer failing closed
> (D2), and dropping the ancestor walk (D3). These were always the three components of the
> "sandbox is the boundary" approach in `PROPOSAL.revised.md`; that note shelved them only
> because it believed a per-project sandbox was impossible, which was the mistaken part.
> They are the plan, not options. Delivery is **Model A** (launch-scoped) — see below.

### Group 1 — the boundary mechanics

Load-bearing. Without them the sandbox either isn't the boundary or breaks the audit.

- **C1 — move the socket out of the working directory.** Today Python resolves
  `cwd/docs/holtz/.sahjhan/daemon.sock` and the Rust daemon binds `data_dir/daemon.sock`;
  Python already honors `SAHJHAN_DAEMON_SOCKET`, Rust does not. Teach the Rust side the
  same override (one helper at the three bind/connect sites) and point both at a directory
  outside cwd that the sandbox config marks `denyWrite`. That makes the socket both
  un-connectable and un-squattable by the sandboxed agent (it can't write the directory to
  pre-bind the path). Note: on Linux this relocation does *not* substitute for the seccomp
  filter — there is no bubblewrap mount-invisibility fallback for sockets (see risks); the
  block still comes from seccomp. The relocation's value is squat-prevention and cleanliness.

- **C2 — the launcher starts the daemon, in the operator's trusted shell, before the
  sandboxed session begins.** `phase-recon.md` has the **agent** run
  `nohup sahjhan … daemon start`; a sandboxed agent can't bind the socket, so it has to
  move. Under Model A the natural home is the launch command itself (below): it runs
  `init`, starts the daemon backgrounded with the socket at the C1 location, does the
  `daemon-init-pid` copy `_daemon_lifecycle.py` depends on, then `exec`s the sandboxed
  `claude`. The daemon then inherits the *human's* environment, not the agent's — which
  also fixes the pre-existing trust problem the postmortem flagged.

  Note: the agent's ordinary recon commands are unaffected. I checked — `transition`,
  `event`, `status`, `render`, `ledger`, `init`, `log`, `query` touch only files (the
  ledger lives under cwd, which stays sandbox-writable). Only `sign`, `vault`, `verify`,
  and `daemon` open the socket, and the agent never legitimately runs those.

### Group 2 — the enforcement layer (accepted)

The three components Jon confirmed. Their role is defense-in-depth and graceful
degradation on top of the boundary; the sandbox is still what keeps the agent out.

- **D1 — the daemon fuse.** On startup (and cheaply per privileged request) the daemon
  reads the effective sandbox config and confirms the session is confining and the socket
  is excluded. If not, it refuses all privileged ops with a clear reason (fail closed).
  This converts "hopefully sandboxed" into "provably sandboxed, or nothing works," and it
  recovers the fail-closed guarantee even when only project-scope config is present. The
  fuse is coherent precisely because in the sandboxed world the agent can't have tampered
  with that config, and in the un-sandboxed world refusing is the safe choice. (The binary
  appears to expose native `sandbox_unavailable` / `sandbox_unconfined` refusal machinery;
  worth checking whether a hook can lean on that instead of the daemon re-deriving it.)

- **D2 — the consumer fails closed.** The *dead-daemon* case is already handled:
  `_daemon_lifecycle.py` writes a `terminated` marker and blocks the write path when the
  daemon dies. The gap is the *live-but-refusing* case — `read_cache` (`_protocol_cache.py`)
  swallows every daemon error and returns `None`, and `pre_tool_hook` then allows
  (fail-open). Extend the existing terminated-marker posture so that during an *active*
  audit a daemon *refusal* (the fuse tripping) blocks rather than waves through, while "no
  audit active" still passes. Small, and it's the difference between the fuse mattering and
  not.

- **D3 — retire the ancestor walk for direct-peer auth.** Authenticate only the process
  directly holding the socket; require its script to canonicalize under `--config-dir`
  with a matching hash. This is *not* the boundary (the sandbox is) and I won't claim it
  as one — but it removes the reported inherited-authority bug, fixes the empty-manifest
  allow-all (currently authenticates everyone), and fixes genuine hooks that are rejected
  today because they're unlisted (`_sahjhan_bootstrap.py`). It preserves the one property
  that already holds: a proof replayed from plain Bash still fails.

---

## What each attack becomes under this design

| Attack | Today | Under the sandboxed design |
|---|---|---|
| Run a trusted hook with poisoned `PATH` (`HANDOFF.md` repro) | Reads vault / forges events | Sandboxed command, socket denied — fails |
| fork()+execve() into a real hook | Defeats every peer check | Forked child still in the sandbox — fails |
| Inject `sitecustomize.py` / sibling module into a genuine hook | In-process code exec | Plugin tree + venv are `denyWrite` — can't plant it |
| ptrace / steal fd of a live connected hook | Open question 2 in `PROPOSAL.md` | Closed while sandboxed — macOS Seatbelt profile denies `task_for_pid` against any process outside the sandbox; Linux runs the agent in its own PID namespace, so the hook isn't even visible to attach to |
| Squat the socket path / pre-bind | Predictable in-cwd path | Socket moved out of cwd; agent can't write there or connect |
| Empty `trusted-callers.toml` ⇒ allow-all | Authenticates everyone | D3 makes it fail closed |

---

## Residual risks — stated here, not in a footnote

1. **Linux socket denial needs the optional seccomp filter — and there is no fallback.**
   Independent verification sharpened this: on Linux/WSL2 the *only* thing that blocks the
   agent from opening a Unix socket is the optional `@anthropic-ai/sandbox-runtime` seccomp
   filter (it blocks `socket(AF_UNIX, …)` outright). Without it, sockets are not blocked at
   all — and, correcting my earlier note, moving the socket out of cwd does *not* substitute
   (there's no mount-invisibility fallback for sockets). macOS (Seatbelt) blocks sockets by
   default. So on Linux the socket boundary is exactly as strong as that filter being
   installed — the daemon fuse (D1) should verify it's present, and the operator must
   install it. This is the sharpest edge. (The separate ptrace concern is *not* on this
   filter: Linux protects against reaching into the hook via the sandbox's PID namespace,
   macOS via the Seatbelt profile — both hold whenever the sandbox is active.)

2. **Nothing inside the plugin can guarantee the sandbox is on — it's an install/launch
   step.** Verified: a Claude Code plugin's own `settings.json` supports only the `agent`
   and `subagentStatusLine` keys, so a plugin *cannot* ship `sandbox.*` at all. Enablement
   has to come from the `/sandbox` panel (writes project-local), a settings file the
   installer drops, managed settings, or the `--settings` launch. If none of that happened,
   the audit would run unsandboxed. `failIfUnavailable: true` turns a silent sandbox failure
   into a hard stop, and it *is* settable project-local (correcting my earlier claim) — but
   it only helps if someone set it. The daemon fuse (D1) is the backstop that refuses to
   serve when the boundary isn't actually there.

3. **The single-threaded daemon is a DoS/availability risk, and availability is integrity
   here.** One client holding a connection open wedges it (no read timeout). Under D2
   (fail closed) a wedged daemon safely blocks the audit rather than failing open, but it's
   still worth a read timeout on the daemon — cheap, independent of everything else.

4. **Quiz-bank poisoning is orthogonal and unsolved by this.** `quiz_capture.py` parses
   agent-controlled Bash stdout for `QUIZ-QUESTION:` markers and stages them to the vault.
   The sandbox doesn't touch this — a trusted hook forwarding attacker-chosen input to a
   privileged op looks identical to the hook doing its job. It needs its own fix
   (provenance on staged questions, or don't let the agent's channel seed the bank). I flag
   it so it isn't mistaken as covered.

5. **Initial-state trust.** The plugin tree and venv must be trustworthy at session start
   and kept read-only by the sandbox thereafter; the config seal already covers the sealed
   config files. If the agent had a prior *unsandboxed* session in the same checkout, it
   could have pre-planted an injection file — so the audit must be sandboxed from t=0, and
   the tree verified going in.

6. **The launch/setup is part of the trust root.** Someone trusted enables the sandbox and
   (for the strong version) sets the two operator-scope locks. The agent must not control
   its own launch flags. This is the same class of assumption as "who installs the plugin"
   and "who starts the daemon" — not a new trust dependency, but it must be named.

---

## Verification status

Verified first-hand this session (docs + installed 2.1.235 binary + holtz on disk):

- Per-project sandbox enablement; strengthen-any-scope / weaken-operator-only rule;
  protected-paths lock and its non-exemptibility; `filesystem.disabled` ignored from
  project scope; Linux UDS/ptrace depending on the optional seccomp filter.
- holtz topology: hooks launch bare `python3`; the agent starts the daemon per
  `phase-recon.md`; `read_cache` fails open while `_daemon_lifecycle` fails closed; which
  `sahjhan` subcommands touch the socket; `SAHJHAN_DAEMON_SOCKET` honored by Python not Rust;
  `vault.toml`'s temporal protection resting on the broken auth.

A second, independent agent then re-verified all of the above against the same binary and
docs. It corroborated the core and corrected three of my claims, now folded in above:
(a) `failIfUnavailable` / `allowUnsandboxedCommands` are *not* operator-scope-only — they're
honored project-local; (b) on Linux there is no mount-invisibility fallback for sockets, so
the seccomp filter is strictly required for the socket block; (c) the ptrace/steal-fd
concern is closed on both platforms while sandboxed (macOS profile denies `task_for_pid`
outside the sandbox; Linux isolates the agent in its own PID namespace). It also confirmed
a plugin cannot ship `sandbox.*` keys.

Not yet done, and I would want done **before** any code:

- An end-to-end run of a real `claude` session under this project sandbox config, on this
  machine, confirming empirically: (a) the agent's Bash is denied the socket, (b) a hook
  Claude Code invokes still reaches it, (c) the sandboxed agent cannot write
  `.claude/settings.local.json`. This is the exact fact the last attempt asserted instead
  of testing; I won't repeat that — two agents reading the same docs is still just reading.

---

## Turning the sandbox off — the audit lifecycle

The sandbox only needs to be on *during* an audit, so "how do I get my project back when
holtz converges" is a first-class question, not an afterthought. One rule shapes the whole
answer:

**Turning the sandbox off is always a human action, and there is deliberately no
agent-reachable off switch.** "Disable the sandbox" is the single thing the adversary most
wants, so — importantly — we must **not** wire "the sandbox turns off automatically when the
audit reaches `finalized`," because the agent can influence whether the ledger gets there.
Reaching the converged state must change nothing about the sandbox on its own. Convergence
is a signal to a *human*, not a trigger.

Given that, what "off" means depends on which delivery model we choose — and this is the
main fork I need you to pick:

- **Model A — scope the sandbox to the audit's launch.** Audits start with a specific
  launch (a `claude --settings holtz-audit.json` wrapper, or a documented one-liner). Only
  sessions started that way are sandboxed. Here there is *nothing to turn off*: when holtz
  converges you just start your next session normally and it isn't sandboxed; your everyday
  work in the project was never touched. Cost: the audit has to be launched the specific
  way — the human does that, the agent can't. I lean toward this precisely because it makes
  your "give me my project back" concern vanish: "off" is the resting state.

- **Model B — a persistent per-project setting** (`.claude/settings.local.json`, the
  drop-in option). More convenient to start (no special launch), but it sandboxes *every*
  session in that project until someone removes it. Turning it off is a human editing that
  one file — delete it, set `enabled: false`, or open `/sandbox` and pick a non-sandboxed
  mode — none of which the agent-under-audit can do (settings files are protected from
  inside the sandbox; a human at a normal shell isn't). Costs: while it's on it gets in the
  way of ordinary work in that repo (sandboxed `git merge`, `docker`, some CLIs misbehave),
  and you have to *remember* to remove it — convergence won't.

**One gotcha for either model:** the sandbox is a *Claude Code* setting, not part of the
holtz plugin. Disabling or uninstalling holtz with `/plugin` stops the hooks but does **not**
remove a persistent sandbox setting — that file has to be cleared on its own. It's easy to
assume "remove the plugin, remove the sandbox," and that isn't true.

My recommendation: **Model A**, so "off" is automatic at convergence and normal sessions are
never affected. If you want Model B's drop-in feel, we pair it with a single human-run
teardown step and go in eyes-open that it sandboxes all sessions in that project until run.

**Chosen 2026-08-20: Model A** — with the two requirements below.

## Model A′ — in-session `!holtz-start` / `!holtz-stop` (Jon's proposal, likely the winner)

Jon proposed a cleaner shape than an out-of-session launcher: a `bin/holtz-start` and
`bin/holtz-stop` pair, run in a *normal* session via the `!` bang prefix, that flip the
sandbox on and off by writing a watched settings file and letting Claude Code reload it live
— no relaunch, no PATH setup, symmetric start/stop.

**Why this is now believed feasible (correcting my earlier "can't enable mid-session"):**
the settings doc says only `model` and `outputStyle` are read once at start; "edits to most
keys apply to the running session without a restart" through the settings watcher (which also
fires a `ConfigChange` hook). The Bash sandbox is applied per-command, so a settings write
that turns it on should confine the *next* Bash command. This reverses what I claimed two
messages ago — flagged as doc-evidence, not yet a live test.

**The flow:**
- You run `!holtz-start`. It's unsandboxed at that instant (it's what turns the sandbox on),
  so it can `sahjhan init`, start the daemon with the socket at the C1 location, then write
  the sandbox config to `.claude/settings.local.json`. Claude Code reloads → subsequent Bash
  is sandboxed. The daemon is started by *your* action, not the agent's — which also keeps
  the daemon's environment and socket path out of the agent's control (a real trust win over
  the agent, or a self-arming skill, starting it).
- The audit runs sandboxed. The agent's Bash can't reach the socket, and can't rewrite the
  now-protected `.claude/settings.local.json` to turn the sandbox off. It can't issue `!`
  commands. If it invokes `holtz-stop` through the Bash tool, that write is denied too — it
  fails safe.
- At convergence, the skill reminds you: `to stop holtz, run !holtz-stop`, which removes the
  config, reloads → unsandboxed, and stops the daemon.

**VALIDATED live 2026-08-20 (macOS, Claude Code 2.1.235), by Jon:**
1. **Mid-session enable works.** With a listener on an unlisted socket, a Bash-tool probe
   returned `CONNECTED`; after `arm.sh` wrote `.claude/settings.local.json` =
   `{"sandbox":{"enabled":true,"allowUnsandboxedCommands":false}}`, the *next* Bash-tool
   probe returned `PermissionError: [Errno 1] Operation not permitted`. The settings watcher
   applied the change with no restart. This is the load-bearing fact, and it holds.
2. **Once armed, nothing inside the session escapes.** A Bash append to
   `.claude/settings.local.json` was denied (`Operation not permitted`) — the sandbox
   protects its own settings file, so the agent can't disarm. And a `!` bang probe was
   *also* denied — bang commands are sandboxed at the session level, exactly like tool-Bash.

**Delivery, VALIDATED end-to-end 2026-08-20: `holtz-start` / `holtz-stop` as typed words.**
Both arm and disarm are done by a `UserPromptSubmit` hook (holtz already runs one,
`primer.py`) — hooks run *outside* the sandbox, so the hook can write the protected settings
file in either direction while the session is sandboxed. The human just types the bare word
in the normal input; the agent never touches sandbox settings. Live result on macOS / CC
2.1.235:

- probe `CONNECTED` (baseline) → type `holtz-start` → probe `DENIED` → type `holtz-stop` →
  probe `CONNECTED`. The disarm dropped a *live* sandbox — Claude Code re-reads the settings
  per command, so both enable and disable apply mid-session with no restart.

Why this sidesteps the earlier mess: the auto-mode classifier *correctly* blocks the **agent**
from editing its own sandbox settings — so arming/disarming must not be an agent tool call.
It isn't: the human types the word, and the hook (not classifier-gated) does the write. The
trigger is **exact match** on the whole message, so pasted text or "how do I holtz-stop?"
can't fire it, and only a human can (the agent can't submit prompts). This also retires the
whole "separate terminal / `/sandbox` panel / bang" problem — none of them are needed.

In production `holtz-start` additionally starts the daemon (unsandboxed, before it writes the
settings) so the socket is bound outside the sandbox before the agent's Bash is confined —
the one piece beyond the toggle-only scratch test, and straightforward. The daemon fuse (D1)
+ consumer fail-closed (D2) + a `/holtz` guardrail (a `CLAUDE_CODE_SANDBOXED` probe) cover the
case where someone forgets to type `holtz-start`: the audit degrades closed and is told to arm.

**One caveat to accept:** the settings file persists, so between start and stop *every*
session in that project is sandboxed (the Model B window). For a bounded audit that's fine,
and `holtz-stop` + the convergence reminder are the teardown.

**The minimal test to settle it** (this is the point to stop reading docs):

```
mkdir -p /tmp/sbtest/.claude && cd /tmp/sbtest
# 1. start a throwaway unix-socket listener OUTSIDE the project:
python3 -c 'import socket,os; p="/tmp/sb.sock"; os.path.exists(p) and os.remove(p); s=socket.socket(socket.AF_UNIX); s.bind(p); s.listen(); __import__("time").sleep(600)' &
# 2. open a normal (unsandboxed) claude session in /tmp/sbtest, and in it:
#    a. probe the socket via Bash — expect CONNECTED:
#         python3 -c 'import socket; s=socket.socket(socket.AF_UNIX); s.connect("/tmp/sb.sock"); print("CONNECTED")'
#    b. write .claude/settings.local.json = {"sandbox":{"enabled":true,"allowUnsandboxedCommands":false}}
#    c. probe again via Bash — if mid-session enable works, this is DENIED
#    d. try to Edit .claude/settings.local.json from the agent — expect protected-path denial
#    e. test !holtz-stop analog: does `!echo hi` see the plugin bin? is it sandboxed?
```

If step (c) denies the socket, the whole design stands and this becomes the delivery model.

## Model A — the out-of-session launcher (fallback if mid-session enable doesn't work)

*(Kept as the fallback: if the live test shows the sandbox can't be enabled mid-session, the
launcher below — a shell-level `claude --settings …` — is how we'd deliver it instead.)*

### the easy command and the guardrail

Two things Jon asked for: the launch has to be a genuinely easy command, and running
`/holtz` in a normal (unsandboxed) session must tell the user what to do rather than fail
in a confusing way.

**The command.** A plugin *can* ship executables: files under `bin/` are "added to the Bash
tool's `PATH`" and "invokable as bare commands in any Bash tool call while the plugin is
enabled" (plugins-reference.md). holtz already ships a `bin/` (the `sahjhan` binaries live
there). So the launcher, `bin/holtz-audit`, ships with the plugin — no separate install.

Two scoping facts decide how it's used, and they're the whole subtlety:

1. That PATH is the **Bash tool's** PATH — i.e. bare-invocable *inside* a Claude session.
   It is not your login shell's PATH.
2. The sandbox is established from settings present at session **launch** (user / managed /
   project-local / `--settings`). A plugin can't inject sandbox settings (plugin
   `settings.json` supports only `agent` / `subagentStatusLine`), and a hook runs too late to
   sandbox its own session — turning the sandbox on mid-session isn't a thing.

Put together: starting a *sandboxed* audit is a launch, and a launch happens in your shell,
outside any session — so the bin/ PATH doesn't place `holtz-audit` where the launch needs it.
The clean UX that needs **no PATH setup at all**: run `/holtz` in a normal session, and the
guardrail (below) prints the exact one-liner to paste into your terminal —
`"${CLAUDE_PLUGIN_ROOT}/bin/holtz-audit"` — an absolute path straight to the shipped
launcher. Alias it for brevity if you like, but nothing forces you to.

The launcher itself does the setup the agent used to do (and shouldn't): `sahjhan init`,
start the daemon backgrounded with the socket at the C1 location, copy `daemon-init-pid`,
build the sandbox `--settings` JSON for this install (socket denied; `denyWrite` on the
plugin tree and its `.venv`; `enabled` + `failIfUnavailable` + `allowUnsandboxedCommands:
false`), then `exec claude --settings <that>`. Because it goes through `--settings`, nothing
persists and no normal session is ever sandboxed.

*(Correcting two earlier overstatements of mine: plugins **can** ship bare-command
executables — I said they couldn't — and Claude Code **does** auto-install a plugin's
declared npm/bun dependencies, so there's no "separate install command" in the general case.
The single real constraint is the launch-time-settings one above, not "plugins can't provide
commands.")*

**The guardrail.** If `/holtz` is run in a normal session, the skill's first step probes
whether it's sandboxed — a one-line Bash check of `CLAUDE_CODE_SANDBOXED`, which is set
inside sandboxed commands and empty otherwise. If empty, it stops and says, in plain terms:

> This audit runs sandboxed so the agent can't reach the answer key. This session isn't
> sandboxed. Exit and start it with `holtz-audit` — your normal work in this project is
> unaffected.

That message is UX for an honest operator. The *security* does not depend on it: in an
unsandboxed session the daemon fuse (D1) refuses to serve the vault and enforcement fails
closed (D2), so an audit can't actually proceed unsandboxed even if the prompt is ignored.
Two layers, both pointing at the same one-word fix.

## Decisions I need from you

**Settled:** direction (sandbox is the boundary, daemon enforces); Model A; the fuse (D1),
consumer fail-closed (D2), and dropping the ancestor walk (D3); the two requirements above.
Strength: the launcher's `--settings` includes `failIfUnavailable` + `allowUnsandboxedCommands:
false` *and* we keep the fuse — both, since Model A makes the locks free.

**Still open — the go/no-go on building, and where to start:**

1. **Sequencing.** I'd propose this order, each its own reviewed commit, sahjhan code and
   holtz code and anything you authored kept separate:
   - **First (sahjhan, self-contained):** C1 — teach the Rust daemon `SAHJHAN_DAEMON_SOCKET`
     so the socket can live outside cwd. Mechanical, independently correct, low risk.
   - Then the daemon fuse (D1) + direct-peer auth (D3) in sahjhan.
   - Then the holtz side: the `holtz-audit` launcher (C2), the `/holtz` guardrail, and the
     consumer fail-closed change (D2).
   - The live end-to-end sandbox test (from Verification status) *before* wiring holtz to
     depend on it.
2. **Before any code, I'd want to run the live sandbox check** — the one thing still
   asserted-not-tested. Want me to do that verification run first, as its own step, and
   report back before touching code?

Nothing gets built, no `hooks.json` edits, nothing of yours touched, until you say go.
