# Hardening

The story these mechanisms answer is in [why.md](why.md), which ends where this page starts: the agent had discovered that `sahjhan event` was the only door into the ledger, and that it had a key.

## Restricted events and HMAC authentication

Some events should only come from trusted sources. The agent records its own findings and progress, sure. But quiz results? Those come from the quiz hook. Not from the agent that's being quizzed.

Events that need this protection get marked `restricted` in `events.toml`:

```toml
[events.quiz_passed]
description = "Verification quiz completed by hook"
restricted = true
fields = [
    { name = "score", type = "string" },
    { name = "pass", type = "string" },
]

[events.finding]
description = "Code issue found during review"
fields = [
    { name = "severity", type = "string" },
    { name = "file", type = "string" },
]
```

The agent can still record findings. But when it tries to record a quiz result:

```bash
sahjhan event quiz_passed --field score=5/5 --field pass=true
# error: event type 'quiz_passed' is restricted. Use 'sahjhan authed-event' with a valid proof.
```

Getting one into the ledger means asking the daemon to append it.

### The hardened path: `record_event` over the socket

JSON request/response:

```python
# enforcement/hooks/quiz.py — listed in trusted-callers.toml
import json, os, socket

SOCKET = os.environ.get("SAHJHAN_DAEMON_SOCKET") or "output/.sahjhan/daemon.sock"

def daemon(request):
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.settimeout(5)
    try:
        sock.connect(SOCKET)
        sock.sendall((json.dumps(request) + "\n").encode())
        response = json.loads(sock.makefile().readline())
    finally:
        sock.close()
    if not response.get("ok"):
        raise RuntimeError(f"{response.get('error')}: {response.get('message')}")
    return response

resp = daemon({
    "op": "record_event",
    "event_type": "quiz_passed",
    "fields": {"score": "5/5", "pass": "true"},
})
print("recorded at seq", resp["data"]) # the reply's `data` is the sequence number of the new entry
```

```bash
$ python3 /abs/path/to/enforcement/hooks/quiz.py
recorded at seq 1
```

