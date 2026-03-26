use sha2::{Digest, Sha256};
use thiserror::Error;

/// Errors that can occur when working with ledger entries.
#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("invalid magic bytes")]
    InvalidMagic,

    #[error("unsupported format version: {0}")]
    UnsupportedVersion(u8),

    #[error("invalid UTF-8 in event_type field")]
    InvalidUtf8,

    #[error("hash mismatch for seq {seq}: expected {expected}, computed {computed}")]
    HashMismatch {
        seq: u64,
        expected: String,
        computed: String,
    },

    #[error("sequence gap: expected {expected}, found {found}")]
    SequenceGap { expected: u64, found: u64 },

    #[error("truncated data: expected at least {expected_min} bytes, got {actual}")]
    Truncated { expected_min: usize, actual: usize },

    #[error("timestamp regression at seq {seq}: previous {prev_ts}, current {curr_ts}")]
    TimestampRegression {
        seq: u64,
        prev_ts: i64,
        curr_ts: i64,
    },

    #[error("chain mismatch at seq {seq}: expected prev_hash {expected}, found {found}")]
    ChainMismatch {
        seq: u64,
        expected: String,
        found: String,
    },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Magic bytes that identify a Sahjhan ledger entry.
const MAGIC: &[u8; 4] = b"SAHJ";

/// Current binary format version.
const FORMAT_VERSION: u8 = 1;

