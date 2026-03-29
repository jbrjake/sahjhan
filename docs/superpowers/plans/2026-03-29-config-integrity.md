# Config Integrity at Genesis — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Hash-seal all five TOML config files into the genesis ledger entry and verify on every `Ledger::open()` call, with HMAC-authenticated reseal for legitimate config changes.

**Architecture:** Add `compute_config_seals()` to hash config files, `init_with_seals()` to write seals into genesis, and `verify_config_seal()` to check on open. CLI helpers call verification automatically. A new `reseal` command uses HMAC auth to update seals.

**Tech Stack:** Rust, SHA-256 (sha2 crate), HMAC-SHA256 (hmac crate), hex encoding (hex crate)

---

## File Structure

| File | Responsibility |
|------|---------------|
| `src/config/mod.rs` | New `compute_config_seals()` function |
| `src/ledger/entry.rs` | New `ConfigIntegrityViolation` error variant |
| `src/ledger/chain.rs` | New `init_with_seals()`, `find_effective_seal()`, `verify_config_seal()` methods |
| `src/cli/commands.rs` | Modified `open_ledger()`, `open_targeted_ledger()` to verify seals |
| `src/cli/init.rs` | Modified `cmd_init()` to pass config seals; `cmd_reset()` updated for new `open_ledger` signature |
| `src/cli/authed_event.rs` | New `cmd_reseal()` function |
| `src/main.rs` | New `Commands::Reseal` variant |
| `tests/config_integrity_tests.rs` | All unit and integration tests for this feature |

---

### Task 1: compute_config_seals()

**Files:**
- Modify: `src/config/mod.rs`
- Test: `tests/config_integrity_tests.rs`

- [ ] **Step 1: Write the failing test**

Create `tests/config_integrity_tests.rs`:

```rust
// tests/config_integrity_tests.rs
//
// Tests for config integrity sealing, verification, and reseal.

use std::collections::BTreeMap;
use tempfile::tempdir;

#[test]
fn test_compute_config_seals_all_files_present() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("protocol.toml"), b"[protocol]\nname = \"test\"\n").unwrap();
    std::fs::write(dir.path().join("states.toml"), b"[states.idle]\nlabel = \"Idle\"\n").unwrap();
    std::fs::write(dir.path().join("transitions.toml"), b"[[transitions]]\nfrom = \"idle\"\n").unwrap();
    std::fs::write(dir.path().join("events.toml"), b"[events.e1]\ndescription = \"E1\"\n").unwrap();
    std::fs::write(dir.path().join("renders.toml"), b"[[renders]]\ntarget = \"out.md\"\n").unwrap();

    let seals = sahjhan::config::compute_config_seals(dir.path());

    assert_eq!(seals.len(), 5);
    assert!(seals.contains_key("config_seal_protocol"));
    assert!(seals.contains_key("config_seal_states"));
    assert!(seals.contains_key("config_seal_transitions"));
    assert!(seals.contains_key("config_seal_events"));
    assert!(seals.contains_key("config_seal_renders"));

    // Each value should be a 64-char hex SHA-256
    for (_key, hash) in &seals {
        assert_eq!(hash.len(), 64);
        assert!(hash.chars().all(|c| c.is_ascii_hexdigit()));
    }
}

#[test]
fn test_compute_config_seals_optional_files_missing() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("protocol.toml"), b"proto").unwrap();
    std::fs::write(dir.path().join("states.toml"), b"states").unwrap();
    std::fs::write(dir.path().join("transitions.toml"), b"trans").unwrap();
    // events.toml and renders.toml intentionally missing

    let seals = sahjhan::config::compute_config_seals(dir.path());

    assert_eq!(seals.len(), 5);
    // Missing files should get the SHA-256 of empty bytes
    let empty_hash = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
    assert_eq!(seals["config_seal_events"], empty_hash);
    assert_eq!(seals["config_seal_renders"], empty_hash);
    // Present files should NOT be the empty hash
    assert_ne!(seals["config_seal_protocol"], empty_hash);
}

#[test]
fn test_compute_config_seals_deterministic() {
    let dir = tempdir().unwrap();
    std::fs::write(dir.path().join("protocol.toml"), b"content").unwrap();
    std::fs::write(dir.path().join("states.toml"), b"content2").unwrap();
    std::fs::write(dir.path().join("transitions.toml"), b"content3").unwrap();

    let seals1 = sahjhan::config::compute_config_seals(dir.path());
    let seals2 = sahjhan::config::compute_config_seals(dir.path());
    assert_eq!(seals1, seals2);
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test config_integrity_tests -- test_compute_config_seals 2>&1 | head -20`
Expected: compilation error — `compute_config_seals` does not exist

- [ ] **Step 3: Implement compute_config_seals**

Add to the end of `src/config/mod.rs` (before the closing of the file), after the `impl ProtocolConfig` block:

