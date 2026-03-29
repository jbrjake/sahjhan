// src/cli/authed_event.rs
//
// Authenticated event recording command.
//
// ## Index
// - [cmd-authed-event]            cmd_authed_event()       — record a restricted event with HMAC proof
// - resolve_session_key_path      resolve_session_key_path — resolve key path (per-ledger with global fallback)
// - build_canonical_payload       build_canonical_payload  — build HMAC payload from event type + fields

use std::collections::HashMap;
use std::path::PathBuf;

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::state::machine::StateMachine;

use super::commands::{
    load_config, load_manifest, open_targeted_ledger, resolve_config_dir, resolve_data_dir,
    LedgerTargeting, EXIT_INTEGRITY_ERROR, EXIT_USAGE_ERROR,
};
use super::transition::record_and_render;

type HmacSha256 = Hmac<Sha256>;

/// Resolve the session key path for the given targeting.
///
/// Resolution order:
/// 1. If --ledger <name>, check <data_dir>/ledgers/<name>/session.key
/// 2. If that exists, use it
/// 3. Fall back to <data_dir>/session.key
pub fn resolve_session_key_path(data_dir: &std::path::Path, targeting: &LedgerTargeting) -> PathBuf {
    if let Some(ref name) = targeting.ledger_name {
        let per_ledger = data_dir.join("ledgers").join(name).join("session.key");
        if per_ledger.exists() {
            return per_ledger;
        }
    }
    data_dir.join("session.key")
}

/// Build the canonical payload for HMAC computation.
///
/// Format: `event_type\0field1_name=field1_value\0field2_name=field2_value`
/// Fields sorted lexicographically by name.
fn build_canonical_payload(event_type: &str, fields: &HashMap<String, String>) -> String {
    let mut sorted_fields: Vec<(&str, &str)> = fields.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
    sorted_fields.sort_by_key(|(k, _)| *k);

    let mut payload = event_type.to_string();
    for (k, v) in &sorted_fields {
        payload.push('\0');
        payload.push_str(&format!("{}={}", k, v));
    }
    payload
}

// [cmd-authed-event]
pub fn cmd_authed_event(
    config_dir: &str,
    event_type: &str,
    field_strs: &[String],
    proof: &str,
    targeting: &LedgerTargeting,
) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    // Verify event type IS restricted
    match config.events.get(event_type) {
        Some(event_config) => {
            if event_config.restricted != Some(true) {
                eprintln!(
                    "error: event type '{}' is not restricted. Use 'sahjhan event' instead.",
                    event_type
                );
                return EXIT_USAGE_ERROR;
            }
        }
        None => {
            // Unknown event type — proceed (field validation will catch issues)
        }
    }

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let mut manifest = match load_manifest(&data_dir) {
        Ok(m) => m,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    // Parse --field key=value pairs
    let mut fields: HashMap<String, String> = HashMap::new();
    for f in field_strs {
        if let Some((key, value)) = f.split_once('=') {
            fields.insert(key.to_string(), value.to_string());
        } else {
            eprintln!("error: invalid field '{}': expected key=value", f);
            return EXIT_USAGE_ERROR;
        }
    }

    // Validate fields against events.toml definitions
    if let Some(event_config) = config.events.get(event_type) {
        for field_def in &event_config.fields {
            if !fields.contains_key(&field_def.name) {
                eprintln!(
                    "error: missing field '{}' for event '{}'",
                    field_def.name, event_type
                );
                return EXIT_USAGE_ERROR;
            }
        }
        for field_def in &event_config.fields {
            if let Some(pattern) = &field_def.pattern {
                if let Some(value) = fields.get(&field_def.name) {
                    if let Ok(re) = regex::Regex::new(pattern) {
                        if !re.is_match(value) {
                            eprintln!(
                                "error: field '{}' value '{}' doesn't match pattern '{}'",
                                field_def.name, value, pattern
                            );
                            return EXIT_USAGE_ERROR;
                        }
                    }
                }
            }
            if let Some(allowed) = &field_def.values {
                if let Some(value) = fields.get(&field_def.name) {
                    if !allowed.contains(value) {
                        eprintln!(
                            "error: field '{}' value '{}' not in allowed values {:?}",
                            field_def.name, value, allowed
                        );
                        return EXIT_USAGE_ERROR;
                    }
                }
            }
        }
    }

    // Resolve session key
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

    // Compute expected proof
    let payload = build_canonical_payload(event_type, &fields);
    let mut mac = match HmacSha256::new_from_slice(&key) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: invalid session key: {}", e);
            return EXIT_INTEGRITY_ERROR;
        }
    };
    mac.update(payload.as_bytes());
    let expected = hex::encode(mac.finalize().into_bytes());

    // Compare
    if proof != expected {
        eprintln!("error: invalid proof for event '{}'", event_type);
        return EXIT_INTEGRITY_ERROR;
    }

    // Record the event
    let mut machine = StateMachine::new(&config, ledger);

    record_and_render(
        &config,
        &config_path,
        &mut machine,
        &mut manifest,
        &data_dir,
        event_type,
        fields,
        targeting,
    )
}