/// A single entry in the Sahjhan ledger.
///
/// Binary layout:
/// ```text
/// magic:           [u8; 4]  — "SAHJ"
/// format_version:  u8       — 1
/// seq:             u64 LE   — monotonic sequence number
/// timestamp:       i64 LE   — Unix milliseconds
/// prev_hash:       [u8; 32] — SHA-256 of previous entry
/// event_type_len:  u16 LE   — length of event_type string
/// event_type:      [u8; N]
/// payload_len:     u32 LE   — length of payload
/// payload:         [u8; M]
/// entry_hash:      [u8; 32] — SHA-256(all preceding fields)
/// ```
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
    /// Create a new `LedgerEntry`, computing its hash immediately.
    pub fn new(seq: u64, prev_hash: [u8; 32], event_type: String, payload: Vec<u8>) -> Self {
        let timestamp = chrono::Utc::now().timestamp_millis();
        let entry_hash = compute_hash(seq, timestamp, &prev_hash, &event_type, &payload);
        LedgerEntry {
            seq,
            timestamp,
            prev_hash,
            event_type,
            payload,
            entry_hash,
        }
    }

    /// Serialize this entry to bytes in the canonical binary format.
    pub fn to_bytes(&self) -> Vec<u8> {
        let et_bytes = self.event_type.as_bytes();
        let et_len = et_bytes.len() as u16;
        let pl_len = self.payload.len() as u32;

        // Pre-compute capacity: 4+1+8+8+32+2+N+4+M+32
        let capacity = 4 + 1 + 8 + 8 + 32 + 2 + et_bytes.len() + 4 + self.payload.len() + 32;
        let mut buf = Vec::with_capacity(capacity);

        buf.extend_from_slice(MAGIC);
        buf.push(FORMAT_VERSION);
        buf.extend_from_slice(&self.seq.to_le_bytes());
        buf.extend_from_slice(&self.timestamp.to_le_bytes());
        buf.extend_from_slice(&self.prev_hash);
        buf.extend_from_slice(&et_len.to_le_bytes());
        buf.extend_from_slice(et_bytes);
        buf.extend_from_slice(&pl_len.to_le_bytes());
        buf.extend_from_slice(&self.payload);
        buf.extend_from_slice(&self.entry_hash);

        buf
    }

    /// Deserialize a `LedgerEntry` from bytes, validating the hash.
    ///
    /// Returns `LedgerError::Truncated` if there are not enough bytes,
    /// `LedgerError::InvalidMagic` for bad magic, etc.
    pub fn from_bytes(data: &[u8]) -> Result<Self, LedgerError> {
        let mut cur = Cursor::new(data);

        // magic (4 bytes)
        let magic = cur.read_bytes(4)?;
        if magic != MAGIC {
            return Err(LedgerError::InvalidMagic);
        }

        // format_version (1 byte)
        let version = cur.read_u8()?;
        if version != FORMAT_VERSION {
            return Err(LedgerError::UnsupportedVersion(version));
        }

        // seq (8 bytes LE)
        let seq = u64::from_le_bytes(cur.read_bytes(8)?.try_into().unwrap());

        // timestamp (8 bytes LE)
        let timestamp = i64::from_le_bytes(cur.read_bytes(8)?.try_into().unwrap());

        // prev_hash (32 bytes)
        let prev_hash_slice = cur.read_bytes(32)?;
        let mut prev_hash = [0u8; 32];
        prev_hash.copy_from_slice(prev_hash_slice);

        // event_type_len (2 bytes LE)
        let et_len = u16::from_le_bytes(cur.read_bytes(2)?.try_into().unwrap()) as usize;

        // event_type (et_len bytes)
        let et_bytes = cur.read_bytes(et_len)?.to_vec();
        let event_type =
            String::from_utf8(et_bytes).map_err(|_| LedgerError::InvalidUtf8)?;

        // payload_len (4 bytes LE)
        let pl_len = u32::from_le_bytes(cur.read_bytes(4)?.try_into().unwrap()) as usize;

        // payload (pl_len bytes)
        let payload = cur.read_bytes(pl_len)?.to_vec();

        // entry_hash (32 bytes) — must be exactly the final bytes
        let stored_hash_slice = cur.read_bytes(32)?;
        let mut stored_hash = [0u8; 32];
        stored_hash.copy_from_slice(stored_hash_slice);

        // Ensure we consumed the entire buffer (no trailing garbage)
        if cur.remaining() != 0 {
            return Err(LedgerError::Truncated {
                expected_min: data.len() - cur.remaining(),
                actual: data.len(),
            });
        }

        // Verify hash
        let computed_hash = compute_hash(seq, timestamp, &prev_hash, &event_type, &payload);
        if computed_hash != stored_hash {
            return Err(LedgerError::HashMismatch {
                seq,
                expected: hex_encode(&stored_hash),
                computed: hex_encode(&computed_hash),
            });
        }

        Ok(LedgerEntry {
            seq,
            timestamp,
            prev_hash,
            event_type,
            payload,
            entry_hash: stored_hash,
        })
    }

    /// Deserialize a `LedgerEntry` from a byte slice that may contain trailing data.
    ///
    /// Returns the parsed entry and the number of bytes consumed. Unlike
    /// `from_bytes`, this does **not** fail if there is data remaining after
    /// the entry — it is intended for reading a stream of concatenated entries.
    pub fn from_bytes_partial(data: &[u8]) -> Result<(Self, usize), LedgerError> {
        let mut cur = Cursor::new(data);

        // magic (4 bytes)
        let magic = cur.read_bytes(4)?;
        if magic != MAGIC {
            return Err(LedgerError::InvalidMagic);
        }

        // format_version (1 byte)
        let version = cur.read_u8()?;
        if version != FORMAT_VERSION {
            return Err(LedgerError::UnsupportedVersion(version));
        }

        // seq (8 bytes LE)
        let seq = u64::from_le_bytes(cur.read_bytes(8)?.try_into().unwrap());

        // timestamp (8 bytes LE)
        let timestamp = i64::from_le_bytes(cur.read_bytes(8)?.try_into().unwrap());

        // prev_hash (32 bytes)
        let prev_hash_slice = cur.read_bytes(32)?;
        let mut prev_hash = [0u8; 32];
        prev_hash.copy_from_slice(prev_hash_slice);

        // event_type_len (2 bytes LE)
        let et_len = u16::from_le_bytes(cur.read_bytes(2)?.try_into().unwrap()) as usize;

        // event_type (et_len bytes)
        let et_bytes = cur.read_bytes(et_len)?.to_vec();
        let event_type =
            String::from_utf8(et_bytes).map_err(|_| LedgerError::InvalidUtf8)?;

        // payload_len (4 bytes LE)
        let pl_len = u32::from_le_bytes(cur.read_bytes(4)?.try_into().unwrap()) as usize;

        // payload (pl_len bytes)
        let payload = cur.read_bytes(pl_len)?.to_vec();

        // entry_hash (32 bytes)
        let stored_hash_slice = cur.read_bytes(32)?;
        let mut stored_hash = [0u8; 32];
        stored_hash.copy_from_slice(stored_hash_slice);

        let bytes_consumed = cur.pos;

        // Verify hash
        let computed_hash = compute_hash(seq, timestamp, &prev_hash, &event_type, &payload);
        if computed_hash != stored_hash {
            return Err(LedgerError::HashMismatch {
                seq,
                expected: hex_encode(&stored_hash),
                computed: hex_encode(&computed_hash),
            });
        }

        Ok((
            LedgerEntry {
                seq,
                timestamp,
                prev_hash,
                event_type,
                payload,
                entry_hash: stored_hash,
            },
            bytes_consumed,
        ))
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Compute SHA-256 over the canonical pre-hash fields.
fn compute_hash(
    seq: u64,
    timestamp: i64,
    prev_hash: &[u8; 32],
    event_type: &str,
    payload: &[u8],
) -> [u8; 32] {
    let et_bytes = event_type.as_bytes();
    let et_len = (et_bytes.len() as u16).to_le_bytes();
    let pl_len = (payload.len() as u32).to_le_bytes();

    let mut hasher = Sha256::new();
    hasher.update(MAGIC);
    hasher.update([FORMAT_VERSION]);
    hasher.update(seq.to_le_bytes());
    hasher.update(timestamp.to_le_bytes());
    hasher.update(prev_hash);
    hasher.update(et_len);
    hasher.update(et_bytes);
    hasher.update(pl_len);
    hasher.update(payload);

    hasher.finalize().into()
}

/// Hex-encode a byte slice for display in error messages.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

// ---------------------------------------------------------------------------
// Bounds-checking cursor
// ---------------------------------------------------------------------------

struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    fn new(data: &'a [u8]) -> Self {
        Cursor { data, pos: 0 }
    }

    /// Read exactly `n` bytes, returning `LedgerError::Truncated` on underflow.
    fn read_bytes(&mut self, n: usize) -> Result<&'a [u8], LedgerError> {
        let end = self.pos.checked_add(n).ok_or(LedgerError::Truncated {
            expected_min: n,
            actual: self.data.len(),
        })?;
        if end > self.data.len() {
            return Err(LedgerError::Truncated {
                expected_min: end,
                actual: self.data.len(),
            });
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    /// Read a single byte.
    fn read_u8(&mut self) -> Result<u8, LedgerError> {
        Ok(self.read_bytes(1)?[0])
    }

    /// Return how many bytes remain unread.
    fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }
}
