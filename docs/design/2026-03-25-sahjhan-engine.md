# Sahjhan Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a standalone Rust CLI that enforces AI agent protocol compliance via a hash-chain ledger, declarative state machine, and file-integrity manifest.

**Architecture:** A single Rust binary (`sahjhan`) reads protocol definitions from TOML config files, maintains a tamper-evident binary ledger of events, tracks file integrity via SHA-256 manifest, and exposes a CLI for state transitions, event recording, and verification. Hook bridge scripts (generated Python) integrate with Claude Code's PreToolUse/PostToolUse system.

**Tech Stack:** Rust (2021 edition), clap (CLI), serde + toml (config), sha2 (hashing), rmp-serde (MessagePack for ledger payloads), tera (template rendering), getrandom (CSPRNG), fs2 (file locking)

**Spec:** `docs/superpowers/specs/2026-03-25-sahjhan-enforcement-engine-design.md`

---

## File Structure

```
sahjhan/
├── Cargo.toml
├── src/
│   ├── main.rs                    # CLI entry point, clap command tree
│   ├── lib.rs                     # Public crate API
│   ├── ledger/
│   │   ├── mod.rs                 # Re-exports
│   │   ├── entry.rs               # LedgerEntry struct, binary serialization
│   │   ├── chain.rs               # Append, read, verify operations
│   │   └── genesis.rs             # Genesis block creation
│   ├── state/
│   │   ├── mod.rs
│   │   ├── machine.rs             # StateMachine struct, transition execution
│   │   └── sets.rs                # CompletionSet tracking
│   ├── gates/
│   │   ├── mod.rs
│   │   ├── evaluator.rs           # Gate evaluation orchestrator
│   │   ├── types.rs               # Individual gate type implementations
│   │   └── template.rs            # Template variable resolution + escaping
│   ├── manifest/
│   │   ├── mod.rs
│   │   ├── tracker.rs             # File hash tracking, update operations
│   │   └── verify.rs              # Integrity verification
│   ├── config/
│   │   ├── mod.rs
│   │   ├── protocol.rs            # protocol.toml parsing
│   │   ├── states.rs              # states.toml parsing
│   │   ├── transitions.rs         # transitions.toml parsing
│   │   └── events.rs              # events.toml parsing
│   ├── render/
│   │   ├── mod.rs
│   │   └── engine.rs              # Tera template rendering from ledger state
│   ├── cli/
│   │   ├── mod.rs
│   │   ├── commands.rs            # Built-in command implementations
│   │   └── aliases.rs             # Alias resolution from config
│   └── hooks/
│       ├── mod.rs
│       └── generate.rs            # Hook script generation from templates
├── templates/
│   └── hooks/
│       ├── write_guard.py.tera    # PreToolUse Write/Edit blocker template
│       └── bash_guard.py.tera     # PostToolUse manifest verifier template
├── tests/
│   ├── ledger_tests.rs
│   ├── chain_integrity_tests.rs
│   ├── state_machine_tests.rs
│   ├── gate_tests.rs
│   ├── manifest_tests.rs
│   ├── config_tests.rs
│   ├── template_security_tests.rs
│   └── integration_tests.rs
├── examples/
│   └── minimal/                   # 3-state, 2-transition example protocol
│       ├── protocol.toml
│       ├── states.toml
│       ├── transitions.toml
│       └── events.toml
└── .github/
    └── workflows/
        └── release.yml            # Cross-compile + release binaries
```

---

### Task 1: Project Scaffolding

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/lib.rs`

- [ ] **Step 1: Initialize Rust project**

```bash
cargo init sahjhan
cd sahjhan
```

- [ ] **Step 2: Set up Cargo.toml with dependencies**

```toml
[package]
name = "sahjhan"
version = "0.1.0"
edition = "2021"
description = "Protocol enforcement engine for AI agents"
license = "MIT"
repository = "https://github.com/jbrjake/sahjhan"

[dependencies]
clap = { version = "4", features = ["derive"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"
sha2 = "0.10"
rmp-serde = "1"
tera = "1"
getrandom = "0.2"
fs2 = "0.4"
chrono = { version = "0.4", features = ["serde"] }
thiserror = "2"

[dev-dependencies]
tempfile = "3"
assert_cmd = "2"
predicates = "3"
```

- [ ] **Step 3: Create minimal main.rs with clap skeleton**

```rust
// src/main.rs
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "sahjhan", version, about = "Protocol enforcement engine for AI agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Path to protocol config directory
    #[arg(long, default_value = "enforcement")]
    config_dir: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize ledger, manifest, genesis block
    Init,
    /// Show current state and gate status
    Status,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init => println!("init not yet implemented"),
        Commands::Status => println!("status not yet implemented"),
    }
}
```

- [ ] **Step 4: Create lib.rs with module declarations**

```rust
// src/lib.rs
pub mod ledger;
pub mod state;
pub mod gates;
pub mod manifest;
pub mod config;
pub mod render;
pub mod cli;
pub mod hooks;
```

Create empty `mod.rs` files in each subdirectory.

- [ ] **Step 5: Verify it compiles**

Run: `cargo build`
Expected: Compiles with no errors.

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: scaffold sahjhan project with dependencies and CLI skeleton"
```

