use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use thiserror::Error;

/// Current schema version for JSONL ledger entries.
pub const SCHEMA_VERSION: u64 = 1;

/// Errors that can occur when working with ledger entries.
#[derive(Debug, Error)]
pub enum LedgerError {
    #[error("JSON parse error: {0}")]
    ParseError(String),

    #[error("unsupported schema version: {0}")]
    UnsupportedVersion(u64),

    #[error("hash mismatch at seq {seq}: expected {expected}, got {actual}")]
    HashMismatch {
        seq: u64,
        expected: String,
        actual: String,
    },

    #[error("chain break at seq {seq}: prev={prev}, previous hash={previous_hash}")]
    ChainBreak {
        seq: u64,
        prev: String,
        previous_hash: String,
    },

    #[error("sequence gap: expected {expected}, got {found}")]
    SequenceGap { expected: u64, found: u64 },

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("lock timeout on {path}")]
    LockTimeout { path: String },

    // Legacy variants kept temporarily so chain.rs/cli compiles.
    // Tasks 3-6 will remove these.
    #[error("(legacy) invalid magic bytes")]
    InvalidMagic,

    #[error("(legacy) invalid UTF-8")]
    InvalidUtf8,

    #[error("(legacy) unsupported format version: {0}")]
    UnsupportedFormatVersion(u8),

    #[error("(legacy) truncated data: expected at least {expected_min} bytes, got {actual}")]
    Truncated { expected_min: usize, actual: usize },

    #[error("(legacy) timestamp regression at seq {seq}: previous {prev_ts}, current {curr_ts}")]
    TimestampRegression {
        seq: u64,
        prev_ts: i64,
        curr_ts: i64,
    },

    #[error("(legacy) chain mismatch at seq {seq}: expected prev_hash {expected}, found {found}")]
    ChainMismatch {
        seq: u64,
        expected: String,
        found: String,
    },
}

/// A single entry in the Sahjhan ledger (JSONL format).
///
/// Each entry is one JSON line containing 9 top-level keys (engine, fields,
/// hash, prev, protocol, schema, seq, ts, type) sorted alphabetically per
/// RFC 8785. The `hash` field is a SHA-256 hex digest computed over the
/// canonical JSON of all other fields (the hash input excludes hash itself).
///
/// Legacy fields (`entry_hash`, `prev_hash`, `timestamp`, `payload`) are
/// retained as `#[serde(skip)]` shims so that callers in chain.rs,
/// state/machine.rs, render/engine.rs, gates/types.rs, and cli/commands.rs
/// continue to compile. They will be removed in Tasks 3-6.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LedgerEntry {
    pub schema: u64,
    pub seq: u64,
    pub prev: String,
    pub hash: String,
    pub ts: String,
    #[serde(rename = "type")]
    pub event_type: String,
    pub engine: String,
    pub protocol: String,
    pub fields: BTreeMap<String, String>,

    // ----- Legacy shims (serde-skipped, Tasks 3-6 will remove) -----
    /// Legacy: SHA-256 as raw bytes. Kept for callers that read `entry.entry_hash`.
    #[serde(skip)]
    pub entry_hash: [u8; 32],

    /// Legacy: previous entry hash as raw bytes.
    #[serde(skip)]
    pub prev_hash: [u8; 32],

    /// Legacy: Unix timestamp in milliseconds.
    #[serde(skip)]
    pub timestamp: i64,

    /// Legacy: MessagePack-encoded payload bytes. Always empty in v0.2 entries.
    #[serde(skip)]
    pub payload: Vec<u8>,
}

impl LedgerEntry {
    /// Create a new `LedgerEntry`, computing its hash immediately.
    ///
    /// `prev` is the hash of the previous entry (or a genesis nonce for seq 0).
    pub fn new(
        seq: u64,
        prev: String,
        event_type: &str,
        engine: &str,
        protocol: &str,
        fields: BTreeMap<String, String>,
    ) -> Self {
        let ts = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
        Self::new_with_ts(seq, prev, event_type, engine, protocol, fields, ts)
    }

    /// Create a new `LedgerEntry` with an externally-supplied timestamp.
    ///
    /// Useful for imports, deterministic testing, and migration tooling.
    pub fn new_with_ts(
        seq: u64,
        prev: String,
        event_type: &str,
        engine: &str,
        protocol: &str,
        fields: BTreeMap<String, String>,
        ts: String,
    ) -> Self {
        let hash = Self::compute_hash(
            SCHEMA_VERSION,
            seq,
            &prev,
            &ts,
            event_type,
            engine,
            protocol,
            &fields,
        );

        // Populate legacy shims
        let entry_hash = hex_to_bytes32(&hash);
        let prev_hash = hex_to_bytes32(&prev);
        let timestamp = chrono::DateTime::parse_from_rfc3339(&ts)
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(0);

        LedgerEntry {
            schema: SCHEMA_VERSION,
            seq,
            prev,
            hash,
            ts,
            event_type: event_type.to_string(),
            engine: engine.to_string(),
            protocol: protocol.to_string(),
            fields,
            entry_hash,
            prev_hash,
            timestamp,
            payload: Vec::new(),
        }
    }