That `daemon()` function is the entire client. See [speaking the socket](#speaking-the-socket) for the wire format and the rest of the operations.

The daemon validates the event against your own `events.toml` before it writes anything: an undeclared type or a field that violates its pattern gets rejected as an error. A shared lock with the CLI means a hook and a transition racing each other can't tear the chain. 

What it deliberately doesn't check is `restricted`. That flag stops `sahjhan event` and stops a transition from emitting the type; it says nothing about this path. An authenticated caller records any declared event, restricted or not, because the manifest already answered the only question that matters here — what is on the other end of the socket. So `trusted-callers.toml` isn't extra credit once you're recording restricted events this way. It is the entire boundary.

#### when the signer isn't the recorder

`record_event` covers the case where the hook holding the evidence is the hook recording it. There's no gap for a proof to cover: the daemon is already authenticating that hook, and it needs nothing else. Recording is never what a proof is for.

The gap opens when the evidence and the record are in different processes, with ground the agent can write in between. One hook grades, another decides. The claim has to sit in a file for a moment, and a file proves nothing about who wrote it. A proof does, and `verify` is the one operation that takes one back.

So the daemon acts as notary. One hook signs its result and leaves it where the next will find it:

```python
# grade.py — signs the result, drops it where the next hook will pick it up
fields = {"score": "5/5", "pass": "true"}
proof = daemon({"op": "sign", "event_type": "quiz_passed", "fields": fields})["proof"]
json.dump({"fields": fields, "proof": proof}, open("output/quiz-result.json", "w"))
```

The other sends the proof back before it will act on the claim:

```python
# gate.py — the file is agent-writable, so the proof is the only thing worth believing
claim = json.load(open("output/quiz-result.json"))
try:
    daemon({"op": "verify", "event_type": "quiz_passed",
            "fields": claim["fields"], "proof": claim["proof"]})
except RuntimeError:
    sys.exit("refusing a quiz result that doesn't match its proof")
daemon({"op": "record_event", "event_type": "quiz_passed", "fields": claim["fields"]})
```

Edit the score on its way between the two and the second hook won't take it:

```
verify -> {"ok": true, "verified": true}
verify -> {"ok": false, "error": "invalid_proof", "message": "proof does not match"}
```

Note what the last line of `gate.py` doesn't do: present the proof. The recording is authorized by `gate.py` being a listed caller, same as ever. The proof did one job, which was to let it believe a file it didn't write.

Two things that job doesn't include. The proof carries no signer identity — every trusted caller's proof for the same tuple is byte-identical, so `verify` answers "something with socket access signed this," never "grade.py signed this." And it isn't replay protection: the payload is the event type plus sorted fields with no nonce, so a proof stays good for that exact tuple as long as the daemon lives. If you need either, put the signer's name or a nonce in the fields, where the proof covers them.

### Driving it from the CLI

Restricted events also go through `sahjhan authed-event`, which requires an HMAC-SHA256 proof. The proof is computed over the event type and sorted fields, separated by null bytes. To get the proof, the hook asks the daemon:

```bash
# Inside the quiz hook (not the agent)
PROOF=$(sahjhan sign --event-type quiz_passed --field score=5/5 --field pass=true)

sahjhan authed-event quiz_passed \
    --field score=5/5 --field pass=true \
    --proof "$PROOF"
```

The `sign` command connects to the daemon process, which holds the session key in memory and computes the HMAC. The key never touches disk. The agent can't forge the proof because it can't get the key, and it can't get the key because the key isn't a file.

What this recipe can't survive is caller authentication. Drop a `trusted-callers.toml` in place and both halves of it stop working:

```bash
$ sahjhan sign --event-type quiz_passed --field score=5/5 --field pass=true
sign failed: caller not authenticated
```

The socket peer is the CLI, and the CLI can never be in the manifest, whatever spawned it — including a trusted hook that shells out to it. That isn't a limitation to work around. It's the whole mechanism, because `sahjhan sign` is exactly the door the agent would knock on. What a hardened deployment loses here is the CLI as a courier, not the proof itself.

## The daemon

I tried file-based keys first. I kept a `session.key` on disk with enforcement hooks blocking reads via string-matching on tool arguments. If the agent called `Read` on `.sahjhan/session.key`, the PreToolUse hook intercepted it. Simple.

Here's what the agent did instead:

```python
# Written by the agent, run via Bash
import pathlib
p = pathlib.Path(".sahjhan") / "session.key"
print(p.read_bytes().hex())
```

The hook saw `python3 script.py`, not the protected path, and leaked the key. String-matching on tool arguments has infinite bypasses: variables, indirection, heredocs, base64-encoded paths, `exec()`. Any agent with Bash access can construct a read that the hook will never see.

The only place a secret can live that a same-user adversary with code execution can't reach is in the memory of another process.

So Sahjhan has a daemon mode. `sahjhan daemon start` launches a background process that generates a 32-byte session key, holds it in memory, and serves signing and vault operations over a Unix domain socket. The key is never written to disk. `mlock` tries to prevent it from leaking to swap. `ptrace(PT_DENY_ATTACH)` on macOS and `prctl(PR_SET_DUMPABLE, 0)` on Linux block debugger attachment and `/proc/pid/mem` reads. If the environment has `LD_PRELOAD` or `DYLD_INSERT_LIBRARIES` set, the daemon refuses to start. Pass `--idle-timeout <seconds>` if you want it to shut itself down after a quiet period, which is handy in CI.

When a hook needs a proof, it calls `sahjhan sign`. The CLI connects to the daemon socket, the daemon computes the HMAC, and the proof comes back. The agent never sees the key. Even if the agent reads the Rust source code and understands exactly how the HMAC is computed, it still can't produce one without those 32 bytes in the daemon's address space.

The daemon also authenticates its callers once you opt in. Drop a `trusted-callers.toml` next to the protocol files and every request except `status` goes through peer verification: the daemon reads the connecting process's PID from kernel-provided socket credentials (unspoofable), reads that process's command line, and requires the script it is running to be in the manifest:

```toml
# enforcement/trusted-callers.toml
[callers]
"hooks/lens_quiz.py" = "sha256:7890abcdef01..."
"hooks/stop_hook.py" = "sha256:2345678901ab..."
```

Keys are relative to `--config-dir`, and both the path and the SHA-256 hash must match. The check applies to the *direct* peer only. There is deliberately no walk up the process tree, because manifest authority must not be inheritable: a child process of a trusted script is not itself trusted.

Trusted scripts have to live inside the config dir, and they have to be invoked by absolute path, because the daemon resolves a relative cmdline path against its own working directory, not the caller's.

The agent can't get a proof through the CLI because `sahjhan sign`'s socket peer is the CLI binary, which can never satisfy the manifest, whatever spawned it. It can't write a script into the trusted path because the bootstrap hook blocks writes to `enforcement/`. It can't modify a trusted script to do something else because the hash won't match. And it can't copy a trusted script somewhere and run it from there because the paths won't match.

If you don't provide the file, the daemon serves every caller. Providing the file with an empty `[callers]` table denies everyone.

The manifest itself is the root of trust here. It decides who may sign, so it lives inside the config seal (see below) alongside the protocol files. Rewriting it to add your own caller and restarting the daemon still works *at the socket* because the daemon loads whatever file is on disk, but it stops being silent. The next command that opens the ledger re-hashes `trusted-callers.toml`, sees it drift from the sealed value, and refuses to run with a config integrity violation.

The vault handles more than signing keys. It can act as a general purpose key-value store, and you can restrict it to trusted-callers if you don't want the agent to see the contents. For example, the quiz bank from the example in the README.md lives in the vault. `sahjhan vault store --name quiz-bank --file data.json` can load in a file, which you can then retrieve with `sahjhan vault read --name quiz-bank` during the quiz. Once it's loaded in, you can delete the file from disk. Or better yet, you can have a script build that kind of stuff on the fly, so there's nothing for an agent to see.

### State-gated vault keys

A vault key can also be restricted to particular protocol states, declared in an optional `vault.toml` rather than in imperative hook code. The daemon derives the current state from the active ledger and rejects the operation otherwise.

```toml
# enforcement/vault.toml
[[policy]]
name = "quiz-bank"
readable_in_states = ["reviewing"]
writable_in_states = ["idle"]
deletable_in_states = []
```

If you don't include one of those fields, that action's unconstrained for the key. An empty list denies all. And a populated list limits actions on that key to only when sahjhan's in the listed states. This gives really fine-grained control over read, write, and delete. The example above loads the quiz bank once at the start of a run, reads it only while reviewing, and never lets it be deleted.

If the ledger state can't be determined, a gated operation is rejected rather than allowed. `vault.toml` is one of the eight sealed config files, so a policy can't be loosened without a `reseal`.

Protocol scripts sometimes need their own scratch state, like counters, timers, and flags that the agent shouldn't be able to read or edit. The daemon exposes `enforcement_read` / `enforcement_write` / `enforcement_update` over the socket for that. They live under a reserved `_`-prefixed vault namespace that `vault read` and `vault list` refuse to touch so agents are blocked.

One key in that blob is special: `state`. `enforcement_read` resolves the active ledger, verifies its hash chain, derives the current state from the last `state_transition`, and overrides `state` in the response. If the ledger can't be resolved or fails verification, the stored bytes are served unchanged. There is deliberately no socket op to *write* the state: a CLI invoked by the agent can't authenticate as a trusted caller, and the ledger is already the source of truth.

Recording an *event*, though, is different from writing that `state` key. Trusted callers can do it over the socket with `record_event`. The hook, already authenticated as a trusted caller on its own socket connection, asks the daemon to append the event directly. The daemon validates it against the consumer's `events.toml` and appends to the active ledger under the same exclusive lock the CLI uses.

Relatedly, `sahjhan status --no-gates` skips transition gate evaluation. Plain `status` evaluates every candidate transition's gates to print `ready`/`blocked`, and `command_succeeds` gates can spawn test suites, whereas hooks run under a five-second timeout. With `--no-gates` you get state, sets, event count, and chain verification with no side effects.

Kill the daemon and the secrets vanish. The daemon cleans up its socket and PID files on shutdown. If it dies uncleanly, stale files get cleaned on the next start.

## Speaking the socket

Nothing generates a client for you. `sahjhan hook generate` writes four files — `pre_tool_hook.py`, `post_tool_hook.py`, `stop_hook.py`, and `_sahjhan_bootstrap.py` — and none of them opens the socket. The first three shell out to `sahjhan hook eval`, which needs no authentication, and the bootstrap is a self-contained path guard. So a trusted hook speaks the protocol by hand. The fifteen-line `daemon()` function [above](#the-hardened-path-record_event-over-the-socket) is the whole of it; there is nothing else to import.

The protocol is newline-delimited JSON over a Unix `SOCK_STREAM`: one request object per line, one response line back, in order. A connection can carry several requests, and authentication happens once when it's opened, so a hook doing three things should keep one connection rather than reconnecting. Both directions are bounded at ten seconds, and the daemon serves connections one at a time — hold one open and idle and you're blocking every other hook.

Every response is one flat object with `ok` telling you which kind it is. On failure you get a machine-readable `error` alongside the human `message`, plus a `reason` when authentication was what failed:

```json
{"ok":true,"pid":15163,"uptime_seconds":13,"vault_entries":0,"idle_seconds":0,"idle_timeout":300,"enforcement_active":false}
{"ok":false,"error":"auth_failed","message":"caller not authenticated","reason":"pid_resolution_failed"}
```

Match on `error`, not on the prose. `auth_failed` means enforcement is broken and you should fail closed; `not_found` just means nothing is stored yet.

| op | send | get back |
| --- | --- | --- |
| `record_event` | `event_type`, `fields` | `data` — seq of the new entry |
| `sign` | `event_type`, `fields` | `proof` |
| `verify` | `event_type`, `fields`, `proof` | `verified` |
| `vault_store` | `name`, `data` | — |
| `vault_read` | `name` | `data` |
| `vault_delete` | `name` | — |
| `vault_list` | — | `names` |

The four `vault_*` ops carry one more condition: if `vault.toml` declares a policy for that key, the action also has to be permitted in the current ledger-derived state, or it comes back `state_forbidden`.

| `enforcement_read` | — | `data` (with `state` overridden from the ledger) |
| `enforcement_write` | `data` | — |
| `enforcement_update` | `patch` | — |
| `status` | — | `pid`, `uptime_seconds`, `vault_entries`, `idle_seconds`, `idle_timeout`, `enforcement_active` |

That's the complete list. An unrecognized `op` comes back as a `parse_error` that names the valid ones. `status` is the only operation exempt from caller authentication, so it works as a health check from anywhere — which also means it's the one thing the agent can see.

Both sides resolve the socket the same way: `SAHJHAN_DAEMON_SOCKET` if it's set and non-empty, otherwise `daemon.sock` inside `data_dir`. Set it. The default puts the socket inside the project, where the agent can delete it or squat the path between restarts, and an armed fuse refuses to serve there at all. Put it somewhere the sandbox denies writes to — keyed by a hash of the project path, if you run more than one checkout — and start the daemon and the hooks with the same value.

## The boundary: the sandbox fuse

Despite all of this hardening, a same-user process can ultimately defeat sahjhan by doing things like forking a connected file descriptor into an exec of a genuine hook, rewriting its own command line, or loading code into a trusted interpreter.

For stronger security guarantees, Claude Code can run every Bash command inside a sandbox that blocks access to Unix sockets outside an allowlist. Confined that way, the agent can't reach the daemon socket at all. The trusted hooks run outside the sandboxed Bash tool, so they still can.

You can enforce it in `protocol.toml`:

```toml
[daemon]
require_sandbox = true
```

An armed daemon checks, on every request except `status`, that the Claude Code settings demand the sandbox: effective `sandbox.enabled = true`, `allowUnsandboxedCommands = false`, and `failIfUnavailable = true` configured across the settings scopes (project `settings.local.json`, project `settings.json`, then `~/.claude/settings.json`, in that precedence), no scope allowlists the daemon socket, no scope has a non-empty `excludedCommands`, and the socket itself resides outside the project root — which is what `SAHJHAN_DAEMON_SOCKET` is for. Otherwise, the request is refused with `sandbox_required` and a machine-readable reason.

On Linux, blocking socket access additionally requires the optional seccomp filter (`@anthropic-ai/sandbox-runtime`), which the fuse cannot verify.

## Config integrity

So the ledger can't be edited. The hash chain works. Restricted events need a proof the agent can't forge. The session key lives in a process the agent can't reach. Everything is locked down.

Everything except the TOML config files. You know, the ones that define every gate, every transition, every rule. `transitions.toml` is just a text file sitting in a directory the agent can write to. Remove a gate, lower a threshold, add `gates = []` to the one transition that matters, and Sahjhan's hardened ledger will faithfully enforce your gutted protocol.

To mitigate this, on `sahjhan init`, Sahjhan SHA-256 hashes all eight sealed config files (the five protocol files, `hooks.toml`, `vault.toml`, and `trusted-callers.toml`) and stores those hashes in the genesis ledger entry. Every command that opens the ledger re-hashes and compares (commands that don't, like `validate`, `lint`, and `daemon start`, skip the check). If one byte's different Sahjhan won't run:

```
error: config integrity violation:
  - transitions.toml (expected: 7f83b1657ff1..., found: a3c2e88d1f2b...)

Run 'sahjhan reseal' with a valid session key to update the seal,
or 'sahjhan init' to start a new ledger.
```

Config changes do happen legitimately during protocol development. `sahjhan reseal` updates the seal. It requires an HMAC proof, the same as restricted events. Without the session key, the agent can rewrite transitions.toml all day long. It just can't use it. The reseal event goes into the ledger, so you can see exactly when the rules changed and decide whether that was you or the agent getting creative.

To reseal after a legitimate config change:

```bash
PROOF=$(sahjhan sign --event-type config_reseal)
sahjhan reseal --proof "$PROOF"
```

The payload is just the event type with no fields. This recipe works only while `trusted-callers.toml` is unconfigured. With caller auth on, a trusted hook can still get the proof over the socket — it just has nowhere to spend it. `reseal` verifies through the daemon like everything else, and the peer it presents is the CLI:

```bash
# inside a trusted hook, holding a valid proof it signed for over the socket
$ sahjhan reseal --proof "$PROOF"
error: caller not authenticated
```

There's no `reseal` op on the wire to use instead, and the same goes for `reset`. Both are reachable only through the CLI, so do your resealing while caller auth is off and turn it on once the protocol has stopped changing.

## Gate attestation

So the ledger can't be edited. Restricted events need proof. Config is sealed. What about the gates themselves?

When a `command_succeeds` gate runs `python -m pytest tests/`, Sahjhan executes the command, checks the exit code, and records a `state_transition` event: "moved from implementing to verifying." That's it. The transition happened. But the ledger says nothing about *why* it was allowed. What command ran? What did it output? When? The evidence exists for about three hundred milliseconds inside a Rust struct, then gets thrown away.

It's great to see that the agent transitioned, but you've got to know if the tests actually passed. You're trusting the gate's boolean. That's better than trusting the agent, but it's still a gap in traceability.

Gate attestation closes it. When a `command_succeeds`, `command_output`, or `snapshot_compare` gate passes during a transition, Sahjhan emits a `gate_attestation` event immediately after the `state_transition`:

```bash
sahjhan log tail 2
# {"event_type": "state_transition", "fields": {"from": "implementing", "to": "verifying", "command": "submit"}, ...}
# {"event_type": "gate_attestation", "fields": {"gate_type": "command_succeeds", "command": "python -m pytest tests/", "exit_code": "0", "stdout_hash": "a3c2e88d1f2b...", "wall_time_ms": "4523", "executed_at": "2026-03-30T14:23:07.123Z", "working_dir": "/Users/you/proj", "transition_command": "submit"}, ...}
```

The `stdout_hash` is SHA-256 of the raw command output. The agent can't fabricate it because Sahjhan itself runs the command and computes the hash. For deterministic commands (most test suites, linters, build tools), replaying the command should reproduce the hash. That's an independently verifiable claim sitting in a hash-chained ledger.

The `working_dir` is where the command ran. Usually the project root, but it can change to [`anchor = "caller"`](gates.md#notes-on-the-ones-with-sharp-edges), which runs in whatever directory sahjhan was invoked from.

Every command and snapshot gate attests by default. If a gate runs something trivial that isn't worth recording (a warmup check, an `echo`), suppress it:

```toml
{ type = "command_succeeds", cmd = "echo warmup", attest = false }
```

The attestation event is `restricted`: mark it in your `events.toml` and the agent can't forge one via `sahjhan event`. It'll get the same rejection as a fabricated quiz result:

```bash
sahjhan event gate_attestation --field gate_type=command_succeeds --field stdout_hash=abc123 ...
# error: event type 'gate_attestation' is restricted. Use 'sahjhan authed-event' with a valid proof.
```

This means the ledger has two tiers of evidence, machine-attested (the gate ran, here's the hash), and agent-reported (I reviewed this, trust me), with different confidence levels that auditors can distinguish between.

## What the agent tries, and what happens

| What the agent tries | What happens |
|----------------|----------|
| Write/Edit to managed file | Blocked before execution |
| Edit source without a failing test | Runtime hook checks gate, blocks the edit |
| Claim "audit complete" in non-terminal state | Stop hook pattern-matches output, blocks |
| Shell tricks (`echo >`, `sed -i`, `python -c`) | Invisible to the built-in guards, which see Edit/Write, not Bash. `manifest verify` catches the aftermath; recording a violation takes a `hooks.toml` rule |
| Delete the ledger | Hash chain for deletion. Reset is a real command, but it needs an HMAC proof the agent can't forge. |
| Fabricate history entries | Every entry is hash-linked. Fabricate one, the chain breaks. |
| `sahjhan event quiz_passed --field pass=true` | Restricted. Needs HMAC proof the agent can't forge. |
| Read the session key to forge proofs | No key on disk. It's in the daemon's memory. `ptrace` blocked. |
| Write a script that reads the key indirectly | There's nothing to read. The key never touches the filesystem. |
| Call `sahjhan sign` directly from Bash | With caller auth configured, the CLI can never authenticate, whatever spawned it. Rejected. |
| Modify a trusted hook to leak the proof | Hash won't match the manifest. Rejected. |
| Edit `transitions.toml` to remove gates | Config sealed at init. One byte different, Sahjhan won't run. |
| Kill the daemon to extract secrets | Secrets vanish from memory. Denial of service, not exfiltration. |
| `sleep 65 && check_convergence` | Been there. Use evidence gates. |
| Reach the daemon socket at all, sandboxed | The sandbox blocks the connection; an armed daemon refuses to serve an unsandboxed session either way. |
| `cat` the binary to find loopholes | Compiled Rust. Let me know how it goes. |
| Query the ledger to find exploits | Go ahead. It's your permanent record. |

## What this doesn't do

Sahjhan is not a sandbox. The agent runs as you, with your filesystem and your network, and the threat model is an agent taking the cheap way out rather than an attacker with a budget and a plan. The daemon raises the cost of extracting a key from "four lines of Python" to something meaningfully harder; the actual wall, when you need one, is the OS sandbox the fuse insists on. Anything that can attach to your session unsandboxed can do what you can do.

The seal covers eight config files. A gate that shells out to `scripts/check.py` is trusting a script outside the seal unless you also put it under `paths.managed` and let the manifest track it.

The perimeter of managed paths, write guards, and stop hooks depends on the harness actually calling the hooks. The ledger, the seal, and the gates don't rely on hooks.