---

### Task 2: Ledger Entry Format

**Files:**
- Create: `src/ledger/entry.rs`
- Create: `src/ledger/mod.rs`
- Test: `tests/ledger_tests.rs`

- [ ] **Step 1: Write failing test for entry serialization roundtrip**

```rust
// tests/ledger_tests.rs
use sahjhan::ledger::entry::LedgerEntry;

#[test]
fn test_entry_roundtrip() {
    let entry = LedgerEntry::new(
        1,
        [0u8; 32],
        "test_event".to_string(),
        b"test payload".to_vec(),
    );
    let bytes = entry.to_bytes();
    let parsed = LedgerEntry::from_bytes(&bytes).unwrap();
    assert_eq!(parsed.seq, 1);
    assert_eq!(parsed.event_type, "test_event");
    assert_eq!(parsed.payload, b"test payload");
    // entry_hash should be deterministic
    assert_eq!(entry.entry_hash, parsed.entry_hash);
}

#[test]
fn test_entry_has_magic_bytes() {
    let entry = LedgerEntry::new(0, [0u8; 32], "init".to_string(), vec![]);
    let bytes = entry.to_bytes();
    assert_eq!(&bytes[0..4], b"SAHJ");
}

#[test]
fn test_entry_has_format_version() {
    let entry = LedgerEntry::new(0, [0u8; 32], "init".to_string(), vec![]);
    let bytes = entry.to_bytes();
    assert_eq!(bytes[4], 1); // format version 1
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test ledger_tests`
Expected: FAIL — `LedgerEntry` not found.

- [ ] **Step 3: Implement LedgerEntry**

```rust
// src/ledger/entry.rs
use sha2::{Sha256, Digest};
use std::time::{SystemTime, UNIX_EPOCH};

const MAGIC: &[u8; 4] = b"SAHJ";
const FORMAT_VERSION: u8 = 1;

#[derive(Debug, Clone, PartialEq)]
pub struct LedgerEntry {
    pub seq: u64,
    pub timestamp: i64,
    pub prev_hash: [u8; 32],
    pub event_type: String,
    pub payload: Vec<u8>,
    pub entry_hash: [u8; 32],
}

impl LedgerEntry {
    pub fn new(seq: u64, prev_hash: [u8; 32], event_type: String, payload: Vec<u8>) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;

        let mut entry = Self {
            seq,
            timestamp,
            prev_hash,
            event_type,
            payload,
            entry_hash: [0u8; 32],
        };
        entry.entry_hash = entry.compute_hash();
        entry
    }

    fn compute_hash(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(MAGIC);
        hasher.update([FORMAT_VERSION]);
        hasher.update(self.seq.to_le_bytes());
        hasher.update(self.timestamp.to_le_bytes());
        hasher.update(self.prev_hash);
        hasher.update((self.event_type.len() as u16).to_le_bytes());
        hasher.update(self.event_type.as_bytes());
        hasher.update((self.payload.len() as u32).to_le_bytes());
        hasher.update(&self.payload);
        hasher.finalize().into()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(MAGIC);
        buf.push(FORMAT_VERSION);
        buf.extend_from_slice(&self.seq.to_le_bytes());
        buf.extend_from_slice(&self.timestamp.to_le_bytes());
        buf.extend_from_slice(&self.prev_hash);
        buf.extend_from_slice(&(self.event_type.len() as u16).to_le_bytes());
        buf.extend_from_slice(self.event_type.as_bytes());
        buf.extend_from_slice(&(self.payload.len() as u32).to_le_bytes());
        buf.extend_from_slice(&self.payload);
        buf.extend_from_slice(&self.entry_hash);
        buf
    }

    pub fn from_bytes(data: &[u8]) -> Result<Self, LedgerError> {
        if data.len() < 4 || &data[0..4] != MAGIC {
            return Err(LedgerError::InvalidMagic);
        }
        if data[4] != FORMAT_VERSION {
            return Err(LedgerError::UnsupportedVersion(data[4]));
        }
        let mut pos = 5;

        let seq = u64::from_le_bytes(data[pos..pos+8].try_into().unwrap());
        pos += 8;
        let timestamp = i64::from_le_bytes(data[pos..pos+8].try_into().unwrap());
        pos += 8;
        let prev_hash: [u8; 32] = data[pos..pos+32].try_into().unwrap();
        pos += 32;
        let et_len = u16::from_le_bytes(data[pos..pos+2].try_into().unwrap()) as usize;
        pos += 2;
        let event_type = String::from_utf8(data[pos..pos+et_len].to_vec())
            .map_err(|_| LedgerError::InvalidUtf8)?;
        pos += et_len;
        let pl_len = u32::from_le_bytes(data[pos..pos+4].try_into().unwrap()) as usize;
        pos += 4;
        let payload = data[pos..pos+pl_len].to_vec();
        pos += pl_len;
        let entry_hash: [u8; 32] = data[pos..pos+32].try_into().unwrap();

        let mut entry = Self { seq, timestamp, prev_hash, event_type, payload, entry_hash };
        let computed = entry.compute_hash();
        if computed != entry_hash {
            return Err(LedgerError::HashMismatch { seq, expected: entry_hash, computed });
        }
        entry.entry_hash = entry_hash;
        Ok(entry)
    }

    /// Total byte size of this entry when serialized.
    pub fn byte_len(&self) -> usize {
        4 + 1 + 8 + 8 + 32 + 2 + self.event_type.len() + 4 + self.payload.len() + 32
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    #[error("invalid magic bytes — not a Sahjhan ledger")]
    InvalidMagic,
    #[error("unsupported format version: {0}")]
    UnsupportedVersion(u8),
    #[error("invalid UTF-8 in event type")]
    InvalidUtf8,
    #[error("hash mismatch at seq {seq}: expected {expected:?}, computed {computed:?}")]
    HashMismatch { seq: u64, expected: [u8; 32], computed: [u8; 32] },
    #[error("sequence gap: expected {expected}, found {found}")]
    SequenceGap { expected: u64, found: u64 },
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

- [ ] **Step 4: Update mod.rs**

```rust
// src/ledger/mod.rs
pub mod entry;
pub mod chain;
pub mod genesis;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test ledger_tests`
Expected: 3 passed.

- [ ] **Step 6: Commit**

```bash
git add src/ledger/ tests/ledger_tests.rs
git commit -m "feat: implement LedgerEntry binary serialization with hash chain"
```

---

### Task 3: Chain Operations (Append, Read, Verify)

**Files:**
- Create: `src/ledger/chain.rs`
- Create: `src/ledger/genesis.rs`
- Test: `tests/chain_integrity_tests.rs`

- [ ] **Step 1: Write failing tests for chain operations**

```rust
// tests/chain_integrity_tests.rs
use sahjhan::ledger::chain::Ledger;
use tempfile::tempdir;

