// src/cli/authed_event.rs
//
// Authenticated event recording and config resealing commands.
// Proofs are verified via the daemon — no disk-based keys.
//
// ## Index
// - [cmd-authed-event]        cmd_authed_event()  — record a restricted event with HMAC proof
// - [cmd-reseal]              cmd_reseal()         — re-seal config hashes with HMAC proof

use std::collections::HashMap;

use crate::state::machine::StateMachine;

use super::commands::{
    load_config, load_manifest, open_targeted_ledger, resolve_config_dir, resolve_data_dir,
    resolve_ledger_from_targeting, LedgerTargeting, EXIT_INTEGRITY_ERROR, EXIT_USAGE_ERROR,
};
use super::transition::{record_and_render, validate_event_fields};

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

    // Verify event type IS restricted.
    // Undefined event types are rejected — authed-event requires explicit
    // declaration in events.toml with restricted = true.
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
            eprintln!(
                "error: event type '{}' is not defined in events.toml",
                event_type
            );
            return EXIT_USAGE_ERROR;
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

    // Parse fields
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

    // Verify proof via daemon
    let verify_code = super::verify_cmd::cmd_verify(config_dir, event_type, field_strs, proof);
    if verify_code != 0 {
        return verify_code;
    }

    // Proof verified — record the event
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
pub fn cmd_reseal(config_dir: &str, proof: &str, targeting: &LedgerTargeting) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

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

    // Verify proof via daemon (payload is "config_reseal" with no fields)
    let verify_code = super::verify_cmd::cmd_verify(config_dir, "config_reseal", &[], proof);
    if verify_code != 0 {
        return verify_code;
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
