# Config Integrity at Genesis

## Problem

The hooks block writes to managed dirs, the ledger is append-only and hash-chained, but the TOML config files themselves have zero integrity protection. A sufficiently motivated agent with pre-init write access can gut gates in transitions.toml. This is a hardened vault with an unlocked side door.

## Solution

`sahjhan init` hashes all five TOML config files and stores SHA-256 hashes in the genesis ledger entry. `Ledger::open()` verifies those hashes on every command. A new `sahjhan reseal` command (HMAC-restricted) records updated hashes when config legitimately changes.

## Design Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Which files to seal | All five, always | Partial coverage creates false confidence. Missing optional files get sentinel hash (SHA-256 of empty bytes). |
| Failure behavior | Hard fail with reseal escape hatch | Legitimate config evolution happens. Forcing re-init is punitive. Reseal creates auditable record. |
| Reseal authorization | HMAC-restricted (session key) | An agent that can write transitions.toml but lacks the session key can't launder changes through the ledger. |
| Verification location | Inside `Ledger::open()` | Commands that need integrity are exactly commands that open a ledger. Impossible to forget for new commands. |
| Hash storage | Individual genesis fields | Fits existing flat BTreeMap model. Per-file tamper reporting for free. |

## Genesis Entry Changes

The genesis entry's `fields` map gains 5 new keys:

| Field | Value |
|-------|-------|
| `config_seal_protocol` | SHA-256 hex of `protocol.toml` raw bytes |
| `config_seal_states` | SHA-256 hex of `states.toml` raw bytes |
| `config_seal_transitions` | SHA-256 hex of `transitions.toml` raw bytes |
| `config_seal_events` | SHA-256 hex of `events.toml` raw bytes, or SHA-256 of empty bytes if absent |
| `config_seal_renders` | SHA-256 hex of `renders.toml` raw bytes, or SHA-256 of empty bytes if absent |

Existing fields (`protocol_name`, `protocol_version`) remain unchanged.

Hash is of raw file bytes, not parsed TOML. Any byte-level change (including whitespace/comments) is a tamper signal.

## Verification on Ledger Open

`Ledger::open()` gains a new parameter: `config_dir: Option<&Path>`.

- `Some(path)`: reads genesis entry (seq 0), extracts `config_seal_*` fields, hashes current config files, compares. Mismatch returns `LedgerError::ConfigIntegrityViolation` listing which files changed.
- `None`: skips verification. For internal/test use only.

**Effective seal resolution**: scan ledger from end backward for most recent `config_reseal` event. If found, those hashes are authoritative. Otherwise, genesis hashes are used.

## Reseal Command

New CLI command: `sahjhan reseal`

- Requires HMAC session key (same mechanism as `authed-event`)
- Reads current config files, computes hashes
- Appends `config_reseal` event with the same 5 `config_seal_*` fields
- Hardcoded event type in engine (not declared in user's events.toml) to prevent removal
- Prints which files changed compared to previous seal

## Error Behavior

On integrity violation:

```
error: config integrity violation — the following files have been modified since the last seal:
  - transitions.toml (expected: a1b2c3..., found: d4e5f6...)
  - events.toml (expected: 789abc..., found: def012...)

Run 'sahjhan reseal' with a valid session key to update the seal,
or 'sahjhan init' to start a new ledger.
```

Exit code: new `EXIT_INTEGRITY_ERROR` constant.

## Backward Compatibility

Existing ledgers without `config_seal_*` fields in genesis and no `config_reseal` events: verification is skipped silently. Old ledgers continue to work. Users can run `sahjhan reseal` on an existing ledger to opt in.

## What Doesn't Change

- `sahjhan validate` -- no ledger, no verification needed
- `sahjhan init` -- creates the seal, doesn't verify one
- Ledger hash chain integrity -- orthogonal, unchanged
- The `fields` BTreeMap type -- still `BTreeMap<String, String>`

## Files Modified

| File | Changes |
|------|---------|
| `config/mod.rs` | New `compute_config_seals(dir) -> BTreeMap<String,String>` utility |
| `ledger/chain.rs` | `init()` accepts config dir, hashes files into genesis. `open()` accepts optional config dir, verifies seal. New `find_effective_seal()` helper. |
| `ledger/entry.rs` | New `LedgerError::ConfigIntegrityViolation` variant |
| `cli/init.rs` | `cmd_init` passes config dir to `Ledger::init()` |
| `cli/commands.rs` | `open_targeted()` and helpers pass config dir to `Ledger::open()`. New `EXIT_INTEGRITY_ERROR`. |
| `cli/authed_event.rs` | New `cmd_reseal()` function |
| `main.rs` | New `Commands::Reseal` variant, dispatch to `cmd_reseal` |

## Test Plan

1. **Happy path**: init, verify all 5 hashes in genesis entry, open ledger succeeds
2. **Tamper detection**: init, modify transitions.toml, open ledger fails with correct error
3. **Missing optional file**: init without events.toml, verify sentinel hash, open succeeds
4. **Reseal flow**: init, modify config, reseal with valid key, open succeeds with new hashes
5. **Reseal without auth**: attempt reseal without session key, verify rejection
6. **Backward compat**: open legacy ledger (no seal fields), verify no error
7. **Multiple reseals**: verify most recent reseal is authoritative