#[test]
fn test_genesis_creates_valid_chain() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&path, "test-protocol", "1.0.0").unwrap();
    assert_eq!(ledger.len(), 1);
    assert!(ledger.verify().is_ok());
}

#[test]
fn test_append_maintains_chain() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&path, "test-protocol", "1.0.0").unwrap();
    ledger.append("test_event", b"payload1".to_vec()).unwrap();
    ledger.append("test_event", b"payload2".to_vec()).unwrap();
    assert_eq!(ledger.len(), 3); // genesis + 2
    assert!(ledger.verify().is_ok());
}

#[test]
fn test_verify_detects_tampering() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&path, "test-protocol", "1.0.0").unwrap();
    ledger.append("event", b"data".to_vec()).unwrap();
    drop(ledger);

    // Tamper: flip a byte in the middle of the file
    let mut data = std::fs::read(&path).unwrap();
    let mid = data.len() / 2;
    data[mid] ^= 0xFF;
    std::fs::write(&path, &data).unwrap();

    let ledger = Ledger::open(&path).unwrap();
    assert!(ledger.verify().is_err());
}

#[test]
fn test_verify_detects_deletion() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&path, "test-protocol", "1.0.0").unwrap();
    ledger.append("event1", b"a".to_vec()).unwrap();
    ledger.append("event2", b"b".to_vec()).unwrap();
    ledger.append("event3", b"c".to_vec()).unwrap();
    drop(ledger);

    // Remove the second entry by rewriting genesis + entry3 only
    let ledger = Ledger::open(&path).unwrap();
    let entries = ledger.entries();
    // Write only entries 0 and 2 (skip 1) — sequence gap
    let mut tampered = Vec::new();
    tampered.extend(entries[0].to_bytes());
    tampered.extend(entries[2].to_bytes());
    std::fs::write(&path, &tampered).unwrap();

    let ledger = Ledger::open(&path).unwrap();
    let result = ledger.verify();
    assert!(result.is_err());
}

#[test]
fn test_open_existing_ledger() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&path, "test-protocol", "1.0.0").unwrap();
    ledger.append("event", b"data".to_vec()).unwrap();
    drop(ledger);

    let ledger = Ledger::open(&path).unwrap();
    assert_eq!(ledger.len(), 2);
    assert!(ledger.verify().is_ok());
}

#[test]
fn test_concurrent_access_locked() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("ledger.bin");
    let mut ledger = Ledger::init(&path, "test-protocol", "1.0.0").unwrap();
    // Hold the write lock
    ledger.append("event", b"data".to_vec()).unwrap();
    // Ledger holds exclusive lock during operations — tested via fs2
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test chain_integrity_tests`
Expected: FAIL — `Ledger` not found.

- [ ] **Step 3: Implement genesis.rs**

```rust
// src/ledger/genesis.rs
use super::entry::LedgerEntry;
use getrandom::getrandom;
use rmp_serde;
use serde::Serialize;

#[derive(Serialize)]
struct GenesisPayload {
    protocol_name: String,
    protocol_version: String,
    format_version: u8,
}

pub fn create_genesis(protocol_name: &str, protocol_version: &str) -> LedgerEntry {
    let mut nonce = [0u8; 32];
    getrandom(&mut nonce).expect("CSPRNG failed");

    let payload = GenesisPayload {
        protocol_name: protocol_name.to_string(),
        protocol_version: protocol_version.to_string(),
        format_version: 1,
    };
    let payload_bytes = rmp_serde::to_vec(&payload).unwrap();

    LedgerEntry::new(0, nonce, "protocol_init".to_string(), payload_bytes)
}
```

- [ ] **Step 4: Implement chain.rs**

```rust
// src/ledger/chain.rs
use super::entry::{LedgerEntry, LedgerError};
use super::genesis::create_genesis;
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub struct Ledger {
    path: PathBuf,
    entries: Vec<LedgerEntry>,
}

