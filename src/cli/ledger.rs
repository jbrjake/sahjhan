// src/cli/ledger.rs
//
// Multi-ledger management commands.
//
// ## Index
// - [cmd-ledger-create] cmd_ledger_create() — register and initialize a new named ledger (direct or template-based)
// - [cmd-ledger-list] cmd_ledger_list() — list registered ledgers
// - [cmd-ledger-remove] cmd_ledger_remove() — remove a ledger from the registry
// - [cmd-ledger-verify] cmd_ledger_verify() — verify hash chain integrity of a ledger
// - [cmd-ledger-checkpoint] cmd_ledger_checkpoint() — write a checkpoint to a ledger
// - [cmd-ledger-import] cmd_ledger_import() — import bare JSONL from stdin

use std::path::{Path, PathBuf};

use crate::ledger::chain::Ledger;
use crate::ledger::import::import_jsonl;
use crate::ledger::registry::{LedgerMode, LedgerRegistry};

use super::commands::{
    compute_registry_path, ledger_path, load_config, registry_path_from_config, resolve_config_dir,
    resolve_data_dir, resolve_registry_path, EXIT_CONFIG_ERROR, EXIT_INTEGRITY_ERROR, EXIT_SUCCESS,
    EXIT_USAGE_ERROR,
};

// ---------------------------------------------------------------------------
// ledger create (Task 12)
// ---------------------------------------------------------------------------

