// src/cli/init.rs
//
// Initialization, validation, and reset commands.
//
// ## Index
// - [cmd-init] cmd_init() — initialize ledger, manifest, genesis
// - [cmd-validate] cmd_validate() — validate protocol config
// - [cmd-reset] cmd_reset() — archive and reset run

use std::path::PathBuf;

use crate::config::ProtocolConfig;
use crate::manifest::tracker::Manifest;

use super::commands::{
    atty_check, hex_encode_short, ledger_path, load_config, manifest_path, open_ledger, pathdiff,
    remove_active_ledger, resolve_config_dir, resolve_data_dir, save_manifest, write_status_cache,
    EXIT_CONFIG_ERROR, EXIT_INTEGRITY_ERROR, EXIT_SUCCESS, EXIT_USAGE_ERROR,
};

// ---------------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------------

// [cmd-validate]
pub fn cmd_validate(config_dir: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);

    // Load the config (parse-level errors)
    let config = match ProtocolConfig::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    // Run deep validation
    let (errors, warnings) = config.validate_deep(&config_path);

    // Print warnings first
    for w in &warnings {
        eprintln!("warning: {}", w);
    }

    if errors.is_empty() {
        println!("valid.");
        EXIT_SUCCESS
    } else {
        for e in &errors {
            eprintln!("error: {}", e);
        }
        EXIT_CONFIG_ERROR
    }
}

// ---------------------------------------------------------------------------
// init
// ---------------------------------------------------------------------------

// [cmd-init]
pub fn cmd_init(config_dir: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);

    // Create data_dir
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        eprintln!("error: cannot create data directory: {}", e);
        return EXIT_CONFIG_ERROR;
    }

    let lp = ledger_path(&data_dir);
    if lp.exists() {
        eprintln!(
            "error: already initialized ({}). run reset first.",
            lp.display()
        );
        return EXIT_USAGE_ERROR;
    }

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

    // Create ledgers.toml registry with a "default" entry pointing to the new ledger
    {
        let reg_path = data_dir.join("ledgers.toml");
        // Relative path from data_dir to ledger (just the filename)
        let ledger_rel_to_data = lp
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "ledger.jsonl".to_string());
        let mut registry = match crate::ledger::registry::LedgerRegistry::new(&reg_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("error: cannot create ledger registry: {}", e);
                return EXIT_INTEGRITY_ERROR;
            }
        };
        if let Err(e) = registry.create(
            "default",
            &ledger_rel_to_data,
            crate::ledger::registry::LedgerMode::Stateful,
        ) {
            // If the registry already has a "default" entry, skip — idempotent.
            if !e.contains("already exists") {
                eprintln!("error: cannot register default ledger: {}", e);
                return EXIT_INTEGRITY_ERROR;
            }
        }
    }

    // Initialize manifest
    let mut manifest = match Manifest::init(&config.paths.data_dir, config.paths.managed.clone()) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    // Track the ledger file in the manifest
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let ledger_rel = pathdiff(&lp, &cwd);
    if let Err(e) = manifest.track(&ledger_rel, &lp, "genesis", 0) {
        eprintln!("error: cannot track ledger in manifest: {}", e);
        return EXIT_INTEGRITY_ERROR;
    }

    // Save manifest
    if let Err((code, msg)) = save_manifest(&mut manifest, &data_dir) {
        eprintln!("{}", msg);
        return code;
    }

    // Write status cache for fast hook discovery
    let initial_state = config.initial_state().unwrap_or("unknown").to_string();
    write_status_cache(&data_dir, &config, &config_path, &initial_state);

    println!("initialized. good luck.");
    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------
// reset
// ---------------------------------------------------------------------------

// [cmd-reset]
pub fn cmd_reset(config_dir: &str, confirm: bool, token: &Option<String>) -> i32 {
    if !confirm {
        eprintln!("error: reset requires --confirm");
        return EXIT_USAGE_ERROR;
    }

    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let ledger = match open_ledger(&data_dir, &config_path) {
        Ok(l) => l,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    // Derive confirmation token from genesis hash
    let genesis_hash = ledger
        .entries()
        .first()
        .map(|e| e.entry_hash)
        .unwrap_or([0u8; 32]);
    let token_str = hex_encode_short(&genesis_hash, 6);

    // Check if piped (not a TTY) — record violation
    let is_tty = atty_check();

    match token {
        Some(provided_token) if provided_token == &token_str => {
            // Token matches — proceed with reset
            if !is_tty {
                // Programmatic invocation — record violation before reset
                eprintln!("warning: reset invoked programmatically");
            }

            // Archive current ledger and manifest
            let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
            let ledger_archive = data_dir.join(format!("ledger.{}.jsonl", timestamp));
            let manifest_archive = data_dir.join(format!("manifest.{}.json", timestamp));

            let lp = ledger_path(&data_dir);
            let mp = manifest_path(&data_dir);

            if let Err(e) = std::fs::rename(&lp, &ledger_archive) {
                eprintln!("error: cannot archive ledger: {}", e);
                return EXIT_INTEGRITY_ERROR;
            }
            if let Err(e) = std::fs::rename(&mp, &manifest_archive) {
                eprintln!("error: cannot archive manifest: {}", e);
                return EXIT_INTEGRITY_ERROR;
            }

            // Remove active-ledger marker (#25)
            remove_active_ledger(&data_dir);

            // Reinitialize
            let result = cmd_init(config_dir);
            if result == EXIT_SUCCESS {
                println!("reset. prior run archived.");
            }
            result
        }
        Some(provided_token) => {
            eprintln!(
                "error: token mismatch. expected '{}', got '{}'",
                token_str, provided_token
            );
            EXIT_USAGE_ERROR
        }
        None => {
            // Display token and prompt
            println!("reset requires --token {}", token_str);
            EXIT_USAGE_ERROR
        }
    }
}