impl Ledger {
    pub fn init(path: &Path, protocol_name: &str, protocol_version: &str) -> Result<Self, LedgerError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let genesis = create_genesis(protocol_name, protocol_version);
        let mut file = File::create(path)?;
        file.lock_exclusive().map_err(|e| LedgerError::Io(e))?;
        file.write_all(&genesis.to_bytes())?;
        file.unlock().map_err(|e| LedgerError::Io(e))?;

        Ok(Self {
            path: path.to_path_buf(),
            entries: vec![genesis],
        })
    }

    pub fn open(path: &Path) -> Result<Self, LedgerError> {
        let mut file = File::open(path)?;
        let mut data = Vec::new();
        file.read_to_end(&mut data)?;

        let mut entries = Vec::new();
        let mut pos = 0;
        while pos < data.len() {
            let entry = LedgerEntry::from_bytes(&data[pos..])?;
            pos += entry.byte_len();
            entries.push(entry);
        }

        Ok(Self {
            path: path.to_path_buf(),
            entries,
        })
    }

    pub fn append(&mut self, event_type: &str, payload: Vec<u8>) -> Result<&LedgerEntry, LedgerError> {
        let prev_hash = self.entries.last().unwrap().entry_hash;
        let seq = self.entries.len() as u64;
        let entry = LedgerEntry::new(seq, prev_hash, event_type.to_string(), payload);

        let mut file = OpenOptions::new().append(true).open(&self.path)?;
        file.lock_exclusive().map_err(|e| LedgerError::Io(e))?;
        file.write_all(&entry.to_bytes())?;
        file.unlock().map_err(|e| LedgerError::Io(e))?;

        self.entries.push(entry);
        Ok(self.entries.last().unwrap())
    }

    pub fn verify(&self) -> Result<(), LedgerError> {
        for (i, entry) in self.entries.iter().enumerate() {
            // Check sequence monotonicity
            if entry.seq != i as u64 {
                return Err(LedgerError::SequenceGap {
                    expected: i as u64,
                    found: entry.seq,
                });
            }
            // Check prev_hash chain (skip genesis)
            if i > 0 {
                let prev = &self.entries[i - 1];
                if entry.prev_hash != prev.entry_hash {
                    return Err(LedgerError::HashMismatch {
                        seq: entry.seq,
                        expected: prev.entry_hash,
                        computed: entry.prev_hash,
                    });
                }
            }
        }
        Ok(())
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn entries(&self) -> &[LedgerEntry] {
        &self.entries
    }

    pub fn last_hash(&self) -> [u8; 32] {
        self.entries.last().unwrap().entry_hash
    }

    pub fn events_of_type(&self, event_type: &str) -> Vec<&LedgerEntry> {
        self.entries.iter().filter(|e| e.event_type == event_type).collect()
    }

    pub fn tail(&self, n: usize) -> &[LedgerEntry] {
        let start = self.entries.len().saturating_sub(n);
        &self.entries[start..]
    }
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --test chain_integrity_tests`
Expected: All pass.

- [ ] **Step 6: Commit**

```bash
git add src/ledger/ tests/chain_integrity_tests.rs
git commit -m "feat: implement hash-chain ledger with append, verify, and tamper detection"
```

---

### Task 4: Config Parsing

**Files:**
- Create: `src/config/protocol.rs`
- Create: `src/config/states.rs`
- Create: `src/config/transitions.rs`
- Create: `src/config/events.rs`
- Create: `src/config/mod.rs`
- Create: `examples/minimal/` (4 TOML files)
- Test: `tests/config_tests.rs`

- [ ] **Step 1: Create minimal example protocol**

```toml
# examples/minimal/protocol.toml
[protocol]
name = "minimal"
version = "1.0.0"
description = "Minimal example protocol"

[paths]
managed = ["output"]
data_dir = "output/.sahjhan"
render_dir = "output"

[sets.check]
description = "Verification checks"
values = ["tests", "lint"]

[aliases]
"start" = "transition begin"
"finish" = "transition complete"
```

```toml
# examples/minimal/states.toml
[states.idle]
label = "Idle"
initial = true

[states.working]
label = "Working"

[states.done]
label = "Done"
terminal = true
```

```toml
# examples/minimal/transitions.toml
[[transitions]]
from = "idle"
to = "working"
command = "begin"
gates = []

[[transitions]]
from = "working"
to = "done"
command = "complete"
gates = [
    { type = "set_covered", set = "check", event = "set_member_complete", field = "member" },
]
```

```toml
# examples/minimal/events.toml
[events.set_member_complete]
description = "A check passed"
fields = [
    { name = "set", type = "string" },
    { name = "member", type = "string" },
]
```

- [ ] **Step 2: Write failing test for config loading**

```rust
// tests/config_tests.rs
use sahjhan::config::ProtocolConfig;
use std::path::Path;

#[test]
fn test_load_minimal_protocol() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    assert_eq!(config.protocol.name, "minimal");
    assert_eq!(config.states.len(), 3);
    assert_eq!(config.transitions.len(), 2);
    assert_eq!(config.events.len(), 1);
    assert!(config.sets.contains_key("check"));
    assert_eq!(config.sets["check"].values.len(), 2);
}

