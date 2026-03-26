use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use fs2::FileExt;

use super::entry::{LedgerEntry, LedgerError};
use super::genesis::create_genesis;

/// An append-only, hash-chained ledger stored in a single binary file.
///
/// Entries are concatenated raw bytes (each entry produced by
/// `LedgerEntry::to_bytes()`). The chain is verified by checking that every
/// entry's `prev_hash` matches the `entry_hash` of its predecessor.
pub struct Ledger {
    path: PathBuf,
    entries: Vec<LedgerEntry>,
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
        let bytes = genesis.to_bytes();

        // Create (exclusive) — fail if the file already exists.
        let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;

        file.lock_exclusive()?;
        file.write_all(&bytes)?;
        file.unlock()?;

        Ok(Ledger {
            path: path.to_path_buf(),
            entries: vec![genesis],
        })
    }

    /// Open an existing ledger, parsing and validating every entry.
    pub fn open(path: &Path) -> Result<Self, LedgerError> {
        let mut file = File::open(path)?;
        file.lock_shared()?;

        let mut raw = Vec::new();
        file.read_to_end(&mut raw)?;
        file.unlock()?;

        let entries = parse_entries(&raw)?;

        Ok(Ledger {
            path: path.to_path_buf(),
            entries,
        })
    }

    // -----------------------------------------------------------------------
    // Mutation
    // -----------------------------------------------------------------------

    /// Append a new entry to the ledger.
    ///
    /// The entry's `seq` is `last_seq + 1` and its `prev_hash` is the
    /// `entry_hash` of the current tail.
    pub fn append(&mut self, event_type: &str, payload: Vec<u8>) -> Result<(), LedgerError> {
        let prev = self
            .entries
            .last()
            .expect("ledger must have at least one entry (genesis)");
        let seq = prev.seq + 1;
        let prev_hash = prev.entry_hash;

        let entry = LedgerEntry::new_binary(seq, prev_hash, event_type.to_string(), payload);
        let bytes = entry.to_bytes();

        // Append to file under exclusive lock.
        let mut file = OpenOptions::new().append(true).open(&self.path)?;
        file.lock_exclusive()?;
        file.write_all(&bytes)?;
        file.unlock()?;

        self.entries.push(entry);
        Ok(())
    }

    /// Re-read the ledger file from disk, replacing the in-memory entries.
    ///
    /// Call this after any operation that may have let an external process
    /// append to the ledger file (e.g. a gate command that records events).
    pub fn reload(&mut self) -> Result<(), LedgerError> {
        let mut file = File::open(&self.path)?;
        file.lock_shared()?;

        let mut raw = Vec::new();
        file.read_to_end(&mut raw)?;
        file.unlock()?;

        self.entries = parse_entries(&raw)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Verification
    // -----------------------------------------------------------------------

    /// Verify the full integrity of the in-memory ledger.
    ///
    /// Checks (in order):
    /// 1. Sequence numbers are contiguous starting at 0.
    /// 2. Hash chain: `entry[i].prev_hash == entry[i-1].entry_hash`.
    /// 3. Timestamps are non-decreasing (E8).
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
                if entry.prev_hash != prev.entry_hash {
                    return Err(LedgerError::ChainMismatch {
                        seq: entry.seq,
                        expected: hex_encode(&prev.entry_hash),
                        found: hex_encode(&entry.prev_hash),
                    });
                }

                // Timestamp monotonicity (E8)
                if entry.timestamp < prev.timestamp {
                    return Err(LedgerError::TimestampRegression {
                        seq: entry.seq,
                        prev_ts: prev.timestamp,
                        curr_ts: entry.timestamp,
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

    /// The `entry_hash` of the most recent entry.
    pub fn last_hash(&self) -> [u8; 32] {
        self.entries
            .last()
            .expect("ledger must have genesis entry")
            .entry_hash
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
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Parse a stream of concatenated entries from raw bytes.
fn parse_entries(mut data: &[u8]) -> Result<Vec<LedgerEntry>, LedgerError> {
    let mut entries = Vec::new();
    while !data.is_empty() {
        let (entry, consumed) = LedgerEntry::from_bytes_partial(data)?;
        entries.push(entry);
        data = &data[consumed..];
    }
    Ok(entries)
}

/// Hex-encode bytes for error messages.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