// [cmd-ledger-create]
pub fn cmd_ledger_create(
    config_dir: &str,
    name: Option<&str>,
    path: Option<&str>,
    from_template: Option<&str>,
    instance_id: Option<&str>,
    mode_str: &str,
) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let mode = match mode_str {
        "stateful" => LedgerMode::Stateful,
        "event-only" => LedgerMode::EventOnly,
        other => {
            eprintln!(
                "Unknown ledger mode '{}'. Valid: stateful, event-only.",
                other
            );
            return EXIT_USAGE_ERROR;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);

    // Determine name, path, and template metadata based on creation mode
    let (ledger_name, ledger_file, tmpl_name, tmpl_id) = if let Some(template_name) = from_template
    {
        // --- Template-based creation ---
        let tmpl = match config.ledgers.get(template_name) {
            Some(t) => t,
            None => {
                eprintln!(
                    "No ledger template '{}' in protocol.toml. Available: {}",
                    template_name,
                    if config.ledgers.is_empty() {
                        "(none)".to_string()
                    } else {
                        let mut keys: Vec<_> = config.ledgers.keys().cloned().collect();
                        keys.sort();
                        keys.join(", ")
                    }
                );
                return EXIT_CONFIG_ERROR;
            }
        };

        let path_template = match &tmpl.path_template {
            Some(pt) => pt,
            None => {
                eprintln!(
                    "Ledger '{}' uses a fixed path, not a path_template. Use --name/--path instead.",
                    template_name
                );
                return EXIT_USAGE_ERROR;
            }
        };

        let id = match instance_id {
            Some(id) => id,
            None => {
                eprintln!(
                    "Template '{}' requires an instance_id (e.g., `ledger create --from {} 25`)",
                    template_name, template_name
                );
                return EXIT_USAGE_ERROR;
            }
        };

        let resolved_path = path_template
            .replace("{template.instance_id}", id)
            .replace("{template.name}", template_name);

        let derived_name = format!("{}-{}", template_name, id);
        let file = if PathBuf::from(&resolved_path).is_absolute() {
            PathBuf::from(&resolved_path)
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(&resolved_path)
        };

        (
            derived_name,
            file,
            Some(template_name.to_string()),
            Some(id.to_string()),
        )
    } else {
        // --- Direct creation ---
        let n = name.unwrap(); // clap ensures this is present when --from is absent
        let p = path.unwrap();

        let file = if PathBuf::from(p).is_absolute() {
            PathBuf::from(p)
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(p)
        };

        (n.to_string(), file, None, None)
    };

    // Initialize the ledger file
    if let Err(e) = std::fs::create_dir_all(ledger_file.parent().unwrap_or(Path::new("."))) {
        eprintln!("Cannot create directory for ledger: {}", e);
        return EXIT_CONFIG_ERROR;
    }

    match Ledger::init(
        &ledger_file,
        &config.protocol.name,
        &config.protocol.version,
    ) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Cannot initialize ledger: {}", e);
            return EXIT_INTEGRITY_ERROR;
        }
    }

    // Register in the registry
    let reg_path = registry_path_from_config(&config);
    let mut registry = match LedgerRegistry::new(&reg_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Cannot load registry: {}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    let registry_stored_path = compute_registry_path(&ledger_file, &data_dir);
    if let Err(e) = registry.create_with_template(
        &ledger_name,
        &registry_stored_path,
        mode,
        tmpl_name.as_deref(),
        tmpl_id.as_deref(),
    ) {
        eprintln!("Cannot register ledger: {}", e);
        return EXIT_CONFIG_ERROR;
    }

    println!(
        "Ledger '{}' created at {} and registered.",
        ledger_name,
        ledger_file.display()
    );
    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------
// ledger list (Task 12)
// ---------------------------------------------------------------------------

// [cmd-ledger-list]
pub fn cmd_ledger_list(config_dir: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let reg_path = registry_path_from_config(&config);
    let registry = match LedgerRegistry::new(&reg_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Cannot load registry: {}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    let entries = registry.list();
    if entries.is_empty() {
        println!("No ledgers registered.");
        return EXIT_SUCCESS;
    }

    println!("Registered ledgers ({}):", entries.len());
    for entry in entries {
        let mode_str = match entry.mode {
            LedgerMode::Stateful => "stateful",
            LedgerMode::EventOnly => "event-only",
        };
        println!(
            "  {} ({}) -> {} [{}]",
            entry.name, mode_str, entry.path, entry.created
        );
    }

    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------
// ledger remove (Task 12)
// ---------------------------------------------------------------------------

// [cmd-ledger-remove]
pub fn cmd_ledger_remove(config_dir: &str, name: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let reg_path = registry_path_from_config(&config);
    let mut registry = match LedgerRegistry::new(&reg_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Cannot load registry: {}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    if let Err(e) = registry.remove(name) {
        eprintln!("{}", e);
        return EXIT_CONFIG_ERROR;
    }

    println!(
        "Ledger '{}' removed from registry. File kept on disk.",
        name
    );
    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------
// ledger verify (Task 12)
// ---------------------------------------------------------------------------

// [cmd-ledger-verify]
pub fn cmd_ledger_verify(config_dir: &str, name: Option<&str>, path: Option<&str>) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    // Resolve which ledger to verify
    let ledger_file = if let Some(p) = path {
        PathBuf::from(p)
    } else if let Some(n) = name {
        let reg_path = registry_path_from_config(&config);
        let registry = match LedgerRegistry::new(&reg_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Cannot load registry: {}", e);
                return EXIT_CONFIG_ERROR;
            }
        };
        let entry = match registry.resolve(Some(n)) {
            Ok(e) => e.clone(),
            Err(e) => {
                eprintln!("{}", e);
                return EXIT_CONFIG_ERROR;
            }
        };
        resolve_registry_path(&entry.path, &config)
    } else {
        // Default: use config data_dir
        let data_dir = resolve_data_dir(&config.paths.data_dir);
        ledger_path(&data_dir)
    };

    let ledger = match Ledger::open(&ledger_file) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Cannot open ledger at {}: {}", ledger_file.display(), e);
            return EXIT_INTEGRITY_ERROR;
        }
    };

    match ledger.verify() {
        Ok(()) => {
            println!(
                "Chain valid. {} entries, all hashes check out. ({})",
                ledger.len(),
                ledger_file.display()
            );
            EXIT_SUCCESS
        }
        Err(e) => {
            eprintln!("Chain INVALID: {}", e);
            eprintln!("Tampering detected.");
            EXIT_INTEGRITY_ERROR
        }
    }
}

// ---------------------------------------------------------------------------
// ledger checkpoint (Task 12)
// ---------------------------------------------------------------------------

// [cmd-ledger-checkpoint]
pub fn cmd_ledger_checkpoint(config_dir: &str, name: &str, scope: &str, snapshot: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let reg_path = registry_path_from_config(&config);
    let registry = match LedgerRegistry::new(&reg_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Cannot load registry: {}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    let entry = match registry.resolve(Some(name)) {
        Ok(e) => e.clone(),
        Err(e) => {
            eprintln!("{}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    let ledger_file = resolve_registry_path(&entry.path, &config);
    let mut ledger = match Ledger::open(&ledger_file) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Cannot open ledger: {}", e);
            return EXIT_INTEGRITY_ERROR;
        }
    };

    match ledger.write_checkpoint(scope, snapshot) {
        Ok(cp) => {
            println!(
                "Checkpoint written at seq {} (scope='{}', snapshot='{}').",
                cp.seq, scope, snapshot
            );
            EXIT_SUCCESS
        }
        Err(e) => {
            eprintln!("Cannot write checkpoint: {}", e);
            EXIT_INTEGRITY_ERROR
        }
    }
}

// ---------------------------------------------------------------------------
// ledger import (Task 12)
// ---------------------------------------------------------------------------

// [cmd-ledger-import]
pub fn cmd_ledger_import(config_dir: &str, name: &str, path: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    // Resolve output path relative to cwd (not data_dir)
    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let ledger_file = if PathBuf::from(path).is_absolute() {
        PathBuf::from(path)
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(path)
    };

    if let Err(e) = std::fs::create_dir_all(ledger_file.parent().unwrap_or(Path::new("."))) {
        eprintln!("Cannot create directory for ledger: {}", e);
        return EXIT_CONFIG_ERROR;
    }

    // Read from stdin
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();

    match import_jsonl(
        &mut reader,
        &ledger_file,
        &config.protocol.name,
        &config.protocol.version,
    ) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("Import failed: {}", e);
            return EXIT_INTEGRITY_ERROR;
        }
    }

    // Register in the registry
    let reg_path = registry_path_from_config(&config);
    let mut registry = match LedgerRegistry::new(&reg_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Cannot load registry: {}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    let registry_stored_path = compute_registry_path(&ledger_file, &data_dir);
    if let Err(e) = registry.create(name, &registry_stored_path, LedgerMode::EventOnly) {
        eprintln!("Cannot register ledger: {}", e);
        return EXIT_CONFIG_ERROR;
    }

    println!(
        "Imported JSONL into '{}' at {} and registered.",
        name,
        ledger_file.display()
    );
    EXIT_SUCCESS
}