#[test]
fn test_initial_state_exists() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    assert!(config.initial_state().is_some());
    assert_eq!(config.initial_state().unwrap(), "idle");
}

#[test]
fn test_transitions_reference_valid_states() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let errors = config.validate();
    assert!(errors.is_empty(), "Validation errors: {:?}", errors);
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --test config_tests`
Expected: FAIL — `ProtocolConfig` not found.

- [ ] **Step 4: Implement config parsing**

Implement `ProtocolConfig` struct with `load()` that reads all four TOML files from a directory, `initial_state()` that finds the state with `initial = true`, and `validate()` that checks:
- Exactly one initial state
- All transition `from`/`to` reference existing states
- All `set_covered` gates reference existing sets
- All event field types are valid

Use `serde::Deserialize` with TOML for each config struct. The structs should match the TOML format from the spec — `ProtocolMeta`, `StateConfig`, `TransitionConfig`, `GateConfig`, `EventConfig`, `SetConfig`.

- [ ] **Step 5: Run tests**

Run: `cargo test --test config_tests`
Expected: All pass.

- [ ] **Step 6: Commit**

```bash
git add src/config/ examples/minimal/ tests/config_tests.rs
git commit -m "feat: implement TOML config parsing with validation"
```

---

### Task 5: State Machine Executor

**Files:**
- Create: `src/state/machine.rs`
- Create: `src/state/sets.rs`
- Test: `tests/state_machine_tests.rs`

- [ ] **Step 1: Write failing tests**

```rust
// tests/state_machine_tests.rs
use sahjhan::config::ProtocolConfig;
use sahjhan::state::machine::StateMachine;
use sahjhan::ledger::chain::Ledger;
use tempfile::tempdir;
use std::path::Path;

#[test]
fn test_initial_state() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "minimal", "1.0.0").unwrap();
    let sm = StateMachine::new(&config, ledger);
    assert_eq!(sm.current_state(), "idle");
}

#[test]
fn test_valid_transition() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "minimal", "1.0.0").unwrap();
    let mut sm = StateMachine::new(&config, ledger);
    let result = sm.transition("begin", &[]);
    assert!(result.is_ok());
    assert_eq!(sm.current_state(), "working");
}

#[test]
fn test_invalid_transition_from_wrong_state() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "minimal", "1.0.0").unwrap();
    let mut sm = StateMachine::new(&config, ledger);
    let result = sm.transition("complete", &[]); // can't complete from idle
    assert!(result.is_err());
}

#[test]
fn test_gate_blocks_transition() {
    let config = ProtocolConfig::load(Path::new("examples/minimal")).unwrap();
    let dir = tempdir().unwrap();
    let ledger_path = dir.path().join("ledger.bin");
    let ledger = Ledger::init(&ledger_path, "minimal", "1.0.0").unwrap();
    let mut sm = StateMachine::new(&config, ledger);
    sm.transition("begin", &[]).unwrap();
    // Try to complete without recording set completions
    let result = sm.transition("complete", &[]);
    assert!(result.is_err()); // set_covered gate fails
}
```

- [ ] **Step 2: Run tests, verify fail**

Run: `cargo test --test state_machine_tests`
Expected: FAIL.

- [ ] **Step 3: Implement StateMachine**

The `StateMachine` owns a `Ledger` and a reference to `ProtocolConfig`. `transition()` finds matching transitions from the current state with the given command, evaluates all gates, and if all pass, appends a `state_transition` event to the ledger. `current_state()` reads the most recent `state_transition` event from the ledger (or returns the initial state if no transitions exist).

`CompletionSet` (in `sets.rs`) provides `is_covered()` that reads the ledger for `set_member_complete` events and checks all set values are represented.

- [ ] **Step 4: Run tests**

Run: `cargo test --test state_machine_tests`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/state/ tests/state_machine_tests.rs
git commit -m "feat: implement state machine executor with gate evaluation"
```

---

### Task 6: Gate Evaluator

**Files:**
- Create: `src/gates/evaluator.rs`
- Create: `src/gates/types.rs`
- Create: `src/gates/template.rs`
- Test: `tests/gate_tests.rs`

- [ ] **Step 1: Write failing tests for each gate type**

Test at minimum: `files_exist`, `file_exists`, `command_succeeds`, `command_output`, `ledger_has_event`, `set_covered`, `min_elapsed`, `no_violations`. Each test creates a controlled environment (tempdir with files, ledger with events) and verifies the gate passes/fails correctly.

- [ ] **Step 2: Run tests, verify fail**

- [ ] **Step 3: Implement gate types**

Each gate type is a function `fn evaluate(gate: &GateConfig, context: &GateContext) -> Result<(), GateFailure>`. The `GateContext` holds references to the ledger, config, current state params, and working directory.

