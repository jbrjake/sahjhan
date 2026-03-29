// src/cli/authed_event.rs
//
// Authenticated event recording command.
//
// ## Index
// - [cmd-authed-event]            cmd_authed_event()       — record a restricted event with HMAC proof
// - resolve_session_key_path      resolve_session_key_path — resolve key path (per-ledger with global fallback)
// - build_canonical_payload       build_canonical_payload  — build HMAC payload from event type + fields
// - [cmd-reseal]                  cmd_reseal()             — re-seal config hashes with HMAC proof

use std::collections::HashMap;
use std::path::PathBuf;

use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::state::machine::StateMachine;

use super::commands::{
    load_config, load_manifest, open_targeted_ledger, resolve_config_dir, resolve_data_dir,
    resolve_ledger_from_targeting, LedgerTargeting, EXIT_INTEGRITY_ERROR, EXIT_USAGE_ERROR,
};
use super::transition::{record_and_render, validate_event_fields};

type HmacSha256 = Hmac<Sha256>;

/// Resolve the session key path for the given targeting.
///
/// Resolution order:
/// 1. If --ledger <name>, check <data_dir>/ledgers/<name>/session.key
/// 2. If that exists, use it
/// 3. Fall back to <data_dir>/session.key
pub fn resolve_session_key_path(
    data_dir: &std::path::Path,
    targeting: &LedgerTargeting,
) -> PathBuf {
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
    let mut sorted_fields: Vec<(&str, &str)> = fields
        .iter()
        .map(|(k, v)| (k.as_str(), v.as_str()))
        .collect();
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

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting, &config_path) {
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
        if let Err((code, msg)) = validate_event_fields(event_config, &fields, event_type) {
            eprintln!("{}", msg);
            return code;
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
    let (path, _mode) = match resolve_ledger_from_targeting(&config, targeting) {
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