```rust
/// Compute SHA-256 hashes of all five TOML config files.
///
/// Missing optional files (events.toml, renders.toml) hash as empty bytes.
/// Returns a BTreeMap with keys: config_seal_protocol, config_seal_states,
/// config_seal_transitions, config_seal_events, config_seal_renders.
pub fn compute_config_seals(dir: &Path) -> BTreeMap<String, String> {
    use sha2::{Digest, Sha256};

    let files = [
        ("config_seal_protocol", "protocol.toml"),
        ("config_seal_states", "states.toml"),
        ("config_seal_transitions", "transitions.toml"),
        ("config_seal_events", "events.toml"),
        ("config_seal_renders", "renders.toml"),
    ];

    let mut seals = BTreeMap::new();
    for (key, filename) in &files {
        let path = dir.join(filename);
        let bytes = std::fs::read(&path).unwrap_or_default();
        let hash = hex::encode(Sha256::digest(&bytes));
        seals.insert(key.to_string(), hash);
    }
    seals
}
```

Also add `use std::collections::BTreeMap;` to the imports at the top of `config/mod.rs` if not already present.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test config_integrity_tests -- test_compute_config_seals -v`
Expected: all 3 tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/config/mod.rs tests/config_integrity_tests.rs
git commit -m "feat: add compute_config_seals() for config integrity hashing"
```

---

### Task 2: ConfigIntegrityViolation Error Variant

**Files:**
- Modify: `src/ledger/entry.rs`

- [ ] **Step 1: Add the error variant**

In `src/ledger/entry.rs`, add a new variant to `LedgerError` after the `LockTimeout` variant:

```rust
    #[error("config integrity violation:\n{}", details.join("\n"))]
    ConfigIntegrityViolation { details: Vec<String> },
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: successful build

- [ ] **Step 3: Commit**

```bash
git add src/ledger/entry.rs
git commit -m "feat: add ConfigIntegrityViolation error variant to LedgerError"
```

---

### Task 3: init_with_seals() and Modified create_genesis

**Files:**
- Modify: `src/ledger/chain.rs`
- Test: `tests/config_integrity_tests.rs`

- [ ] **Step 1: Write the failing test**

Append to `tests/config_integrity_tests.rs`:

```rust
use sahjhan::ledger::chain::Ledger;

#[test]
fn test_init_with_seals_stores_hashes_in_genesis() {
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");

    let mut seals = BTreeMap::new();
    seals.insert("config_seal_protocol".to_string(), "aaa111".to_string());
    seals.insert("config_seal_states".to_string(), "bbb222".to_string());
    seals.insert("config_seal_transitions".to_string(), "ccc333".to_string());
    seals.insert("config_seal_events".to_string(), "ddd444".to_string());
    seals.insert("config_seal_renders".to_string(), "eee555".to_string());

    let ledger = Ledger::init_with_seals(&ledger_path, "test", "1.0.0", seals).unwrap();

    let genesis = &ledger.entries()[0];
    assert_eq!(genesis.fields.get("config_seal_protocol").unwrap(), "aaa111");
    assert_eq!(genesis.fields.get("config_seal_states").unwrap(), "bbb222");
    assert_eq!(genesis.fields.get("config_seal_transitions").unwrap(), "ccc333");
    assert_eq!(genesis.fields.get("config_seal_events").unwrap(), "ddd444");
    assert_eq!(genesis.fields.get("config_seal_renders").unwrap(), "eee555");
    // Original fields still present
    assert_eq!(genesis.fields.get("protocol_name").unwrap(), "test");
    assert_eq!(genesis.fields.get("protocol_version").unwrap(), "1.0.0");
}