**Critical:** Template variable resolution in `template.rs` must:
1. Validate field values against their declared patterns before interpolation
2. Apply POSIX shell escaping (single-quote wrapping) to all values interpolated into `cmd` strings
3. Never allow the `cmd` string itself to come from agent input

- [ ] **Step 4: Write template security tests**

```rust
// tests/template_security_tests.rs
use sahjhan::gates::template::resolve_template;

#[test]
fn test_shell_metacharacters_escaped() {
    let result = resolve_template(
        "grep -q '{{id}}'",
        &[("id".to_string(), "'; rm -rf /; echo '".to_string())],
    );
    // Should be safely escaped — the value must not break out of quotes
    assert!(!result.contains("rm -rf"));
}

#[test]
fn test_valid_pattern_passes() {
    // BH-001 matches ^B[HJ]-\d{3}$
    let result = resolve_template("grep -q '{{id}}'", &[("id".to_string(), "BH-001".to_string())]);
    assert!(result.contains("BH-001"));
}
```

- [ ] **Step 5: Run all tests**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 6: Commit**

```bash
git add src/gates/ tests/gate_tests.rs tests/template_security_tests.rs
git commit -m "feat: implement gate evaluator with all gate types and template security"
```

---

### Task 7: Manifest Tracker

**Files:**
- Create: `src/manifest/tracker.rs`
- Create: `src/manifest/verify.rs`
- Test: `tests/manifest_tests.rs`

- [ ] **Step 1: Write failing tests**

Test: `init_creates_manifest`, `track_file_updates_hash`, `verify_detects_modification`, `verify_passes_clean`, `restore_from_render`. Create tempdir with managed files, track them, modify one outside the CLI, verify detection.

- [ ] **Step 2: Run tests, verify fail**

- [ ] **Step 3: Implement Manifest struct**

`Manifest` stores `HashMap<PathBuf, ManifestEntry>` with SHA-256 hash, last operation, timestamp, and ledger seq. `track()` computes hash and records. `verify()` recomputes all hashes and returns mismatches. `manifest_hash()` computes SHA-256 of the serialized entries for recording in the ledger.

JSON serialization for the manifest file (`.sahjhan/manifest.json`). The manifest itself is inside the managed path and tracked by the ledger.

- [ ] **Step 4: Run tests**

Run: `cargo test --test manifest_tests`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/manifest/ tests/manifest_tests.rs
git commit -m "feat: implement manifest tracker with SHA-256 file integrity verification"
```

---

### Task 8: CLI Commands

**Files:**
- Modify: `src/main.rs`
- Create: `src/cli/commands.rs`
- Create: `src/cli/aliases.rs`
- Test: `tests/integration_tests.rs`

- [ ] **Step 1: Write integration tests using assert_cmd**

```rust
// tests/integration_tests.rs
use assert_cmd::Command;
use tempfile::tempdir;
use predicates::prelude::*;

#[test]
fn test_init_creates_ledger() {
    let dir = tempdir().unwrap();
    // Copy minimal example config to tempdir
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();
    for file in &["protocol.toml", "states.toml", "transitions.toml", "events.toml"] {
        std::fs::copy(
            format!("examples/minimal/{}", file),
            config_dir.join(file),
        ).unwrap();
    }

    Command::cargo_bin("sahjhan").unwrap()
        .args(["--config-dir", config_dir.to_str().unwrap(), "init"])
        .current_dir(dir.path())
        .assert()
        .success();

    assert!(dir.path().join("output/.sahjhan/ledger.bin").exists());
    assert!(dir.path().join("output/.sahjhan/manifest.json").exists());
}

#[test]
fn test_status_shows_current_state() {
    // init + status
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan").unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("State:"))
        .stdout(predicate::str::contains("Idle"));
}

#[test]
fn test_transition_advances_state() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan").unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin"])
        .current_dir(dir.path())
        .assert()
        .success();

    Command::cargo_bin("sahjhan").unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .assert()
        .stdout(predicate::str::contains("Working"));
}

#[test]
fn test_log_verify_clean() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan").unwrap()
        .args(["--config-dir", "enforcement", "log", "verify"])
        .current_dir(dir.path())
        .assert()
        .success();
}

#[test]
fn test_manifest_verify_clean() {
    let dir = setup_initialized_dir();
    Command::cargo_bin("sahjhan").unwrap()
        .args(["--config-dir", "enforcement", "manifest", "verify"])
        .current_dir(dir.path())
        .assert()
        .success();
}
```

- [ ] **Step 2: Run tests, verify fail**

- [ ] **Step 3: Implement all CLI commands**

Wire up clap subcommands: `init`, `status`, `transition`, `event`, `set complete`, `set status`, `log dump`, `log verify`, `log tail`, `manifest verify`, `manifest list`, `render`, `gate check`, `reset`. Each command loads config, opens ledger, performs the operation, and prints results.

`aliases.rs` reads the `[aliases]` table from protocol.toml and registers them as clap aliases.

- [ ] **Step 4: Run tests**

Run: `cargo test --test integration_tests`
Expected: All pass.

- [ ] **Step 5: Run full test suite**

Run: `cargo test`
Expected: All pass.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs src/cli/ tests/integration_tests.rs
git commit -m "feat: implement full CLI command surface with alias support"
```

