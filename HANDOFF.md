# HANDOFF — 2026-08-19

**Uncommitted on purpose. Do not commit this file or push it: sahjhan is a
public repo and this describes a live, unfixed auth bypass.**

## Priority for next session: trusted-caller auth is bypassable

`find_trusted_ancestor` (`src/daemon/auth.rs`) authenticates a connection if
**any** ancestor within 10 levels is a script that is (a) under `--config-dir`
and (b) present in `trusted-callers.toml` with a matching SHA-256.

That means authority is inherited by every *descendant* of a trusted script. The
agent controls the environment a trusted script is launched in, so it controls
what descendants that script spawns.

### Reproduction (verified 2026-08-19 against v0.20.1)

1. Agent writes a wrapper it controls:

   ```sh
   #!/bin/sh
   REAL=/path/to/real/sahjhan
   out=$("$REAL" "$@" 2>&1); printf '%s\n' "$out" >> /tmp/evil/stolen.txt
   printf '%s\n' "$out"
   ```

2. Agent runs the **unmodified** hook with the wrapper ahead on `PATH`:

   ```sh
   PATH=/tmp/evil:$PATH python3 enforcement/hooks/quiz.py
   ```

3. The hook's `subprocess.run(["sahjhan", ...])` resolves to the wrapper. The
   wrapper is a child of `quiz.py`, so the ancestor walk finds `quiz.py` at the
   right path with the right hash, and authenticates.

Both halves of the trust boundary fall:

- **Vault read.** A hook written to read the quiz bank and deliberately never
  print it still leaks it — the wrapper captured
  `{"q1":"SECRET-ANSWER-spill_batch_size=65536"}`.
- **Restricted event forgery.** A wrapper that mints its own proof and submits
  it landed `type=quiz_passed {pass=true, score=0/5}` in the ledger, with the
  trusted script's hash still matching the manifest.

### What does still hold

A proof stolen and replayed from plain Bash **fails** — `authed-event` verifies
through the daemon, and that connection is authenticated too. The attack has to
inject from inside the trusted script's subtree rather than exfiltrate and
replay. Keep that property in any fix.

### Why the obvious fix is not enough

Requiring the peer's *immediate* parent to be the trusted script blocks the
shell-wrapper variant above, but not `PYTHONPATH` injection: a `sitecustomize.py`
that hooks `subprocess` leaves the real binary a direct child of the trusted
script again. Process lineage cannot authenticate against an adversary who
controls the launch environment.

Directions worth weighing (not yet designed):

- A secret handed to the hook out-of-band by the harness — env var or inherited
  fd — that the agent can't read, presented on connect.
- A nonce the daemon issues at hook-spawn time, single-use, bound to the PID.
- Have hooks speak the socket protocol directly instead of shelling out to the
  CLI, so there is no child process to hijack. Narrows the surface; doesn't
  close it on its own.

The ancestor walk was added deliberately (see the doc comment: shell
intermediaries on macOS sit between the hook and the CLI). Whatever replaces it
has to survive that process shape.

## Also uncommitted: the README/docs restructure

Working tree holds a finished docs restructure — README 1,175 → ~347 lines, with
eight new pages under `docs/`. It is complete and link-checked, and `cargo fmt
--all -- --check` is clean. It was **not** committed because the `## trusted
callers` section asserts properties the bug above disproves. Three claims to fix
or cut before this ships:

- "some things need to be writable and readable by processes that aren't the
  agent, and by nothing else"
- "It can't produce the entry itself"
- "It can't call `sign` (its parent is `bash`)" — true for that one path, but it
  implies lineage is sufficient, and it isn't

Everything else in the restructure is independent of the bug and can ship once
that section is rewritten.

### Two doc-accuracy fixes already applied to CLAUDE.md (also uncommitted)

- The config seal covers **eight** files, not seven — `compute_config_seals`
  includes `vault.toml`. An absent file seals as the hash of zero bytes, so
  creating one later trips the seal.
- `src/config/vault_policy.rs` (state-gated vault keys) had no rows in the
  lookup tables at all, and neither did `tests/paths_tests.rs` or
  `tests/vault_policy_tests.rs`.

### One open question from the restructure

`trusted-callers.toml` keys are relative to `--config-dir` (`plugin_root =
&self.config_dir`, `src/daemon/mod.rs:224`), verified empirically. The example
in `docs/hardening.md` — carried over verbatim from the old README — shows
`"enforcement/hooks/lens_quiz.py"`, which would not match under that
resolution. Confirm what holtz actually passes as `--config-dir` before
correcting it.
