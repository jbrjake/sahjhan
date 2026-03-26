use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use fs2::FileExt;

use super::entry::{LedgerEntry, LedgerError};

/// Engine identifier stamped into every entry.
const ENGINE_NAME: &str = "sahjhan";

/// Timeout for acquiring an exclusive file lock (seconds).
const LOCK_TIMEOUT_SECS: u64 = 5;

/// An append-only, hash-chained ledger stored as a JSONL file.
///
/// Each line is a single JSON object produced by `LedgerEntry::to_jsonl()`.
/// The chain is verified by checking that every entry's `prev` matches the
/// `hash` of its predecessor, and that sequence numbers are contiguous.
#[derive(Debug)]
pub struct Ledger {
    path: PathBuf,
    entries: Vec<LedgerEntry>,
    engine: String,
    protocol: String,
}

impl Ledger {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    /// Create a new ledger at `path` with a genesis entry for the given
    /// protocol. Fails if the file already exists.
    pub fn init(
        path: &Path,
        protocol_name: &str,
        protocol_version: &str,
    ) -> Result<Self, LedgerError> {
        let genesis = create_genesis(protocol_name, protocol_version);

        // Create (exclusive) — fail if the file already exists.
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

    /// Open an existing ledger, parsing and validating every entry.
    pub fn open(path: &Path) -> Result<Self, LedgerError> {
        let file = File::open(path)?;
        file.lock_shared()?;

        let entries = parse_file_inner(&file)?;
        file.unlock()?;

        if entries.is_empty() {
            return Err(LedgerError::ParseError(
                "ledger file is empty (no genesis entry)".to_string(),
            ));
        }

        // Extract engine/protocol from genesis
        let engine = entries[0].engine.clone();
        let protocol = entries[0].protocol.clone();

        let ledger = Ledger {
            path: path.to_path_buf(),
            entries,
            engine,
            protocol,
        };

        ledger.verify()?;

        Ok(ledger)
    }

    // -----------------------------------------------------------------------
    // Mutation
    // -----------------------------------------------------------------------

    /// Append a new entry to the ledger.
    ///
    /// The entry's `seq` is `last_seq + 1` and its `prev` is the
    /// `hash` of the current tail.
    pub fn append(
        &mut self,
        event_type: &str,
        fields: BTreeMap<String, String>,
    ) -> Result<(), LedgerError> {
        let prev = self
            .entries
            .last()
            .expect("ledger must have at least one entry (genesis)");
        let seq = prev.seq + 1;
        let prev_hash = prev.hash.clone();

        let entry = LedgerEntry::new(
            seq,
            prev_hash,
            event_type,
            &self.engine,
            &self.protocol,
            fields,
        );

        // Append to file under exclusive lock.
        let file = OpenOptions::new().append(true).open(&self.path)?;
        lock_exclusive_with_timeout(&file, &self.path)?;
        // Use a BufWriter-like approach: write line then unlock
        let mut file = file;
        writeln!(file, "{}", entry.to_jsonl())?;
        file.unlock()?;

        self.entries.push(entry);
        Ok(())
    }

    /// Append a new entry with an explicit timestamp.
    ///
    /// Useful for imports and migration tooling.
    pub fn append_with_ts(
        &mut self,
        event_type: &str,
        fields: BTreeMap<String, String>,
        ts: String,
    ) -> Result<(), LedgerError> {
        let prev = self
            .entries
            .last()
            .expect("ledger must have at least one entry (genesis)");
        let seq = prev.seq + 1;
        let prev_hash = prev.hash.clone();

        let entry = LedgerEntry::new_with_ts(
            seq,
            prev_hash,
            event_type,
            &self.engine,
            &self.protocol,
            fields,
            ts,
        );

        let file = OpenOptions::new().append(true).open(&self.path)?;
        lock_exclusive_with_timeout(&file, &self.path)?;
        let mut file = file;
        writeln!(file, "{}", entry.to_jsonl())?;
        file.unlock()?;

        self.entries.push(entry);
        Ok(())
    }

    /// Re-read the ledger file from disk, replacing the in-memory entries.
    ///
    /// Call this after any operation that may have let an external process
    /// append to the ledger file (e.g. a gate command that records events).
    pub fn reload(&mut self) -> Result<(), LedgerError> {
        let file = File::open(&self.path)?;
        file.lock_shared()?;

        let entries = parse_file_inner(&file)?;
        file.unlock()?;

        if !entries.is_empty() {
            self.engine = entries[0].engine.clone();
            self.protocol = entries[0].protocol.clone();
        }

        self.entries = entries;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Verification
    // -----------------------------------------------------------------------

    /// Verify the full integrity of the in-memory ledger.
    ///
    /// Checks:
    /// 1. Sequence numbers are contiguous starting at 0.
    /// 2. Hash chain: `entry[i].prev == entry[i-1].hash`.
    pub fn verify(&self) -> Result<(), LedgerError> {
        for (i, entry) in self.entries.iter().enumerate() {
            let expected_seq = i as u64;
            if entry.seq != expected_seq {
                return Err(LedgerError::SequenceGap {
                    expected: expected_seq,
                    found: entry.seq,
                });
            }

            if i > 0 {
                let prev = &self.entries[i - 1];

                // Hash chain check
                if entry.prev != prev.hash {
                    return Err(LedgerError::ChainBreak {
                        seq: entry.seq,
                        prev: entry.prev.clone(),
                        previous_hash: prev.hash.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Accessors
    // -----------------------------------------------------------------------

    /// Number of entries (including genesis).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the ledger contains no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Borrow all entries.
    pub fn entries(&self) -> &[LedgerEntry] {
        &self.entries
    }

    /// The file system path of this ledger.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// The `hash` of the most recent entry (hex string).
    pub fn last_hash(&self) -> String {
        self.entries
            .last()
            .expect("ledger must have genesis entry")
            .hash
            .clone()
    }

    /// All entries whose `event_type` equals `kind`.
    pub fn events_of_type(&self, kind: &str) -> Vec<&LedgerEntry> {
        self.entries
            .iter()
            .filter(|e| e.event_type == kind)
            .collect()
    }

    /// The last `n` entries (or fewer if the ledger is shorter).
    pub fn tail(&self, n: usize) -> &[LedgerEntry] {
        let len = self.entries.len();
        if n >= len {
            &self.entries
        } else {
            &self.entries[len - n..]
        }
    }

    /// The engine identifier (e.g. "sahjhan").
    pub fn engine(&self) -> &str {
        &self.engine
    }

    /// The protocol identifier (e.g. "my-proto/1.0.0").
    pub fn protocol(&self) -> &str {
        &self.protocol
    }
}

// ---------------------------------------------------------------------------
// Genesis
// ---------------------------------------------------------------------------

/// Create the genesis (first) entry for a new ledger.
///
/// The genesis entry has seq 0. Its `prev` is a cryptographically random
/// nonce (32 bytes, hex-encoded to 64 chars) so that two ledgers initialised
/// with the same parameters are still distinguishable.
fn create_genesis(protocol_name: &str, protocol_version: &str) -> LedgerEntry {
    let mut nonce = [0u8; 32];
    getrandom::getrandom(&mut nonce).expect("CSPRNG failed");
    let prev = hex::encode(nonce);

    let protocol = format!("{}/{}", protocol_name, protocol_version);

    let mut fields = BTreeMap::new();
    fields.insert("protocol_name".to_string(), protocol_name.to_string());
    fields.insert(
        "protocol_version".to_string(),
        protocol_version.to_string(),
    );

    LedgerEntry::new(
        0,
        prev,
        "genesis",
        ENGINE_NAME,
        &protocol,
        fields,
    )
}

// ---------------------------------------------------------------------------
// Public file parsing (for query module)
// ---------------------------------------------------------------------------

/// Parse a JSONL ledger file into entries without creating a full `Ledger`.
///
/// This is useful for the query module (Task 10) which needs to parse JSONL
/// files without the overhead of a full Ledger struct.
pub fn parse_file_entries(path: &Path) -> Result<Vec<LedgerEntry>, LedgerError> {
    let file = File::open(path)?;
    file.lock_shared()?;
    let entries = parse_file_inner(&file)?;
    file.unlock()?;
    Ok(entries)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Parse entries from an already-opened file handle.
fn parse_file_inner(file: &File) -> Result<Vec<LedgerEntry>, LedgerError> {
    let reader = BufReader::new(file);
    let mut entries = Vec::new();

    for line_result in reader.lines() {
        let line = line_result?;
        let trimmed = line.trim();

        // Skip blank lines
        if trimmed.is_empty() {
            continue;
        }

        // Attempt to parse; from_jsonl verifies hash integrity
        let entry = LedgerEntry::from_jsonl(trimmed)?;
        entries.push(entry);
    }

    Ok(entries)
}

/// Acquire an exclusive lock with a timeout, using try_lock_exclusive polling.
fn lock_exclusive_with_timeout(file: &File, path: &Path) -> Result<(), LedgerError> {
    let deadline = Instant::now() + std::time::Duration::from_secs(LOCK_TIMEOUT_SECS);

    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(()),
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                if Instant::now() >= deadline {
                    return Err(LedgerError::LockTimeout {
                        path: path.display().to_string(),
                    });
                }
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            Err(e) => return Err(LedgerError::Io(e)),
        }
    }
}