---

### Task 9: Template Rendering

**Files:**
- Create: `src/render/engine.rs`
- Test: add render tests to `tests/integration_tests.rs`

- [ ] **Step 1: Write failing test**

Test that after init + transition, `sahjhan render` produces a STATUS.md file in the render directory with current state information.

- [ ] **Step 2: Implement render engine**

Load Tera templates from the config directory's `templates/` subdirectory. Build a context from the current ledger state (current state, events by type, set completion, metrics). Render each template per the `renders.toml` triggers. Write output and track in manifest.

- [ ] **Step 3: Run tests**

- [ ] **Step 4: Commit**

```bash
git add src/render/ tests/
git commit -m "feat: implement Tera template rendering from ledger state"
```

---

### Task 10: Hook Bridge Generation

**Files:**
- Create: `src/hooks/generate.rs`
- Create: `templates/hooks/write_guard.py.tera`
- Create: `templates/hooks/bash_guard.py.tera`

- [ ] **Step 1: Create hook templates**

`write_guard.py.tera`: A Python script that reads the PreToolUse event JSON from stdin, checks if `tool_input.file_path` is under any managed path, and blocks if so. The managed paths are injected from `protocol.toml` at generation time.

`bash_guard.py.tera`: A Python script that calls `sahjhan manifest verify` after a Bash command and warns if files were modified.

- [ ] **Step 2: Implement generate.rs**

`sahjhan hook generate --harness cc` reads the templates, resolves the managed paths from config, and writes the generated Python scripts to stdout or a specified directory.

- [ ] **Step 3: Test generated hooks manually**

Run: `sahjhan --config-dir examples/minimal hook generate --harness cc`
Verify: Output contains valid Python with the correct managed paths.

- [ ] **Step 4: Commit**

```bash
git add src/hooks/ templates/
git commit -m "feat: implement hook bridge generation for Claude Code"
```

---

### Task 11: Cross-Platform CI + Release

**Files:**
- Create: `.github/workflows/ci.yml`
- Create: `.github/workflows/release.yml`

- [ ] **Step 1: Create CI workflow**

Run `cargo test`, `cargo clippy`, `cargo fmt --check` on push/PR.

- [ ] **Step 2: Create release workflow**

On tag push (`v*`): cross-compile for `aarch64-apple-darwin`, `x86_64-apple-darwin`, `x86_64-unknown-linux-gnu`, `aarch64-unknown-linux-gnu`. Upload binaries as GitHub release assets. Use `cross` for cross-compilation.

- [ ] **Step 3: Commit**

```bash
git add .github/
git commit -m "ci: add CI and cross-platform release workflows"
```

---

### Task 12: README and Documentation

**Files:**
- Create: `README.md`
- Create: `LICENSE`

- [ ] **Step 1: Write README**

Cover: what Sahjhan is, why it exists (link to the postmortem concept), quick start with the minimal example, full CLI reference, how to write a protocol definition, how to generate hooks for Claude Code.

- [ ] **Step 2: Add MIT LICENSE**

- [ ] **Step 3: Commit**

```bash
git add README.md LICENSE
git commit -m "docs: add README and LICENSE"
```

---

## Consumer Integration: How Holtz Gets Sahjhan Binaries

The Holtz project consumes Sahjhan as vendored binaries, not as a Rust dependency. The flow:

1. **Sahjhan releases** are tagged (`v0.1.0`, etc.) and CI produces cross-compiled binaries as GitHub release assets.

2. **Holtz vendors binaries** via a download script:

```bash
# scripts/vendor-sahjhan.sh
#!/bin/bash
set -euo pipefail
VERSION="${1:?Usage: vendor-sahjhan.sh <version>}"
BASE_URL="https://github.com/jbrjake/sahjhan/releases/download/v${VERSION}"
mkdir -p bin
for target in aarch64-apple-darwin x86_64-apple-darwin x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu; do
    curl -sL "${BASE_URL}/sahjhan-${target}" -o "bin/sahjhan-${target}"
    chmod +x "bin/sahjhan-${target}"
done
echo "Vendored sahjhan v${VERSION}"
```

3. **Holtz's `scripts/install-hooks.sh`** is updated to also set up a `sahjhan` wrapper that resolves the correct platform binary:

```bash
# Added to install-hooks.sh
ARCH=$(uname -m)
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
# Map arch names
case "$ARCH" in
    arm64) ARCH="aarch64" ;;
    x86_64) ARCH="x86_64" ;;
esac
SAHJHAN_BIN="bin/sahjhan-${ARCH}-${OS}"
if [ -f "$SAHJHAN_BIN" ]; then
    ln -sf "../$SAHJHAN_BIN" .git/sahjhan
    echo "Sahjhan linked: $SAHJHAN_BIN"
fi
```

4. **Holtz's generated hook scripts** resolve the binary via environment variable or relative path:

```python
# In enforcement/hooks/write_guard.py
import os, platform
def sahjhan_binary():
    env = os.environ.get("SAHJHAN_BIN")
    if env:
        return env
    arch = platform.machine()
    system = platform.system().lower()
    if arch == "arm64":
        arch = "aarch64"
    root = os.environ.get("CLAUDE_PLUGIN_ROOT", os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    return os.path.join(root, "bin", f"sahjhan-{arch}-{system}")
```

