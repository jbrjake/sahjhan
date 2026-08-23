# Hardening

The mechanisms, each in the words that introduced it. The story they answer is in
[why.md](why.md), which ends where this page starts: the agent had discovered
that `sahjhan event` was the only door into the ledger, and that it had a key.

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

Restricted events go through `sahjhan authed-event`, which requires an HMAC-SHA256 proof. The proof is computed over the event type and sorted fields, separated by null bytes. To get the proof, the hook asks the daemon:

```bash
# Inside the quiz hook (not the agent)
PROOF=$(sahjhan sign --event-type quiz_passed --field score=5/5 --field pass=true)

sahjhan authed-event quiz_passed \
    --field score=5/5 --field pass=true \
    --proof "$PROOF"
```

Or from Python:

```python
import subprocess

proof = subprocess.check_output([
    "sahjhan", "sign",
    "--event-type", "quiz_passed",
    "--field", "score=5/5",
    "--field", "pass=true",
]).decode().strip()

subprocess.run([
    "sahjhan", "authed-event", "quiz_passed",
    "--field", "score=5/5", "--field", "pass=true",
    "--proof", proof,
])
```

The `sign` command connects to the daemon process, which holds the session key in memory and computes the HMAC. The key never touches disk. The agent can't forge the proof because it can't get the key, and it can't get the key because the key isn't a file. More on why that matters in a moment.

## The daemon

I tried file-based keys first. `session.key` on disk, enforcement hooks blocking reads via string-matching on tool arguments. If the agent calls `Read` on `.sahjhan/session.key`, the PreToolUse hook intercepts it. Simple.

Here's what the agent did instead:

```python
# Written by the agent, run via Bash
import pathlib
p = pathlib.Path(".sahjhan") / "session.key"
print(p.read_bytes().hex())
```

The hook saw `python3 script.py`. Not the protected path. The key leaked in one move. String-matching on tool arguments has infinite bypasses: variables, indirection, heredocs, base64-encoded paths, `exec()`. Any agent with Bash access can construct a read that the hook will never see.

The only place a secret can live that a same-user adversary with code execution can't reach is in the memory of another process.

So Sahjhan has a daemon mode. `sahjhan daemon start` launches a background process that generates a 32-byte session key, holds it in memory, and serves signing and vault operations over a Unix domain socket. The key is never written to disk. `mlock` prevents it from leaking to swap (best-effort — it's not the primary defense). `ptrace(PT_DENY_ATTACH)` on macOS and `prctl(PR_SET_DUMPABLE, 0)` on Linux block debugger attachment and `/proc/pid/mem` reads. If the environment has `LD_PRELOAD` or `DYLD_INSERT_LIBRARIES` set, the daemon refuses to start. Pass `--idle-timeout <seconds>` if you want it to shut itself down after a quiet period — useful in CI, where a daemon outliving the job is just a leaked process with secrets in it.

When a hook needs a proof, it calls `sahjhan sign`. The CLI connects to the daemon socket, the daemon computes the HMAC, and the proof comes back. The agent never sees the key. Even if the agent reads the Rust source code and understands exactly how the HMAC is computed, it still can't produce one without those 32 bytes in the daemon's address space.

The daemon also authenticates its callers. Every connection goes through PID-based verification: the daemon reads the connecting process's PID from kernel-provided socket credentials (unspoofable), walks up the process tree to find the hook script, resolves its path, and checks it against a trusted-callers manifest:

```toml
# enforcement/trusted-callers.toml
[callers]
"enforcement/hooks/lens_quiz.py" = "sha256:7890abcdef01..."
"enforcement/hooks/stop_hook.py" = "sha256:2345678901ab..."
```

Both the path and the SHA-256 hash must match. The agent can't call `sahjhan sign` directly (parent PID would be `bash`, not a trusted script). It can't write a script into the trusted path (the bootstrap hook blocks writes to `enforcement/`). It can't modify a trusted script to do something else (hash won't match). It can't copy a trusted script somewhere and run it from there (path won't match).

The manifest itself is the root of trust here — it decides who may sign — so it lives inside the config seal (below) alongside the protocol files. Rewriting it to add your own caller and restarting the daemon no longer works silently: the next command re-hashes `trusted-callers.toml`, sees it drift from the sealed value, and refuses to run with a config integrity violation. Changing who can sign now requires a `reseal`, which needs the session key and lands on the permanent record — the same authenticated path as any other config change.

The vault handles more than keys. The quiz bank — the questions the hook uses to verify the agent actually read the code — lives in daemon memory too. The hook loads it at startup with `sahjhan vault store --name quiz-bank --file questions.json`, then retrieves questions with `sahjhan vault read --name quiz-bank` during the quiz. The file gets deleted from disk after loading. No point in HMAC-protecting the answers if the agent can read the questions in advance, and no point in leaving a file on disk that doesn't need to exist.


### State-gated vault keys

