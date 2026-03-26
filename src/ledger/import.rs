//! Ledger import — wrap bare JSONL events in a hash-chained ledger.
//!
//! `import_jsonl` reads one JSON object per line from any `BufRead` source
//! and appends each event to a freshly-created ledger at `output_path`.
//! The function preserves the original `ts` field when present; otherwise
//! the current time is used.

use std::collections::BTreeMap;
use std::io::BufRead;
use std::path::Path;

use super::chain::Ledger;
use super::entry::LedgerError;

/// Import bare JSONL events from `reader` into a new ledger at `output_path`.
///
/// Each input line must be a JSON object with at minimum a `"type"` key.
/// Optional keys:
/// - `"fields"` — a JSON object whose values are all strings.
/// - `"ts"` — an RFC 3339 timestamp string; preserved verbatim if present.
///
/// Blank lines are silently skipped. The output ledger starts with a genesis
/// entry followed by one entry per imported event.
pub fn import_jsonl(
    reader: &mut dyn BufRead,
    output_path: &Path,
    protocol_name: &str,
    protocol_version: &str,
) -> Result<(), LedgerError> {
    let mut ledger = Ledger::init(output_path, protocol_name, protocol_version)?;

    let mut line_buf = String::new();
    loop {
        line_buf.clear();
        let bytes_read = reader
            .read_line(&mut line_buf)
            .map_err(LedgerError::Io)?;

        // EOF
        if bytes_read == 0 {
            break;
        }

        let trimmed = line_buf.trim();

        // Skip blank lines
        if trimmed.is_empty() {
            continue;
        }

        // Parse as a generic JSON object
        let obj: serde_json::Value = serde_json::from_str(trimmed)
            .map_err(|e| LedgerError::ParseError(format!("import line parse error: {e}")))?;

        // Extract event type (required)
        let event_type = obj
            .get("type")
            .and_then(|v| v.as_str())
            .ok_or_else(|| {
                LedgerError::ParseError(
                    "import line missing required \"type\" field".to_string(),
                )
            })?;

        // Extract fields (optional — default to empty map)
        let fields: BTreeMap<String, String> = match obj.get("fields") {
            Some(serde_json::Value::Object(map)) => map
                .iter()
                .filter_map(|(k, v)| {
                    // Coerce all values to strings
                    let s = match v {
                        serde_json::Value::String(s) => s.clone(),
                        other => other.to_string(),
                    };
                    Some((k.clone(), s))
                })
                .collect(),
            Some(_) => {
                return Err(LedgerError::ParseError(
                    "import line \"fields\" must be a JSON object".to_string(),
                ))
            }
            None => BTreeMap::new(),
        };

        // Extract optional timestamp
        let ts_opt = obj.get("ts").and_then(|v| v.as_str()).map(String::from);

        match ts_opt {
            Some(ts) => ledger.append_with_ts(event_type, fields, ts)?,
            None => ledger.append(event_type, fields)?,
        }
    }

    Ok(())
}
