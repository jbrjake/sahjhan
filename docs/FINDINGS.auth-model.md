# What actually authorizes what

**Status:** findings, not a proposal. Nothing here changes code or docs.
**Binary under test:** `target/release/sahjhan`, v0.23.0, HEAD `411e5b1`, macOS.
**How to check it:** every experimental claim below was produced by the script in
[Appendix A](#appendix-a-reproduction-script). Run it and compare. Every source
claim is a `file:line` you can open. Do not take any of it on my word.

Labels used throughout: **[E]** verified by running it today, **[S]** read from
source, **[I]** inference — stated as such, never mixed in.

---

## The answer, in one place

**Is signing needed and used in socket mode?**

**Used: yes.** Signing works over the socket for trusted callers and is a live
capability of socket mode. `sign` and `verify` both function for a
manifest-listed script; the CLI is excluded from both. **[E]** §1.

**Needed by sahjhan itself: no.** Not for anything. No socket operation consumes
a proof in order to act — `record_event`, `vault_*`, and `enforcement_*` take no
proof parameter at all (`src/daemon/protocol.rs:16-55`). Recording a *restricted*
event over the socket requires no proof (F1). Every proof-consuming command
(`authed-event`, `reseal`, `reset`) is a CLI command, and under caller auth the
CLI can't authenticate, so none of them is reachable anyway (F6).

**Needed by your protocol: only for one job.** Making a claim tamper-evident
while it travels between two trusted processes through storage the agent can
write. Nothing else in the system does that, and it is the only thing signing
does here. §2 covers it in full.

**The decision rule:** if the trusted process that produces a claim is the same
one that records it, you never need a proof — call `record_event`. If a claim is
produced by one trusted process and acted on by another, and it passes through
somewhere the agent can reach in between, signing is the only tool that helps. If
it passes between two trusted processes but never needs to leave daemon memory,
use `enforcement_write`/`enforcement_read` instead — also no proof.

That is the whole answer. Sections 1 and 2 are the evidence for it; §3 onward is
about other things entirely.

---

## 0. Why this has been confusing

Four different things in this system have overlapping names, and I have been
sliding between them. They are not layers of one mechanism. They are separate
mechanisms that happen to share vocabulary:

| Thing | What it actually is | Lives where |
|---|---|---|
| `restricted = true` | a flag on an **event type** in `events.toml` | config |
| a **proof** | a 64-hex-char HMAC-SHA256 string over `event_type` + sorted fields | a string you pass around |
| `verify` | a **socket operation** that answers yes/no about a proof | daemon wire protocol |
| `sahjhan authed-event` / `reseal` / `reset` | **CLI commands** that ask `verify` before acting | CLI process |
| `trusted-callers.toml` | **peer identity** — which script may open the socket | daemon connection setup |

The sentence "restricted events are signed" mixes three of these and is false on
the path that matters. Sorting them out is most of this document.

---

## 1. What `verify` is

**[S]** `verify` is one of eleven operations on the daemon's Unix socket. Its
wire shape is at `src/daemon/protocol.rs:33`:

```rust
Verify {
    event_type: String,
    fields: HashMap<String, String>,
    proof: String,
}
```

**[S]** Its entire implementation is `src/daemon/mod.rs:579-590`:

```rust
Request::Verify { event_type, fields, proof } => {
    let expected = compute_sign(session_key, &event_type, &fields);
    if constant_time_eq(proof.as_bytes(), expected.as_bytes()) {
        Response::ok_verified()
    } else {
        Response::err("invalid_proof", "proof does not match")
    }
}
```

That is the whole thing. It recomputes the HMAC and compares. **It does not
write to the ledger, does not unlock anything, and does not grant any
capability.** It is a pure predicate — an oracle you can ask a yes/no question.

**[E]** Observed both answers, from a manifest-listed script:

```
{"ok":true,"verified":true}
{"ok":false,"error":"invalid_proof","message":"proof does not match"}
```

The second is the same proof with one field value changed. So what `verify`
actually answers is precisely: *"was this exact `(event_type, fields)` tuple
signed by the key this daemon is holding?"* Nothing about who is asking, and
nothing about whether they may do anything.

### Where `verify` is used

**[S]** There is exactly one consumer of a proof in the entire tree, and it is
this op. Four places reach it, all through `cli/verify_cmd.rs:13`:

| Caller | Site | Payload it verifies |
|---|---|---|
| `sahjhan verify` | CLI command, direct | whatever you pass |
| `sahjhan authed-event` | `cli/authed_event.rs:96` | the event type + fields being recorded |
| `sahjhan reseal` | `cli/authed_event.rs:144` | `"config_reseal"`, no fields |
| `sahjhan reset` | `cli/init.rs:176` | `"reset"`, no fields |

**Every one of those is a CLI command.** No socket operation requires a proof
in order to act. `sign`, `vault_*`, `enforcement_*`, and `record_event` take no
proof parameter at all — check the enum at `src/daemon/protocol.rs:16-55`.

### How sahjhan uses it, and why that matters

The pattern in all three gated commands is the same. From `cli/authed_event.rs:96`:

```rust
let verify_code = super::verify_cmd::cmd_verify(config_dir, event_type, field_strs, proof);
if verify_code != 0 {
    return verify_code;
}
// ... proceed to append to the ledger
```

The authorization decision is made **client-side, in the CLI process the agent
invoked.** The daemon is consulted only as an oracle. It never learns what the
answer will be used for, and it never decides anything.

That design is only as strong as the scarcity of proofs. Which brings us to the
findings.

### Signing works in socket mode — [E]

With `trusted-callers.toml` enforced, a manifest-listed script signs and
verifies over the socket normally (C1, C2):

```
{"ok":true,"proof":"8c4fed5c9212902900101af2ef3223dd57a38692c65573399c8532ca1bdcabba"}
{"ok":true,"verified":true}
{"ok":false,"error":"invalid_proof","message":"proof does not match"}   ← one field altered
```

The CLI is excluded from both, which is the manifest doing its job. So signing
is a live, functioning capability of socket mode, available to exactly the
callers you listed.

What it is *for* is one specific job, covered next.

---

## 2. The tamper-evidence use case, in full

This is the only job signing does in socket mode. It is worth being exact about,
because everything I got wrong earlier came from being vague here.

### The problem it solves

Trusted process **A** produces a claim. Trusted process **B** must decide
whether to act on it. Between them, the claim rests somewhere the agent can
write — a file in the workspace, a rendered artifact, a commit message, a scratch
path. **B needs to know the claim it is reading is the claim A wrote.**

That is the entire job. A signs the claim; B sends the claim and its proof to
`verify` before acting.

### Why the other mechanisms don't cover it

Three things look like they should solve this, and each fails for a different
reason:

**`trusted-callers.toml` doesn't**, because it authenticates *connections*, not
*data at rest*. B's socket connection is authenticated. The file B read is not
part of that connection and the manifest says nothing about its contents. Peer
identity tells you who is talking to the daemon right now; it cannot tell you
who wrote a file an hour ago.

**`record_event` doesn't**, because it commits. If A can simply record the claim,
there is no problem to solve — A should do that, and no proof is involved (F1).
The problem only exists when the decision to commit belongs to B. A recording it
pre-empts B's decision; the ledger is append-only, so there is no taking it back.

**The enforcement blob usually *does*, and this is the important caveat.**
`enforcement_write` / `enforcement_read` / `enforcement_update` keep an opaque
JSON object in a reserved `_`-prefixed vault namespace that `vault read` and
`vault list` refuse to serve (`src/daemon/mod.rs:494-496`, `mod.rs:551`). If A
and B are both trusted callers and the claim never needs to leave daemon memory,
that channel is strictly better than signing: the agent cannot read it, cannot
write it, and no proof is required.

**So reach for a proof only when the claim must exist somewhere the enforcement
blob cannot go.** Concretely, that means:

- the claim must be **visible to the agent** — you want it to see its own quiz
  score, or a review verdict — while remaining unalterable by it;
- the claim must land in an artifact outside the daemon's reach — a rendered
  file, a commit message, a PR body, a transcript another tool consumes;
- the claim must survive a process that isn't a trusted caller sitting in the
  middle of the chain.

If none of those apply, use the enforcement namespace and skip the proof.

### What the proof actually covers

**[S]** `build_canonical_payload` (`src/daemon/mod.rs:891-904`) builds
`event_type \0 k=v \0 k=v ...` with fields sorted by key, and
`compute_sign` (`mod.rs:879-884`) HMAC-SHA256s that with the session key.

So the proof binds **the event type and the exact set of field key/value pairs.**
Consequences worth knowing before you rely on it:

| Change to the claim | Detected? |
|---|---|
| any field **value** altered | **yes** — **[E]** `{"ok":false,"error":"invalid_proof"}` |
| a field **added** or **removed** | **yes** — the canonical payload differs |
| the **event type** changed | **yes** |
| fields **reordered** in the file | no — and correctly so; they're sorted before signing |
| anything **else in the carrier file** | **no** — the proof covers the tuple, not the file. Comments, timestamps, sibling keys are all unprotected. |
| the claim **replayed** later in the session | **no** — see below |
| **which** trusted caller signed it | **no** — see below |

### Two things it deliberately does not give you

**It is not replay protection.** **[S]** The canonical payload has no nonce, no
timestamp, no counter, so a proof for a given tuple is constant. **[E]** (A5) one
proof recorded the same event three times. If your claim is "the quiz was
passed," a proof observed once stays valid for that tuple for the daemon's
lifetime. If freshness matters, put a nonce in the fields yourself and track it.

**It does not identify the signer.** Any trusted caller's proof for a given tuple
is byte-identical, because the input is the tuple and the shared session key.
**[E]** two different manifest-listed scripts, same tuple:

```
alice -> {"ok":true,"proof":"cfe57f531fe9fc635e8f0ae582cf8b8ebc2fd82e21fcff0cf20e461cef5db951"}
bob   -> {"ok":true,"proof":"cfe57f531fe9fc635e8f0ae582cf8b8ebc2fd82e21fcff0cf20e461cef5db951"}
```

So `verify` answers *"something with socket access signed this exact tuple,"*
never *"hook A signed this."* If you need signer attribution, put the signer's
name in the fields — then it's covered by the proof.

**And its scope is one daemon lifetime, by design.** The session key lives only
in daemon memory and is regenerated at startup, which is the whole premise of
daemon mode. **[E]**:

```
proof from daemon #1: 7d78bd9f0a5e68568d21097007156c63450dd17e03039e34cc992b84bb39409b
verify against daemon #1:            {"ok":true,"verified":true}
verify SAME proof against daemon #2: {"ok":false,"error":"invalid_proof","message":"proof does not match"}
```

A proof attests within a session. It is not a persistence mechanism.

### Worked example

Both scripts are listed in `trusted-callers.toml` and invoked by absolute path.
`daemon()` is the fifteen-line client — connect, send one JSON line, read one
back, raise if `ok` is false.

```python
# grade.py — produces the claim, and cannot itself decide to commit it
fields = {"score": "5/5", "pass": "true"}
proof = daemon({"op": "sign", "event_type": "quiz_passed", "fields": fields})["proof"]
json.dump({"fields": fields, "proof": proof}, open("output/quiz-result.json", "w"))
```

```python
# gate.py — decides. The file is agent-writable, so the proof is the only thing
# here worth believing.
claim = json.load(open("output/quiz-result.json"))
try:
    daemon({"op": "verify", "event_type": "quiz_passed",
            "fields": claim["fields"], "proof": claim["proof"]})
except RuntimeError:                       # daemon answered invalid_proof
    sys.exit("refusing a quiz result that doesn't match its proof")
daemon({"op": "record_event", "event_type": "quiz_passed", "fields": claim["fields"]})
```

Note the last line: the *recording* still needs no proof. `record_event` is
authorized by `gate.py` being a listed caller (F1). The proof did one job — it
let `gate.py` believe a file it did not write.

**[E]** Untouched, then with `score` edited in the file between the two steps:

```
verify -> {"ok":true,"verified":true}
verify -> {"ok":false,"error":"invalid_proof","message":"proof does not match"}
```

### Is it used in practice?

In holtz — the one consumer repo I looked at — `compute_event_proof` is defined
at `enforcement/hooks/_common.py:276` and every call site is under `tests/`. So
that consumer currently records through `record_event` and has no live signing
path. I did not audit other consumers, and one repo not using it says nothing
about whether it should.

---

## 3. The two doors into the ledger

There are two ways an event reaches the ledger, and they are guarded by
completely different things. Conflating them is the root of the confusion.

```
DOOR 1 — the CLI                          DOOR 2 — the daemon socket
sahjhan event <type>                      {"op":"record_event", ...}
  └─ refuses if restricted                  └─ never looks at `restricted`
sahjhan authed-event <type> --proof P        guarded by: peer identity
  └─ requires restricted, verifies P         (trusted-callers.toml)
guarded by: possession of a proof
```

**[S]** Door 1's restricted check: `cli/transition.rs:567`.
**[S]** Door 2's handler, `handle_record_event` at `src/daemon/mod.rs:715-781`,
loads the config, does `config.events.get(event_type)`, calls
`validate_event_fields`, and appends. It never reads `restricted`. Its own
doc comment says so at `mod.rs:707-708`: *"Any declared event is accepted
(restricted or not) — the trust boundary is the authenticated peer."*

**[E]** Both doors, same event type, same project, one after the other:

```
── A1. 'restricted' event appended over the socket with NO proof ──
{"ok":true,"data":"1"}
[2026-08-25T21:39:55.138Z] seq=1 type=privileged_claim hash=a391997b5ecb {claim=i-was-never-authorized}
chain valid (2 events)

── A2. the same event via the plain CLI is refused ──
error: event type 'privileged_claim' is restricted. Use 'sahjhan authed-event' with a valid proof.
exit=4
```

`privileged_claim` is declared `restricted = true`. The socket took it with no
proof. The CLI refused it. **This is the single most important fact in this
document.**

---

## 4. What `restricted` actually does

**[S]** `grep -rn "\.restricted\|restricted ==\|restricted !=" src/` returns
five read sites. All five:

| # | Site | Effect |
|---|---|---|
| 1 | `config/mod.rs:258` | `validate` rejects a transition whose `emits` names a restricted event |
| 2 | `state/machine.rs:257` | runtime backstop for the same thing |
| 3 | `cli/transition.rs:567` | `sahjhan event` refuses the type |
| 4 | `cli/authed_event.rs:42` | `sahjhan authed-event` *requires* the type to be restricted |
| 5 | `lint/checks.rs:138` | L1 lint treats a producerless restricted event as fatal, not a warning |

**Nothing under `src/daemon/`.** The flag has no effect on the socket path.

So `restricted` means: *this event type may not be written by the plain CLI
command, nor by a transition emit.* It does not mean "this event requires a
proof," and it does not mean "the agent cannot write this."

---

## 5. Findings

### F1 — `record_event` writes restricted events with no proof

Covered in §3. **[E]** demonstrated twice: without a manifest (any same-uid
process) and with one (any manifest-listed script). This is deliberate — see the
doc comment at `mod.rs:707-708` — but it is the opposite of what the prose docs say.

### F2 — `sign` has no domain separation

**[S]** `Request::Sign` at `src/daemon/mod.rs:489` calls `compute_sign`
(`mod.rs:879`) on whatever string it is given. There is no check that the event
type is declared, no check that it is restricted, no namespace separating event
payloads from control-plane payloads.

**[E]** It signs an event type that does not exist:

```
── A3. 'sign' will sign an event type that does not exist in events.toml ──
da82f7a18ab44767652df2a8cb0e075233cd9643cf556d76c05b0aa38c6d64ff  exit=0
```

**[E]** And it signs the two control-plane payloads — the exact strings that
`reseal` and `reset` verify:

```
── A4. 'sign' will sign the control-plane payloads 'reset' and 'config_reseal' ──
04d25b5de4957d77359df101cc746d75418ce02925acbb9b65a84bf76beb5714
674648c9404fd2ac14c9ced7accc2cc51330f571b05639521c5ec3d0247ff68d
```

### F3 — proofs are deterministic and replayable

**[S]** `build_canonical_payload` (`src/daemon/mod.rs:891-904`) is
`event_type \0 k=v \0 k=v`, fields sorted. No nonce, no timestamp, no counter.

**[E]** Same input signed twice gives the same proof, and one proof records the
same event three times:

```
── A5. proofs are deterministic and replayable (no nonce, no timestamp) ──
proof #1: dd96f6d3d240f0981fd719b905728ebe5107bafbbc5d37dd4d5b3bc9321b97d9
proof #2: dd96f6d3d240f0981fd719b905728ebe5107bafbbc5d37dd4d5b3bc9321b97d9
IDENTICAL
recorded: privileged_claim
recorded: privileged_claim
recorded: privileged_claim
count
-----
4
```

### F4 — the mint and the verifier are the same unguarded endpoint

This is F2 and F3 combined, and it is the finding that matters.

`sign` and `verify` sit on the same socket, behind the same (absent by default)
caller check. Whoever can reach the socket can mint any proof and then satisfy
any proof-gated command. **[E]** the full chain, no manifest configured:

```
── A6. self-minted proof drives 'reset' ──
initialized. good luck.
reset. prior run archived.
exit=0
```

That is `sahjhan sign --event-type reset` piped straight into
`sahjhan reset --confirm --proof`. The ledger was archived and reinitialized.

**[I]** The inference, labeled as such: with no `trusted-callers.toml` and no
sandbox, proof-gating provides no protection against an agent with Bash, because
the credential is free to anyone who can open the socket. The gate is
implemented correctly; the credential is simply not scarce.

**[S]** What proofs *do* buy, legitimately: a proof binds a payload to the
daemon's key, so a party who trusts the signer more than the transmitter can
detect tampering. That requires socket access to be restricted — i.e. a
manifest — and requires the payload to travel through somewhere untrusted.

### F5 — a `hooks.toml` auto_record writes restricted events, with no daemon

**[S]** `validate_deep` checks that a transition's `emits` does not name a
restricted event (`config/mod.rs:258`, with a comment explaining that allowing
it "would bypass the HMAC proof"). The auto_record validation at
`config/mod.rs:626` checks only that the event type *exists*:

```rust
if !self.events.contains_key(&auto.event_type) {
    errors.push(format!("{}: auto_record.event_type '{}' is not defined in events.toml", ...));
}
```

No restricted check. And `cli/hooks_cmd.rs:176` appends it with a bare
`ledger.append`.

**[E]** A `hooks.toml` rule with `auto_record.event_type = "privileged_claim"`:

```
── B1. does validate object? ──
valid.
exit=0

── B2. hook eval, no daemon running ──
(no daemon process)
{ "decision": "allow", "auto_records": [ { "event_type": "privileged_claim",
  "fields": { "claim": "recorded-by-hook-eval-no-proof" } } ], ... }

── B3. did it land? ──
[2026-08-25T21:39:55.891Z] seq=1 type=privileged_claim hash=0361126ca662 {claim=recorded-by-hook-eval-no-proof}
chain valid (2 events)
```

`validate` blesses the semantically identical thing it rejects for transitions.
**Mitigating:** `hooks.toml` is one of the eight sealed files, so this is a
protocol-author footgun rather than a runtime escape for the agent.

### F6 — under caller auth, `reseal` and `reset` become impossible for everyone

**[E]** With a manifest enforced, a trusted hook mints a valid `config_reseal`
proof over the socket, and the CLI still refuses to spend it:

```
── C6. CLI reseal, using a proof the TRUSTED HOOK minted ──
hook-minted config_reseal proof: d356773aa147963c54ff3d11d877b1dd640d882fc9ff186d92e43c5e6df2ea35
error: caller not authenticated
exit=2

── C7. CLI reset, using a proof the TRUSTED HOOK minted ──
error: caller not authenticated
error: reset requires a valid proof from the daemon
hint: sahjhan sign --event-type reset | xargs -I{} sahjhan reset --confirm --proof {}
exit=2
```

The proof is valid. The CLI cannot present it, because `cmd_verify` opens its
own socket connection and the peer is the CLI binary, which can never be in the
manifest. And there is no `reseal` or `reset` operation on the wire for a
trusted script to use instead — see the op list in
`src/daemon/protocol.rs:16-55`.

**[I]** Recovering from a config change after caller auth is on therefore
requires stopping the daemon or removing the manifest first. The `hint` printed
by `reset` recommends a command that cannot work in this configuration.

---

## 6. The model, stated once

```
OS: socket is mode 0600            → same-uid processes only
  │
  ├─ op == "status" ──────────────→ SERVE. Exempt from both gates below.
  │
  ├─ sandbox fuse armed & tripped ─→ REFUSE (sandbox_required). Re-checked per request.
  │
  ├─ trusted-callers.toml
  │     absent  ──────────────────→ ALLOW EVERY PEER   ← the default
  │     empty   ──────────────────→ DENY EVERY PEER
  │     entries ──────────────────→ direct peer only; absolute path under the
  │                                  config dir; SHA-256 must match. Not inheritable.
  │
  └─ dispatch → sign | verify | vault_* | enforcement_* | record_event
        · no operation consumes a proof in order to act
        · record_event ignores `restricted` entirely
        · vault_* additionally consult vault.toml state policy
```

**Connection-level peer identity authorizes everything non-`status`,
uniformly.** There is no per-operation authorization inside the daemon. The
proof system authorizes nothing at the daemon; it is a client-side convention,
using an oracle that the same endpoint offers to whoever can reach it.

**[E]** The manifest-on comparison, one daemon, two callers:

| | trusted script (direct socket) | `sahjhan` CLI |
|---|---|---|
| `sign` | `{"ok":true,"proof":"8c4fed5c…"}` | `sign failed: caller not authenticated` (exit 2) |
| `verify` | `{"ok":true,"verified":true}` | exit 2 |
| `record_event` (restricted, no proof) | `{"ok":true,"data":"1"}` | no CLI surface |
| `authed-event` with a hook-minted proof | n/a | `error: caller not authenticated` (exit 2) |
| `reseal` / `reset` with a hook-minted proof | not a wire op | `error: caller not authenticated` (exit 2) |
| `status` | `{"ok":true,...}` | `{"ok":true,...}` — **exempt** |

---

## 7. Severity — which of these actually matter

Most of the alarming-sounding material above describes the **default
configuration, with no `trusted-callers.toml`**. Configuring one is the fix, and
it is the fix the design already anticipates. Sorting the findings by whether
they survive hardening:

| Finding | With a manifest configured | Severity |
|---|---|---|
| **F6** reseal/reset impossible under caller auth | **still broken** — no wire op exists, CLI can't authenticate | **operational defect.** Highest. Recovering from a legitimate config change means stopping the daemon or removing the manifest. `reset` prints a hint that cannot work. |
| **F5** auto_record writes restricted events; `validate` blesses it | **still true** — no daemon involved at all | **validation gap.** `validate_deep` rejects the identical thing for transition `emits` (`config/mod.rs:258`) but not for auto_record (`config/mod.rs:626`). Sealed config limits blast radius to protocol-author error. |
| **F1** `record_event` ignores `restricted` | still true, and **intended** (`mod.rs:707-708`) | **doc bug, not a code bug.** The design is coherent; the prose describing it is not (§8). |
| **F2** `sign` has no domain separation | mitigated — only manifest-listed scripts can sign | **latent.** Worth a domain prefix so an event payload can never collide with `reset`/`config_reseal`, but not exploitable once the manifest is on. |
| **F3** proofs are deterministic and replayable | mitigated — same | **latent**, same reasoning. |
| **F4** mint and verifier are the same free endpoint | **this is exactly what the manifest fixes** | **overstated by me.** It is a statement about the unhardened default, not an indictment of the mechanism. |

So, directly: **HMAC signing through the socket is a working, used part of the
hardened design.** The manifest is what makes proofs scarce, and with it in place
the model is coherent — peer identity authorizes, `record_event` records,
`verify` attests across process boundaries. I presented F2–F4 with more alarm
than they warrant, because I was reasoning about the default configuration while
talking about the hardened one.

The two findings that are worth your time regardless of configuration are **F6**
and **F5**. Everything else is either a documentation fix or a latent
hardening-nice-to-have.

---

## 8. Prose that is now demonstrably wrong

| Location | Claim | Why it's false |
|---|---|---|
| `README.md:315` | restricted events must "arrive as an HMAC proof" | F1 — `record_event` takes them with none |
| `README.md:353` | "the agent can't record them at all" | F4 (self-minted proof) and F5 (auto_record, no daemon) |
| `lint/checks.rs:146` | "can only be recorded by `authed-event` with an HMAC proof" | same; **this string ships to users as a lint hint** |
| `config/mod.rs:249-251`, `state/machine.rs:251-253` | guards justified as protecting "the HMAC proof that `authed-event` requires" | guards are correct; the stated premise is not |
| `daemon/auth.rs:105-113` | `pid_resolution_failed` documented as PID→script resolution failure | `NotInManifest` — a script that resolved fine and simply isn't listed — maps there too **[E]** (C9) |
| `CLAUDE.md` | no rows for `config/vault_policy.rs` / `check_vault_policy` | `vault.toml` state policy is live and enforced |

`docs/hardening.md` is excluded from this table deliberately — it is mid-edit
and I am the wrong person to grade it.

---

## Appendix A — reproduction script

Working copy at `/tmp/sje2e/repro.sh`. Self-contained; it builds three
throwaway projects under `/tmp/sjrepro`, runs every experiment above, and
cleans up its daemons. Set `SJ` to your binary if it isn't at the default path.

It proves, in order: A1 restricted-over-socket-without-proof, A2 the CLI
refusing the same, A3 signing an undeclared type, A4 signing the control-plane
payloads, A5 determinism and replay, A6 self-minted reset, B1–B3 the
auto_record path with no daemon, C1–C3 a trusted script's capabilities, C4–C7
the CLI's total exclusion under a manifest, C8 the `status` exemption, C9 an
unlisted caller being denied.

Two things to know before running it: A6 archives and reinitializes a ledger
(in its own throwaway project), and the script `pkill`s any running
`sahjhan daemon start` process on your machine at the start and end.