A vault key can also be restricted to particular protocol states, declared in an
optional `vault.toml` rather than in imperative hook code. The daemon derives the
current state from the active ledger and rejects the operation otherwise.

```toml
# enforcement/vault.toml
[[policy]]
name = "quiz-bank"
readable_in_states = ["reviewing"]
writable_in_states = ["idle"]
deletable_in_states = []
```

Each of the three fields is independent, and each has three meanings: omitted
means no constraint (which is how every key behaved before policies existed), an
empty list means the operation is never permitted, and a list of states means
only in those states. The example above loads the quiz bank once at the start of
a run, reads it only while reviewing, and never lets it be deleted.

This one is fail-closed: if the ledger state can't be determined, a gated
operation is rejected rather than allowed, which is the opposite of how
`enforcement_read` treats an unreadable ledger. `vault.toml` is one of the eight
sealed config files, so a policy can't be loosened without a `reseal`.

Protocol scripts sometimes need their own scratch state — counters, timers, flags — that the agent shouldn't be able to read or edit. The daemon exposes `enforcement_read` / `enforcement_write` / `enforcement_update` over the socket for that. They live under a reserved `_`-prefixed vault namespace that `vault read` and `vault list` refuse to touch. Same process, different door, and the agent doesn't have a key to either.

One key in that blob is special: `state`. Consumers used to write it themselves after parsing `sahjhan status`, which meant it was only as fresh as their last successful refresh — transitions advance the ledger, not the vault, and a hook that times out refreshing leaves the daemon serving a state many transitions old (holtz #57 wedged a session for 150+ turns this way). Since v0.14.0 the daemon ignores the stored value on read: `enforcement_read` resolves the active ledger, verifies its hash chain, derives the current state from the last `state_transition`, and overrides `state` in the response. If the ledger can't be resolved or fails verification, the stored bytes are served unchanged. There is deliberately no socket op to *write* the state — a CLI invoked by the agent can't authenticate as a trusted caller, and the ledger is already the source of truth.

Recording an *event*, though, is different from writing that `state` key — and since v0.15.0 a trusted caller can do it over the socket with `record_event`. This exists because `restricted` events (like a `context_reset` attesting a `/clear` happened) previously had to go through `authed-event`: a hook asks the daemon to `sign` the event, then hands the proof to a separate `sahjhan authed-event` process to submit. That submitter is the bare CLI binary, and the daemon can't resolve *it* to a trusted script — the ancestor walk finds no hook in its cmdline — so the submit is rejected even though the sign succeeded, and a swallowed exit code hid the failure. `record_event` collapses the two steps into one: the hook, already authenticated as a trusted caller on its own socket connection (exactly like `sign` and `enforcement_write`), asks the daemon to append the event directly. The daemon validates it against the consumer's `events.toml` and appends to the active ledger under the same exclusive lock the CLI uses. No proof to mint, hand off, or leak; no courier to authenticate. It's the ledger-write analog of `enforcement_write` — the peer's identity is the authorization.

Relatedly, `sahjhan status --no-gates` skips transition gate evaluation. Plain `status` evaluates every candidate transition's gates to print `ready`/`blocked`, and `command_succeeds` gates can spawn test suites — poison for a hook running under a five-second timeout. With `--no-gates` you get state, sets, event count, and chain verification with no side effects; the transitions section is omitted and a warning says why.

Kill the daemon and the secrets vanish. That's denial of service, not exfiltration. The daemon cleans up its socket and PID files on shutdown. If it dies uncleanly, stale files get cleaned on the next start.

## Config integrity

So the ledger can't be edited. The hash chain works. Restricted events need a proof the agent can't forge. The session key lives in a process the agent can't reach. Everything is locked down.

Everything except the TOML config files. You know, the ones that define every gate, every transition, every rule. `transitions.toml` is just a text file sitting in a directory the agent can write to. Remove a gate, lower a threshold, add `gates = []` to the one transition that matters — and Sahjhan's hardened ledger will faithfully enforce your gutted protocol. The vault door is six inches of steel. The wall next to it is drywall.

On `sahjhan init`, Sahjhan SHA-256 hashes all seven sealed config files — the five protocol files, `hooks.toml`, and `trusted-callers.toml` — and stores those hashes in the genesis ledger entry. Every subsequent command re-hashes and compares. One byte different — a gate removed, a timeout changed, a comment added to make the diff look innocent — and Sahjhan won't run:

```
error: config integrity violation:
  - transitions.toml (expected: 7f83b1657ff1..., found: a3c2e88d1f2b...)

Run 'sahjhan reseal' with a valid session key to update the seal,
or 'sahjhan init' to start a new ledger.
```

Config changes do happen legitimately. You add a state, tune a gate, fix a typo. `sahjhan reseal` updates the seal — but it requires an HMAC proof, same as restricted events. Without the session key, the agent can rewrite transitions.toml all day long. It just can't use it. The reseal event goes into the ledger, so you can see exactly when the rules changed and decide whether that was you or the agent getting creative.

To reseal after a legitimate config change:

```bash
PROOF=$(sahjhan sign --event-type config_reseal)
sahjhan reseal --proof "$PROOF"
```

Same daemon, same signing mechanism, same key the agent can't reach. The payload is just the event type with no fields.

## Gate attestation

So the ledger can't be edited. Restricted events need proof. Config is sealed. What about the gates themselves?

When a `command_succeeds` gate runs `python -m pytest tests/`, Sahjhan executes the command, checks the exit code, and records a `state_transition` event: "moved from implementing to verifying." That's it. The transition happened. But the ledger says nothing about *why* it was allowed. What command ran? What did it output? When? The evidence exists for about three hundred milliseconds inside a Rust struct, then gets thrown away.

Which means: you can see that the agent transitioned, but you can't see that the tests actually passed. You're trusting the gate's boolean. That's better than trusting the agent, but it's still a gap you could drive a fabricated quiz result through.

Gate attestation closes it. When a `command_succeeds`, `command_output`, or `snapshot_compare` gate passes during a transition, Sahjhan now emits a `gate_attestation` event immediately after the `state_transition`:

```bash
sahjhan log tail 2
# {"event_type": "state_transition", "fields": {"from": "implementing", "to": "verifying", "command": "submit"}, ...}
# {"event_type": "gate_attestation", "fields": {"gate_type": "command_succeeds", "command": "python -m pytest tests/", "exit_code": "0", "stdout_hash": "a3c2e88d1f2b...", "wall_time_ms": "4523", "executed_at": "2026-03-30T14:23:07.123Z", "transition_command": "submit"}, ...}
```

The `stdout_hash` is SHA-256 of the raw command output. The agent can't fabricate it because Sahjhan runs the command and computes the hash — the agent never touches either. For deterministic commands (most test suites, linters, build tools), replaying the command should reproduce the hash. That's an independently verifiable claim sitting in a hash-chained ledger.

Every command and snapshot gate attests by default. If a gate runs something trivial that isn't worth recording (a warmup check, an `echo`), suppress it:

```toml
{ type = "command_succeeds", cmd = "echo warmup", attest = false }
```

The attestation event is `restricted` — mark it in your `events.toml` and the agent can't forge one via `sahjhan event record`. It'll get the same rejection as a fabricated quiz result:

```bash
sahjhan event gate_attestation --field gate_type=command_succeeds --field stdout_hash=abc123 ...
# error: event type 'gate_attestation' is restricted. Use 'sahjhan authed-event' with a valid proof.
```

The ledger now has two tiers of evidence: machine-attested (the gate ran, here's the hash) and agent-reported (I reviewed this, trust me). Different confidence levels, explicitly marked. An auditor can tell which is which. The agent can't blur the line.

## What the agent tries, and what happens

| What the agent tries | What happens |
|----------------|----------|
| Write/Edit to managed file | Blocked before execution |
| Edit source without a failing test | Runtime hook checks gate, blocks the edit |
| Claim "audit complete" in non-terminal state | Stop hook pattern-matches output, blocks |
| Shell tricks (`echo >`, `sed -i`, `python -c`) | Detected, violation recorded permanently |
| Delete the ledger | Hash chain for deletion. Reset is a real command, but it needs an HMAC proof the agent can't forge. |
| Fabricate history entries | Every entry is hash-linked. Fabricate one, the chain breaks. |
| `sahjhan event quiz_passed --field pass=true` | Restricted. Needs HMAC proof the agent can't forge. |
| Read the session key to forge proofs | No key on disk. It's in the daemon's memory. `ptrace` blocked. |
| Write a script that reads the key indirectly | There's nothing to read. The key never touches the filesystem. |
| Call `sahjhan sign` directly from Bash | Daemon checks caller PID. Parent is `bash`, not a trusted hook. Rejected. |
| Modify a trusted hook to leak the proof | Hash won't match the manifest. Rejected. |
| Edit `transitions.toml` to remove gates | Config sealed at init. One byte different, Sahjhan won't run. |
| Kill the daemon to extract secrets | Secrets vanish from memory. Denial of service, not exfiltration. |
| `sleep 65 && check_convergence` | Been there. Use evidence gates. |
| `cat` the binary to find loopholes | Compiled Rust. Let me know how it goes. |
| Query the ledger to find exploits | Go ahead. It's your permanent record. |

## What this doesn't do

Sahjhan is not a sandbox. The agent runs as you, with your filesystem and your
network, and the threat model is an agent taking the cheap way out rather than an
attacker with a budget and a plan. The daemon raises the cost of extracting a key
from "four lines of Python" to something meaningfully harder. It does not make it
impossible, and anything that can attach to your session can do what you can do.

The seal covers eight config files. A gate that shells out to `scripts/check.py`
is trusting a script outside the seal unless you also put it under
`paths.managed` and let the manifest track it.

The perimeter — managed paths, write guards, stop hooks — depends on the harness
actually calling the hooks. The ledger, the seal, and the gates don't.