5. **Version pinning:** Holtz's `enforcement/protocol.toml` declares `sahjhan_version = "0.1.0"`. The install script warns if the vendored binary version doesn't match. `sahjhan --version` outputs the version for comparison.

6. **`.gitignore`:** The vendored binaries are committed to the Holtz repo (they're part of the plugin distribution). They are NOT gitignored. When a new Sahjhan version is released, `scripts/vendor-sahjhan.sh <new-version>` is run and the updated binaries are committed.

---

## Review Errata

The following issues were identified during plan review and must be addressed during implementation.

### Critical Fixes

**E1. Add `renders.toml` parsing to Task 4.**
Task 4 lists four TOML files but the spec defines a fifth: `renders.toml` which configures template rendering triggers. Add `src/config/renders.rs` to the config module and include `renders.toml` in the minimal example. Without this, the render engine (Task 9) has no configuration to read.

**E2. Add explicit `reset` access control to Task 8.**
The `sahjhan reset --confirm` command is security-critical (wipes enforcement state). Task 8 lists it in a one-line summary. Add a dedicated step: implement confirmation token derived from genesis hash, display to terminal, require manual entry. Add a test that verifies `reset` without the token fails, and that piped tokens succeed but record a `protocol_violation` via PostToolUse.

**E3. Fix platform binary resolution in consumer integration.**
The vendor script constructs `sahjhan-${ARCH}-${OS}` (e.g., `sahjhan-aarch64-darwin`) but the actual release binaries use full target triples (`sahjhan-aarch64-apple-darwin`). Fix the platform detection to construct the full triple:
```bash
case "$OS" in
    darwin) TRIPLE="${ARCH}-apple-darwin" ;;
    linux)  TRIPLE="${ARCH}-unknown-linux-gnu" ;;
esac
SAHJHAN_BIN="bin/sahjhan-${TRIPLE}"
```
Apply the same fix in the Python `sahjhan_binary()` resolver.

**E4. Add bounds checking to `from_bytes` in Task 2.**
The `LedgerEntry::from_bytes()` implementation indexes into the data slice with hardcoded offsets without checking `data.len()`. Use a cursor or wrapper that returns `LedgerError` on underflow instead of panicking on truncated/corrupt data.

**E5. Add template injection integration test to Task 6.**
The template security unit tests check `resolve_template` in isolation. Add an integration test: record an event with a field value containing shell metacharacters → execute a gate with `command_succeeds` using that field in a template → verify the command does NOT execute injected code and returns safely.

### Important Fixes

**E6. Resolve Task 5/6 dependency.** Task 5 (State Machine) tests depend on gate evaluation (Task 6). Either: implement a minimal gate evaluator stub in Task 5, defer gate-dependent tests to after Task 6, or reorder so Task 6 comes before Task 5.

**E7. Add missing gate types to Task 6.** The test list omits: `snapshot_compare`, `field_not_empty`, `ledger_has_event_since`, `command_output`. All four are defined in the spec and used in the Holtz integration transitions.

**E8. Add timestamp monotonicity to `verify()`.** The spec requires `log verify` to validate timestamps are non-decreasing. The implementation only checks sequence and hash chain. Add a check in `verify()`.

**E9. Add entry insertion tamper test to Task 3.** Tests cover deletion and modification but not insertion. Add: write genesis + entry1 + fabricated_entry + entry2, verify chain detects the fabrication.

**E10. Add bootstrap hook task.** The spec describes `_sahjhan_bootstrap.py` but no task creates it. This is a consuming project's responsibility, but the Sahjhan engine should ship a template or example.

**E11. Add event field validation task.** When `sahjhan event <type> --field val` is called, field patterns/enums/ranges from `events.toml` must be validated. No task explicitly implements this validation at recording time (separate from template interpolation).

**E12. Add `data_dir` under managed path validation.** The spec requires `sahjhan init` to refuse if `data_dir` is not within a `paths.managed` entry. Add this check and a test.

**E13. Add binary checksum verification to vendor script.** The vendor script downloads over HTTPS but doesn't verify checksums. The release workflow should produce `checksums.sha256`, and the vendor script should verify.

**E14. Add render lifecycle orchestration task.** The lifecycle (validate gates → append event → update manifest → trigger renders → update manifest for renders) cuts across Tasks 5, 7, and 9. Add a task or a step in Task 8 that implements this full orchestration and tests the end-to-end flow.

**E15. Add `manifest restore` implementation detail.** Two strategies (re-render from ledger vs git checkout) need design. Add to Task 7 or create a sub-step.

### Suggestions

**E16.** Fix the concurrent access test placeholder (Task 3) — actually test two threads contending for the lock.
**E17.** Document the binary-in-git size tradeoff (~20-60MB) and consider Git LFS.
**E18.** Establish a unified error type and exit code scheme (0 = success, 1 = gate failed, 2 = integrity error, etc.).
**E19.** Specify `log dump` output format (human-readable deserialization of MessagePack payloads).
**E20.** Remove redundant `use rmp_serde;` import and `map_err` on `lock_exclusive`.