#[test]
fn test_init_without_seals_unchanged() {
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");

    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    let genesis = &ledger.entries()[0];
    assert_eq!(genesis.fields.len(), 2); // Only protocol_name and protocol_version
    assert!(!genesis.fields.contains_key("config_seal_protocol"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test config_integrity_tests -- test_init_with_seals -v 2>&1 | head -10`
Expected: compilation error — `init_with_seals` does not exist

- [ ] **Step 3: Implement init_with_seals and modify create_genesis**

In `src/ledger/chain.rs`, modify `create_genesis` to accept extra fields:

```rust
fn create_genesis(
    protocol_name: &str,
    protocol_version: &str,
    extra_fields: BTreeMap<String, String>,
) -> LedgerEntry {
    let mut nonce = [0u8; 32];
    getrandom::getrandom(&mut nonce).expect("CSPRNG failed");
    let prev = hex::encode(nonce);

    let protocol = format!("{}/{}", protocol_name, protocol_version);

    let mut fields = BTreeMap::new();
    fields.insert("protocol_name".to_string(), protocol_name.to_string());
    fields.insert("protocol_version".to_string(), protocol_version.to_string());
    fields.extend(extra_fields);

    LedgerEntry::new(0, prev, "genesis", ENGINE_NAME, &protocol, fields)
}
```

Update the existing `init()` to pass empty extra fields:

```rust
    // [ledger-init]
    pub fn init(
        path: &Path,
        protocol_name: &str,
        protocol_version: &str,
    ) -> Result<Self, LedgerError> {
        let genesis = create_genesis(protocol_name, protocol_version, BTreeMap::new());
        // ... rest unchanged
    }
```

Add the new `init_with_seals()` method right after `init()`:

```rust
    /// Create a new ledger at `path` with a genesis entry that includes
    /// config integrity seals. The `config_seals` map is merged into the
    /// genesis entry's fields alongside protocol_name and protocol_version.
    pub fn init_with_seals(
        path: &Path,
        protocol_name: &str,
        protocol_version: &str,
        config_seals: BTreeMap<String, String>,
    ) -> Result<Self, LedgerError> {
        let genesis = create_genesis(protocol_name, protocol_version, config_seals);

        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
        lock_exclusive_with_timeout(&file, path)?;
        writeln!(file, "{}", genesis.to_jsonl())?;
        file.unlock()?;

        let engine = genesis.engine.clone();
        let protocol = genesis.protocol.clone();

        Ok(Ledger {
            path: path.to_path_buf(),
            entries: vec![genesis],
            engine,
            protocol,
        })
    }
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test config_integrity_tests -- test_init 2>&1`
Expected: both init tests PASS

- [ ] **Step 5: Run full test suite to verify no regressions**

Run: `cargo test 2>&1 | tail -5`
Expected: all existing tests PASS (create_genesis signature change is internal)

- [ ] **Step 6: Commit**

```bash
git add src/ledger/chain.rs tests/config_integrity_tests.rs
git commit -m "feat: add Ledger::init_with_seals() for config integrity sealing"
```

---

### Task 4: find_effective_seal() and verify_config_seal()

**Files:**
- Modify: `src/ledger/chain.rs`
- Test: `tests/config_integrity_tests.rs`

- [ ] **Step 1: Write the failing tests**

Append to `tests/config_integrity_tests.rs`:

```rust
#[test]
fn test_find_effective_seal_from_genesis() {
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");

    let mut seals = BTreeMap::new();
    seals.insert("config_seal_protocol".to_string(), "aaa".to_string());
    seals.insert("config_seal_states".to_string(), "bbb".to_string());

    let ledger = Ledger::init_with_seals(&ledger_path, "test", "1.0.0", seals).unwrap();

    let effective = ledger.find_effective_seal().unwrap();
    assert_eq!(effective.get("config_seal_protocol").unwrap(), "aaa");
    assert_eq!(effective.get("config_seal_states").unwrap(), "bbb");
}

#[test]
fn test_find_effective_seal_legacy_ledger_returns_none() {
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");

    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    assert!(ledger.find_effective_seal().is_none());
}

#[test]
fn test_find_effective_seal_prefers_reseal_over_genesis() {
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.jsonl");

    let mut seals = BTreeMap::new();
    seals.insert("config_seal_protocol".to_string(), "old_hash".to_string());

    let mut ledger = Ledger::init_with_seals(&ledger_path, "test", "1.0.0", seals).unwrap();

    // Append a config_reseal event with new hashes
    let mut reseal_fields = BTreeMap::new();
    reseal_fields.insert("config_seal_protocol".to_string(), "new_hash".to_string());
    ledger.append("config_reseal", reseal_fields).unwrap();

    let effective = ledger.find_effective_seal().unwrap();
    assert_eq!(effective.get("config_seal_protocol").unwrap(), "new_hash");
}

#[test]
fn test_verify_config_seal_happy_path() {
    let config_dir = tempdir().unwrap();
    std::fs::write(config_dir.path().join("protocol.toml"), b"proto content").unwrap();
    std::fs::write(config_dir.path().join("states.toml"), b"states content").unwrap();
    std::fs::write(config_dir.path().join("transitions.toml"), b"trans content").unwrap();

    let seals = sahjhan::config::compute_config_seals(config_dir.path());

    let ledger_dir = tempdir().unwrap();
    let ledger_path = ledger_dir.path().join("ledger.jsonl");
    let ledger = Ledger::init_with_seals(&ledger_path, "test", "1.0.0", seals).unwrap();

    // Files unchanged — should pass
    assert!(ledger.verify_config_seal(config_dir.path()).is_ok());
}

#[test]
fn test_verify_config_seal_detects_tamper() {
    let config_dir = tempdir().unwrap();
    std::fs::write(config_dir.path().join("protocol.toml"), b"proto content").unwrap();
    std::fs::write(config_dir.path().join("states.toml"), b"states content").unwrap();
    std::fs::write(config_dir.path().join("transitions.toml"), b"trans content").unwrap();

    let seals = sahjhan::config::compute_config_seals(config_dir.path());

    let ledger_dir = tempdir().unwrap();
    let ledger_path = ledger_dir.path().join("ledger.jsonl");
    let ledger = Ledger::init_with_seals(&ledger_path, "test", "1.0.0", seals).unwrap();

    // Tamper with transitions.toml
    std::fs::write(config_dir.path().join("transitions.toml"), b"TAMPERED").unwrap();

    let err = ledger.verify_config_seal(config_dir.path()).unwrap_err();
    let msg = format!("{}", err);
    assert!(msg.contains("config integrity violation"));
    assert!(msg.contains("transitions"));
}

#[test]
fn test_verify_config_seal_skips_legacy_ledger() {
    let config_dir = tempdir().unwrap();
    std::fs::write(config_dir.path().join("protocol.toml"), b"proto").unwrap();
    std::fs::write(config_dir.path().join("states.toml"), b"states").unwrap();
    std::fs::write(config_dir.path().join("transitions.toml"), b"trans").unwrap();

    let ledger_dir = tempdir().unwrap();
    let ledger_path = ledger_dir.path().join("ledger.jsonl");
    // Legacy ledger — no seals
    let ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Should pass silently even though config exists
    assert!(ledger.verify_config_seal(config_dir.path()).is_ok());
}

#[test]
fn test_verify_config_seal_after_reseal() {
    let config_dir = tempdir().unwrap();
    std::fs::write(config_dir.path().join("protocol.toml"), b"proto v1").unwrap();
    std::fs::write(config_dir.path().join("states.toml"), b"states v1").unwrap();
    std::fs::write(config_dir.path().join("transitions.toml"), b"trans v1").unwrap();

    let seals_v1 = sahjhan::config::compute_config_seals(config_dir.path());

    let ledger_dir = tempdir().unwrap();
    let ledger_path = ledger_dir.path().join("ledger.jsonl");
    let mut ledger = Ledger::init_with_seals(&ledger_path, "test", "1.0.0", seals_v1).unwrap();

    // Modify config
    std::fs::write(config_dir.path().join("transitions.toml"), b"trans v2").unwrap();

    // Verify should fail now
    assert!(ledger.verify_config_seal(config_dir.path()).is_err());

    // Reseal with new hashes
    let seals_v2 = sahjhan::config::compute_config_seals(config_dir.path());
    ledger.append("config_reseal", seals_v2).unwrap();

    // Now verify should pass
    assert!(ledger.verify_config_seal(config_dir.path()).is_ok());
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test config_integrity_tests -- test_find_effective_seal 2>&1 | head -10`
Expected: compilation error — `find_effective_seal` does not exist

- [ ] **Step 3: Implement find_effective_seal and verify_config_seal**

Add to `src/ledger/chain.rs` in the `impl Ledger` block, in the Accessors section:

```rust
    // [find-effective-seal]
    /// Find the effective config seal: most recent `config_reseal` event,
    /// or genesis entry seals. Returns `None` for legacy ledgers without seals.
    pub fn find_effective_seal(&self) -> Option<BTreeMap<String, String>> {
        // Scan from end for most recent config_reseal event
        for entry in self.entries.iter().rev() {
            if entry.event_type == "config_reseal" {
                let seals: BTreeMap<String, String> = entry
                    .fields
                    .iter()
                    .filter(|(k, _)| k.starts_with("config_seal_"))
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                if !seals.is_empty() {
                    return Some(seals);
                }
            }
        }

        // Fall back to genesis entry
        if self.entries.is_empty() {
            return None;
        }
        let genesis = &self.entries[0];
        let seals: BTreeMap<String, String> = genesis
            .fields
            .iter()
            .filter(|(k, _)| k.starts_with("config_seal_"))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();

        if seals.is_empty() {
            None
        } else {
            Some(seals)
        }
    }

    // [verify-config-seal]
    /// Verify that current config files match the sealed hashes.
    ///
    /// Returns `Ok(())` if:
    /// - The ledger has no config seals (legacy ledger — backward compatible)
    /// - All config file hashes match the sealed values
    ///
    /// Returns `Err(ConfigIntegrityViolation)` if any file has been modified.
    pub fn verify_config_seal(&self, config_dir: &Path) -> Result<(), LedgerError> {
        let sealed = match self.find_effective_seal() {
            Some(s) => s,
            None => return Ok(()), // Legacy ledger — skip verification
        };

        let current = crate::config::compute_config_seals(config_dir);

        let mut mismatches = Vec::new();
        for (key, expected) in &sealed {
            if let Some(actual) = current.get(key) {
                if actual != expected {
                    let filename = key.strip_prefix("config_seal_").unwrap_or(key);
                    mismatches.push(format!(
                        "  - {}.toml (expected: {}..., found: {}...)",
                        filename,
                        &expected[..12.min(expected.len())],
                        &actual[..12.min(actual.len())],
                    ));
                }
            }
        }

        if mismatches.is_empty() {
            Ok(())
        } else {
            Err(LedgerError::ConfigIntegrityViolation {
                details: mismatches,
            })
        }
    }
```

Update the `// ## Index` comment at the top of `chain.rs` to include the new methods:

```
// - [find-effective-seal]    Ledger::find_effective_seal() — find effective config seal (reseal or genesis)
// - [verify-config-seal]     Ledger::verify_config_seal()  — verify config files match sealed hashes
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test config_integrity_tests -v 2>&1`
Expected: all tests PASS

- [ ] **Step 5: Commit**

```bash
git add src/ledger/chain.rs tests/config_integrity_tests.rs
git commit -m "feat: add find_effective_seal() and verify_config_seal() to Ledger"
```

---

### Task 5: Wire Up cmd_init to Use init_with_seals

**Files:**
- Modify: `src/cli/init.rs`

- [ ] **Step 1: Modify cmd_init to compute and pass config seals**

In `src/cli/init.rs`, replace the `Ledger::init` call (lines 90-100) with:

```rust
    // Compute config integrity seals
    let config_seals = crate::config::compute_config_seals(&config_path);

    // Initialize ledger with genesis block (including config seals)
    let _ledger = match crate::ledger::chain::Ledger::init_with_seals(
        &lp,
        &config.protocol.name,
        &config.protocol.version,
        config_seals,
    ) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: cannot initialize ledger: {}", e);
            return EXIT_INTEGRITY_ERROR;
        }
    };
```

- [ ] **Step 2: Verify it compiles and tests pass**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests PASS

- [ ] **Step 3: Commit**

```bash
git add src/cli/init.rs
git commit -m "feat: cmd_init now seals config file hashes into genesis"
```

---

### Task 6: Wire Up CLI Helpers to Verify Config Seals

**Files:**
- Modify: `src/cli/commands.rs`
- Modify: `src/cli/transition.rs`
- Modify: `src/cli/status.rs`
- Modify: `src/cli/log.rs`
- Modify: `src/cli/render.rs`
- Modify: `src/cli/authed_event.rs`
- Modify: `src/cli/init.rs`
- Modify: `src/cli/ledger.rs`

This task modifies the CLI helpers `open_ledger` and `open_targeted_ledger` to accept a `config_dir` parameter and verify config seals after opening. Then updates all callers.

- [ ] **Step 1: Modify open_ledger in commands.rs**

Replace the `open_ledger` function:

```rust
// [open-ledger]
pub(crate) fn open_ledger(data_dir: &Path, config_dir: &Path) -> Result<Ledger, (i32, String)> {
    let ledger = Ledger::open(&ledger_path(data_dir))
        .map_err(|e| (EXIT_INTEGRITY_ERROR, format!("Cannot open ledger: {}", e)))?;
    ledger.verify_config_seal(config_dir).map_err(|e| {
        (
            EXIT_INTEGRITY_ERROR,
            format!(
                "{}\n\nRun 'sahjhan reseal' with a valid session key to update the seal,\nor 'sahjhan init' to start a new ledger.",
                e
            ),
        )
    })?;
    Ok(ledger)
}
```

- [ ] **Step 2: Modify open_targeted_ledger in commands.rs**

Replace the `open_targeted_ledger` function:

```rust
// [open-targeted]
pub(crate) fn open_targeted_ledger(
    config: &ProtocolConfig,
    targeting: &LedgerTargeting,
    config_dir: &Path,
) -> Result<(Ledger, Option<LedgerMode>), (i32, String)> {
    let (path, mode) = resolve_ledger_from_targeting(config, targeting)?;
    let ledger = Ledger::open(&path)
        .map_err(|e| (EXIT_INTEGRITY_ERROR, format!("Cannot open ledger: {}", e)))?;
    ledger.verify_config_seal(config_dir).map_err(|e| {
        (
            EXIT_INTEGRITY_ERROR,
            format!(
                "{}\n\nRun 'sahjhan reseal' with a valid session key to update the seal,\nor 'sahjhan init' to start a new ledger.",
                e
            ),
        )
    })?;
    Ok((ledger, mode))
}
```

- [ ] **Step 3: Update callers in transition.rs**

All three functions (`cmd_transition`, `cmd_gate_check`, `cmd_event`) have this pattern:

```rust
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) { ... };
    let (ledger, mode) = match open_targeted_ledger(&config, targeting) { ... };
```

Change each `open_targeted_ledger(&config, targeting)` to `open_targeted_ledger(&config, targeting, &config_path)`.

There are 3 call sites in transition.rs (lines ~46, ~185, ~470).

- [ ] **Step 4: Update callers in status.rs**

There are 3 call sites (lines ~40, ~179, ~248). Each has `config_path` available. Change each `open_targeted_ledger(&config, targeting)` to `open_targeted_ledger(&config, targeting, &config_path)`.

- [ ] **Step 5: Update callers in log.rs**

There are 3 call sites (lines ~31, ~58, ~93). Each has `config_path` available. Change each `open_targeted_ledger(&config, targeting)` to `open_targeted_ledger(&config, targeting, &config_path)`.

- [ ] **Step 6: Update callers in render.rs**

There are 2 call sites (lines ~37, ~101). Each has `config_path` available. Change each `open_targeted_ledger(&config, targeting)` to `open_targeted_ledger(&config, targeting, &config_path)`.

- [ ] **Step 7: Update caller in authed_event.rs**

There is 1 call site (line ~97). It has `config_path` available. Change `open_targeted_ledger(&config, targeting)` to `open_targeted_ledger(&config, targeting, &config_path)`.

- [ ] **Step 8: Update caller in init.rs (cmd_reset)**

The `cmd_reset` function calls `open_ledger(&data_dir)` at line ~189. It has `config_path` available. Change to `open_ledger(&data_dir, &config_path)`.

- [ ] **Step 9: Update direct Ledger::open calls in cli/ledger.rs**

The `cmd_ledger_verify` (line ~315) and `cmd_ledger_checkpoint` (line ~372) functions call `Ledger::open()` directly. These have `config_path` available. Add `verify_config_seal` after each:

For `cmd_ledger_verify`, after `let ledger = match Ledger::open(&ledger_file) { ... };`:

```rust
    if let Err(e) = ledger.verify_config_seal(&config_path) {
        eprintln!(
            "warning: {}\n\nRun 'sahjhan reseal' to update the seal.",
            e
        );
        // Don't fail — the user explicitly asked to verify the chain, show the result
    }
```

For `cmd_ledger_checkpoint`, after `let mut ledger = match Ledger::open(&ledger_file) { ... };`:

```rust
    if let Err(e) = ledger.verify_config_seal(&config_path) {
        eprintln!("{}\n\nRun 'sahjhan reseal' with a valid session key to update the seal.", e);
        return EXIT_INTEGRITY_ERROR;
    }
```

- [ ] **Step 10: Verify everything compiles and tests pass**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests PASS

- [ ] **Step 11: Commit**

```bash
git add src/cli/commands.rs src/cli/transition.rs src/cli/status.rs src/cli/log.rs src/cli/render.rs src/cli/authed_event.rs src/cli/init.rs src/cli/ledger.rs
git commit -m "feat: wire config seal verification into all CLI ledger-opening paths"
```

---

### Task 7: Add cmd_reseal Command

**Files:**
- Modify: `src/cli/authed_event.rs`
- Modify: `src/main.rs`
- Test: `tests/config_integrity_tests.rs`

- [ ] **Step 1: Write the failing integration test**

Append to `tests/config_integrity_tests.rs`:

```rust
use assert_cmd::Command;
use predicates::prelude::*;

/// Set up a minimal config dir with all files, run init.
fn setup_sealed_dir() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        r#"
[protocol]
name = "test-seal"
version = "1.0.0"
description = "Seal test"

[paths]
managed = ["output"]
data_dir = "output/.sahjhan"
render_dir = "output"
"#,
    )
    .unwrap();

    std::fs::write(
        config_dir.join("states.toml"),
        "[states.idle]\nlabel = \"Idle\"\ninitial = true\n",
    )
    .unwrap();

    std::fs::write(
        config_dir.join("transitions.toml"),
        "[[transitions]]\nfrom = \"idle\"\nto = \"idle\"\ncommand = \"noop\"\ngates = []\n",
    )
    .unwrap();

    std::fs::create_dir_all(dir.path().join("output")).unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "init"])
        .current_dir(dir.path())
        .assert()
        .success();

    dir
}

#[test]
fn test_cli_tamper_detection_blocks_status() {
    let dir = setup_sealed_dir();

    // Status should work before tampering
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Tamper with transitions.toml
    std::fs::write(
        dir.path().join("enforcement/transitions.toml"),
        "[[transitions]]\nfrom = \"idle\"\nto = \"idle\"\ncommand = \"noop\"\ngates = []\n# tampered\n",
    )
    .unwrap();

    // Status should now fail
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("config integrity violation"))
        .stderr(predicate::str::contains("transitions"));
}

#[test]
fn test_cli_reseal_requires_proof() {
    let dir = setup_sealed_dir();

    // Tamper so we need to reseal
    std::fs::write(
        dir.path().join("enforcement/transitions.toml"),
        "[[transitions]]\nfrom = \"idle\"\nto = \"idle\"\ncommand = \"noop\"\ngates = []\n# v2\n",
    )
    .unwrap();

    // Reseal without proof should fail
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "reseal", "--proof", "bad"])
        .current_dir(dir.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid proof"));
}

#[test]
fn test_cli_reseal_with_valid_proof_succeeds() {
    let dir = setup_sealed_dir();

    // Tamper
    std::fs::write(
        dir.path().join("enforcement/transitions.toml"),
        "[[transitions]]\nfrom = \"idle\"\nto = \"idle\"\ncommand = \"noop\"\ngates = []\n# v2\n",
    )
    .unwrap();

    // Read session key and compute HMAC proof
    let key = std::fs::read(dir.path().join("output/.sahjhan/session.key")).unwrap();
    let payload = "config_reseal";  // event_type only, no fields in HMAC payload
    let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(&key).unwrap();
    hmac::Mac::update(&mut mac, payload.as_bytes());
    let proof = hex::encode(hmac::Mac::finalize(mac).into_bytes());

    // Reseal with valid proof
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "reseal", "--proof", &proof])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("resealed"));

    // Status should now work again
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .assert()
        .success();
}

#[test]
fn test_cli_backward_compat_legacy_ledger() {
    // Manually create a legacy ledger (no seals) and verify commands still work
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();
    let data_dir = dir.path().join("output/.sahjhan");
    std::fs::create_dir_all(&data_dir).unwrap();
    std::fs::create_dir_all(dir.path().join("output")).unwrap();

    std::fs::write(
        config_dir.join("protocol.toml"),
        "[protocol]\nname = \"test\"\nversion = \"1.0.0\"\ndescription = \"t\"\n\n[paths]\nmanaged = [\"output\"]\ndata_dir = \"output/.sahjhan\"\nrender_dir = \"output\"\n",
    )
    .unwrap();
    std::fs::write(
        config_dir.join("states.toml"),
        "[states.idle]\nlabel = \"Idle\"\ninitial = true\n",
    )
    .unwrap();
    std::fs::write(
        config_dir.join("transitions.toml"),
        "[[transitions]]\nfrom = \"idle\"\nto = \"idle\"\ncommand = \"noop\"\ngates = []\n",
    )
    .unwrap();

    // Create a legacy ledger without seals (using Ledger::init directly)
    let ledger_path = data_dir.join("ledger.jsonl");
    let _ledger = Ledger::init(&ledger_path, "test", "1.0.0").unwrap();

    // Create registry
    let reg_path = data_dir.join("ledgers.toml");
    let mut registry = sahjhan::ledger::registry::LedgerRegistry::new(&reg_path).unwrap();
    registry
        .create(
            "default",
            "ledger.jsonl",
            sahjhan::ledger::registry::LedgerMode::Stateful,
        )
        .unwrap();

    // Create manifest
    let mut manifest =
        sahjhan::manifest::tracker::Manifest::init("output/.sahjhan", vec!["output".to_string()])
            .unwrap();
    manifest
        .save(&data_dir.join("manifest.json"))
        .unwrap();

    // Create session key
    std::fs::write(data_dir.join("session.key"), &[0u8; 32]).unwrap();

    // Status should work (no seals = skip verification)
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .assert()
        .success();
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test config_integrity_tests -- test_cli 2>&1 | head -20`
Expected: compilation error or "reseal" not recognized

- [ ] **Step 3: Add cmd_reseal to authed_event.rs**

Add to `src/cli/authed_event.rs`:

```rust
// [cmd-reseal]
/// Re-seal config file hashes into the ledger. Requires HMAC proof.
///
/// The proof is computed over the payload "config_reseal" (event type only,
/// no fields) using the session key.
pub fn cmd_reseal(config_dir: &str, proof: &str, targeting: &LedgerTargeting) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);

    // Open ledger WITHOUT config seal verification (it will fail — that's why we're resealing)
    let (path, _mode) = match super::commands::resolve_ledger_from_targeting(&config, targeting) {
        Ok(pm) => pm,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };
    let mut ledger = match crate::ledger::chain::Ledger::open(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: cannot open ledger: {}", e);
            return EXIT_INTEGRITY_ERROR;
        }
    };

    // Verify HMAC proof
    let key_path = resolve_session_key_path(&data_dir, targeting);
    let key = match std::fs::read(&key_path) {
        Ok(k) => k,
        Err(e) => {
            eprintln!(
                "error: cannot read session key at {}: {}",
                key_path.display(),
                e
            );
            return EXIT_INTEGRITY_ERROR;
        }
    };

    let payload = "config_reseal";
    let mut mac = match HmacSha256::new_from_slice(&key) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: invalid session key: {}", e);
            return EXIT_INTEGRITY_ERROR;
        }
    };
    mac.update(payload.as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());

    if proof != expected {
        eprintln!("error: invalid proof for reseal");
        return EXIT_INTEGRITY_ERROR;
    }

    // Compute new seals
    let new_seals = crate::config::compute_config_seals(&config_path);

    // Show what changed
    if let Some(old_seals) = ledger.find_effective_seal() {
        let mut changed = Vec::new();
        for (key, new_hash) in &new_seals {
            if let Some(old_hash) = old_seals.get(key) {
                if old_hash != new_hash {
                    let filename = key.strip_prefix("config_seal_").unwrap_or(key);
                    changed.push(format!("  {}.toml", filename));
                }
            }
        }
        if !changed.is_empty() {
            println!("changed files:");
            for c in &changed {
                println!("{}", c);
            }
        }
    }

    // Append config_reseal event
    if let Err(e) = ledger.append("config_reseal", new_seals) {
        eprintln!("error: cannot append reseal event: {}", e);
        return EXIT_INTEGRITY_ERROR;
    }

    println!("resealed.");
    super::commands::EXIT_SUCCESS
}
```

Also add `resolve_ledger_from_targeting` to the import list in `authed_event.rs` if not already imported. Check the existing import block — it already imports from `super::commands`. You may need to add `resolve_ledger_from_targeting` to that import.

- [ ] **Step 4: Add Reseal command to main.rs**

In `src/main.rs`, add the new command variant to the `Commands` enum (after `AuthedEvent`):

```rust
    /// Re-seal config file hashes after legitimate changes (requires HMAC proof)
    Reseal {
        /// HMAC-SHA256 proof
        #[arg(long)]
        proof: String,
    },
```

Add the dispatch case in the `match cli.command` block (after the `AuthedEvent` arm):

```rust
        Commands::Reseal { proof } => {
            authed_event::cmd_reseal(&cli.config_dir, &proof, &targeting)
        }
```

- [ ] **Step 5: Verify everything compiles**

Run: `cargo build 2>&1 | tail -5`
Expected: successful build

- [ ] **Step 6: Run all tests**

Run: `cargo test 2>&1 | tail -10`
Expected: all tests PASS

- [ ] **Step 7: Commit**

```bash
git add src/cli/authed_event.rs src/main.rs tests/config_integrity_tests.rs
git commit -m "feat: add sahjhan reseal command with HMAC authentication"
```

---

### Task 8: Update Documentation

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Update Module Lookup Tables in CLAUDE.md**

Add entries to the relevant tables:

In the **config/** table, add:

```
| Config seal hashing | `config/mod.rs` | `compute_config_seals()` | SHA-256 hash all 5 TOML config files |
```

In the **ledger/** table, add:

```
| Config seal init | `ledger/chain.rs` | `init_with_seals` | Create genesis with config integrity seals |
| Find effective seal | `ledger/chain.rs` | `[find-effective-seal]` | Most recent config_reseal or genesis seals |
| Verify config seal | `ledger/chain.rs` | `[verify-config-seal]` | Verify config files match sealed hashes |
| Config integrity error | `ledger/entry.rs` | `ConfigIntegrityViolation` | Error when config files don't match seal |
```

In the **cli/** table, add:

```
| Reseal | `cli/authed_event.rs` | `[cmd-reseal]` | HMAC-authenticated config reseal |
```

- [ ] **Step 2: Update Gate Evaluation Dispatch table**

No change needed — no new gate types.

- [ ] **Step 3: Update Flow Maps**

Add a new flow map section:

```
### Flow: Config Integrity Verification

How config seals are created and verified:

\```
cli/init.rs [cmd-init]
  → config/mod.rs compute_config_seals()      ← SHA-256 of all 5 TOML files
  → ledger/chain.rs init_with_seals()          ← seals stored in genesis entry fields

cli/commands.rs [open-ledger] or [open-targeted]
  → ledger/chain.rs Ledger::open()             ← parse and verify hash chain
  → ledger/chain.rs [verify-config-seal]
    → ledger/chain.rs [find-effective-seal]     ← scan for config_reseal, fall back to genesis
    → config/mod.rs compute_config_seals()      ← re-hash current files
    → compare: mismatch → ConfigIntegrityViolation

cli/authed_event.rs [cmd-reseal]
  → HMAC proof verification (session key)
  → config/mod.rs compute_config_seals()        ← new hashes
  → ledger/chain.rs [ledger-append]             ← config_reseal event
\```
```

- [ ] **Step 4: Update Test Files table**

Add:

```
| `tests/config_integrity_tests.rs` | Config sealing, tamper detection, reseal, backward compat |
```

- [ ] **Step 5: Verify and commit**

Run: `cargo test 2>&1 | tail -5`
Expected: all tests PASS

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md with config integrity module documentation"
```

---

### Task 9: Final Verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test 2>&1`
Expected: all tests PASS, including all new config_integrity_tests

- [ ] **Step 2: Run clippy**

Run: `cargo clippy -- -D warnings 2>&1`
Expected: no warnings

- [ ] **Step 3: Run fmt**

Run: `cargo fmt -- --check 2>&1`
Expected: no formatting issues

- [ ] **Step 4: Verify the end-to-end flow manually**

```bash
cd $(mktemp -d)
mkdir enforcement
# Create minimal config
cat > enforcement/protocol.toml << 'EOF'
[protocol]
name = "verify-test"
version = "1.0.0"
description = "Manual verification"

[paths]
managed = ["out"]
data_dir = "out/.sahjhan"
render_dir = "out"
EOF
cat > enforcement/states.toml << 'EOF'
[states.idle]
label = "Idle"
initial = true
EOF
cat > enforcement/transitions.toml << 'EOF'
[[transitions]]
from = "idle"
to = "idle"
command = "noop"
gates = []
EOF
mkdir -p out

# Init (creates sealed genesis)
sahjhan init

# Status works
sahjhan status

# Tamper with transitions.toml
echo "# tampered" >> enforcement/transitions.toml

# Status should fail with config integrity violation
sahjhan status  # Expected: error

# Reseal (need proof)
KEY=$(cat out/.sahjhan/session.key | xxd -p -c 256)
# Compute HMAC externally or use session-key-path to verify
```
