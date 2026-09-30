// src/cli/init.rs
//
// Initialization, validation, and reset commands.
//
// ## Index
// - [cmd-init] cmd_init() — initialize ledger, manifest, genesis
// - [cmd-validate] cmd_validate() — validate protocol config
// - [cmd-reset] cmd_reset() — archive and reset run (requires HMAC proof via daemon)

use crate::config::ProtocolConfig;
use crate::manifest::tracker::Manifest;

use super::commands::{
    ledger_path, load_config, manifest_path, open_ledger, remove_active_ledger, resolve_config_dir,
    resolve_data_dir, resolve_project_root, save_manifest, write_status_cache, EXIT_CONFIG_ERROR,
    EXIT_INTEGRITY_ERROR, EXIT_SUCCESS, EXIT_USAGE_ERROR,
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

    // Every refusal that needs no write comes before the first write. The
    // genesis seals the config as it is now, so a genesis left behind by a
    // refused init makes the retry "already initialized" and every other
    // command read the corrected config as tampering.
    let mut manifest = match Manifest::init(&config.paths.data_dir, config.paths.managed.clone()) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let lp = ledger_path(&data_dir);
    if lp.exists() {
        eprintln!(
            "error: already initialized ({}). run reset first.",
            lp.display()
        );
        return EXIT_USAGE_ERROR;
    }

    // A write that fails once the first has landed takes back the files, and
    // the data dir, that this init created, and nothing that was already
    // there. The retry then finds no ledger and starts over.
    let reg_path = data_dir.join("ledgers.toml");
    let mp = manifest_path(&data_dir);
    let data_dir_existed = data_dir.exists();
    let registry_existed = reg_path.exists();
    let manifest_existed = mp.exists();
    if let Err((code, msg)) = write_run(&config, &config_path, &data_dir, &lp, &mut manifest) {
        eprintln!("{}", msg);
        let _ = std::fs::remove_file(&lp);
        if !registry_existed {
            let _ = std::fs::remove_file(&reg_path);
        }
        if !manifest_existed {
            let _ = std::fs::remove_file(&mp);
        }
        if !data_dir_existed {
            let _ = std::fs::remove_dir(&data_dir);
        }
        return code;
    }

    // Write status cache for fast hook discovery
    let initial_state = config.initial_state().unwrap_or("unknown").to_string();
    write_status_cache(&data_dir, &config, &config_path, &initial_state);

    println!("initialized. good luck.");
    EXIT_SUCCESS
}

/// The writes `init` makes: the data dir, the sealed genesis, the registry's
/// "default" entry, and the manifest tracking the ledger. The first `Err`
/// stops the rest; undoing what landed is the caller's.
fn write_run(
    config: &ProtocolConfig,
    config_path: &std::path::Path,
    data_dir: &std::path::Path,
    lp: &std::path::Path,
    manifest: &mut Manifest,
) -> Result<(), (i32, String)> {
    std::fs::create_dir_all(data_dir).map_err(|e| {
        (
            EXIT_CONFIG_ERROR,
            format!("error: cannot create data directory: {}", e),
        )
    })?;

    // Compute config integrity seals
    let config_seals = crate::config::compute_config_seals(config_path);

    // Initialize ledger with genesis block (including config seals)
    crate::ledger::chain::Ledger::init_with_seals(
        lp,
        &config.protocol.name,
        &config.protocol.version,
        config_seals,
    )
    .map_err(|e| {
        (
            EXIT_INTEGRITY_ERROR,
            format!("error: cannot initialize ledger: {}", e),
        )
    })?;

    // Create ledgers.toml registry with a "default" entry pointing to the new ledger
    let reg_path = data_dir.join("ledgers.toml");
    // Relative path from data_dir to ledger (just the filename)
    let ledger_rel_to_data = lp
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "ledger.jsonl".to_string());
    let mut registry = crate::ledger::registry::LedgerRegistry::new(&reg_path).map_err(|e| {
        (
            EXIT_INTEGRITY_ERROR,
            format!("error: cannot create ledger registry: {}", e),
        )
    })?;
    if let Err(e) = registry.create(
        "default",
        &ledger_rel_to_data,
        crate::ledger::registry::LedgerMode::Stateful,
    ) {
        // If the registry already has a "default" entry, skip — idempotent.
        if !e.contains("already exists") {
            return Err((
                EXIT_INTEGRITY_ERROR,
                format!("error: cannot register default ledger: {}", e),
            ));
        }
    }

    // Track the ledger file in the manifest, keyed against the project root
    // rather than the cwd `init` happened to run from (holtz #85).
    let root = resolve_project_root(&config.paths.data_dir);
    let ledger_rel = crate::paths::manifest_key(lp, &root);
    manifest.track(&ledger_rel, lp, "genesis", 0).map_err(|e| {
        (
            EXIT_INTEGRITY_ERROR,
            format!("error: cannot track ledger in manifest: {}", e),
        )
    })?;

    save_manifest(manifest, data_dir)
}

// ---------------------------------------------------------------------------
// reset
// ---------------------------------------------------------------------------

// [cmd-reset]
pub fn cmd_reset(config_dir: &str, confirm: bool, proof: &str) -> i32 {
    if !confirm {
        eprintln!("error: reset requires --confirm and --proof");
        return EXIT_USAGE_ERROR;
    }

    // Verify proof via daemon before doing anything destructive
    let verify_code = super::verify_cmd::cmd_verify(config_dir, "reset", &[], proof);
    if verify_code != 0 {
        eprintln!("error: reset requires a valid proof from the daemon");
        eprintln!("hint: sahjhan sign --event-type reset | xargs -I{{}} sahjhan reset --confirm --proof {{}}");
        return verify_code;
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
    // Open ledger (just to confirm it exists — proof already verified)
    if let Err((code, msg)) = open_ledger(&data_dir, &config_path) {
        eprintln!("{}", msg);
        return code;
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