    /// Compute the SHA-256 hash of the canonical (RFC 8785) JSON representation.
    ///
    /// The canonical form includes every field EXCEPT `hash` itself. Keys are
    /// sorted alphabetically at all nesting levels. No optional whitespace.
    #[allow(clippy::too_many_arguments)]
    pub fn compute_hash(
        schema: u64,
        seq: u64,
        prev: &str,
        ts: &str,
        event_type: &str,
        engine: &str,
        protocol: &str,
        fields: &BTreeMap<String, String>,
    ) -> String {
        let fields_json = canonical_json_object(fields);
        // Keys in alphabetical order: engine, fields, prev, protocol, schema, seq, ts, type
        let canonical = format!(
            r#"{{"engine":{},"fields":{},"prev":{},"protocol":{},"schema":{},"seq":{},"ts":{},"type":{}}}"#,
            json_string(engine),
            fields_json,
            json_string(prev),
            json_string(protocol),
            schema,
            seq,
            json_string(ts),
            json_string(event_type),
        );
        let mut hasher = Sha256::new();
        hasher.update(canonical.as_bytes());
        hex::encode(hasher.finalize())
    }

    /// Serialize this entry to a single JSONL line (canonical key ordering, no trailing newline).
    pub fn to_jsonl(&self) -> String {
        // Build canonical JSON by hand to guarantee key order and RFC 8785 compliance.
        let fields_json = canonical_json_object(&self.fields);
        format!(
            r#"{{"engine":{},"fields":{},"hash":{},"prev":{},"protocol":{},"schema":{},"seq":{},"ts":{},"type":{}}}"#,
            json_string(&self.engine),
            fields_json,
            json_string(&self.hash),
            json_string(&self.prev),
            json_string(&self.protocol),
            self.schema,
            self.seq,
            json_string(&self.ts),
            json_string(&self.event_type),
        )
    }

    /// Parse a `LedgerEntry` from a single JSONL line.
    ///
    /// Rejects entries with `schema > SCHEMA_VERSION`.
    pub fn from_jsonl(line: &str) -> Result<Self, LedgerError> {
        let mut entry: LedgerEntry =
            serde_json::from_str(line).map_err(|e| LedgerError::ParseError(e.to_string()))?;

        if entry.schema > SCHEMA_VERSION {
            return Err(LedgerError::UnsupportedVersion(entry.schema));
        }

        // Verify hash integrity
        let recomputed = Self::compute_hash(
            entry.schema,
            entry.seq,
            &entry.prev,
            &entry.ts,
            &entry.event_type,
            &entry.engine,
            &entry.protocol,
            &entry.fields,
        );
        if recomputed != entry.hash {
            return Err(LedgerError::HashMismatch {
                seq: entry.seq,
                expected: entry.hash.clone(),
                actual: recomputed,
            });
        }

        // Populate legacy shims (serde skips them on deserialize)
        entry.entry_hash = hex_to_bytes32(&entry.hash);
        entry.prev_hash = hex_to_bytes32(&entry.prev);
        entry.timestamp = chrono::DateTime::parse_from_rfc3339(&entry.ts)
            .map(|dt| dt.timestamp_millis())
            .unwrap_or(0);

        Ok(entry)
    }

    // -----------------------------------------------------------------------
    // Legacy stubs — these panic at runtime. Callers will be migrated in
    // Tasks 3-6 and these stubs will be removed.
    // -----------------------------------------------------------------------

    /// Legacy 4-argument constructor. Panics at runtime.
    /// Exists only so chain.rs and genesis.rs compile during the v0.2 migration.
    #[allow(unused_variables)]
    pub fn new_binary(seq: u64, prev_hash: [u8; 32], event_type: String, payload: Vec<u8>) -> Self {
        panic!("binary format removed in v0.2.0 — use LedgerEntry::new()")
    }

    /// **REMOVED in v0.2.0** — use `to_jsonl()`.
    pub fn to_bytes(&self) -> Vec<u8> {
        panic!("binary format removed in v0.2.0 — use to_jsonl()")
    }

    /// **REMOVED in v0.2.0** — use `from_jsonl()`.
    #[allow(unused_variables)]
    pub fn from_bytes(data: &[u8]) -> Result<Self, LedgerError> {
        panic!("binary format removed in v0.2.0 — use from_jsonl()")
    }

    /// **REMOVED in v0.2.0** — use `from_jsonl()`.
    #[allow(unused_variables)]
    pub fn from_bytes_partial(data: &[u8]) -> Result<(Self, usize), LedgerError> {
        panic!("binary format removed in v0.2.0 — use from_jsonl()")
    }
}

// ---------------------------------------------------------------------------
// RFC 8785 canonical JSON helpers
// ---------------------------------------------------------------------------

/// Encode a string value per RFC 8785: only escape `"`, `\`, and control
/// characters U+0000..U+001F. Forward slashes are NOT escaped.
fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c < '\u{0020}' => {
                // Other control characters: \u00XX
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Build a canonical JSON object string from a BTreeMap (already sorted by key).
fn canonical_json_object(map: &BTreeMap<String, String>) -> String {
    let mut out = String::from("{");
    let mut first = true;
    for (k, v) in map {
        if !first {
            out.push(',');
        }
        first = false;
        out.push_str(&json_string(k));
        out.push(':');
        out.push_str(&json_string(v));
    }
    out.push('}');
    out
}

/// Best-effort hex string to [u8; 32]. Returns zeroed array on failure.
fn hex_to_bytes32(s: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    if let Ok(bytes) = hex::decode(s) {
        if bytes.len() == 32 {
            out.copy_from_slice(&bytes);
        }
    }
    out
}
