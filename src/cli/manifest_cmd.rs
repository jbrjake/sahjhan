// src/cli/manifest_cmd.rs
//
// Manifest verification, listing, and restore commands.
//
// ## Index
// - [cmd-manifest-verify] cmd_manifest_verify() — check managed files against manifest
// - [cmd-manifest-list] cmd_manifest_list() — show managed files and hashes
// - [cmd-manifest-restore] cmd_manifest_restore() — restore file from last known-good state

use std::path::PathBuf;

use crate::manifest::verify as manifest_verify;

use super::commands::{
    load_config, load_manifest, resolve_config_dir, resolve_data_dir, EXIT_INTEGRITY_ERROR,
    EXIT_SUCCESS, EXIT_USAGE_ERROR,
};

// ---------------------------------------------------------------------------
// manifest verify
// ---------------------------------------------------------------------------

// [cmd-manifest-verify]
pub fn cmd_manifest_verify(config_dir: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let manifest = match load_manifest(&data_dir) {
        Ok(m) => m,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let result = manifest_verify::verify(&manifest, &cwd);

    if result.clean {
        println!("manifest clean ({} tracked)", manifest.entries.len());
        EXIT_SUCCESS
    } else {
        eprintln!("manifest: {} modified", result.mismatches.len());
        for m in &result.mismatches {
            let actual_str = match &m.actual {
                Some(h) => format!("got {}", &h[..12]),
                None => "missing".to_string(),
            };
            eprintln!(
                "  {} \u{2014} expected {}, {}",
                m.path,
                &m.expected[..12],
                actual_str
            );
        }
        EXIT_INTEGRITY_ERROR
    }
}

// ---------------------------------------------------------------------------
// manifest list
// ---------------------------------------------------------------------------

// [cmd-manifest-list]
pub fn cmd_manifest_list(config_dir: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let manifest = match load_manifest(&data_dir) {
        Ok(m) => m,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let mut paths: Vec<_> = manifest.entries.keys().collect();
    paths.sort();
    for path in paths {
        let entry = &manifest.entries[path];
        println!(
            "{} {} ({})",
            &entry.sha256[..12],
            path,
            entry.last_operation
        );
    }

    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------
// manifest restore
// ---------------------------------------------------------------------------

// [cmd-manifest-restore]
pub fn cmd_manifest_restore(config_dir: &str, path: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let manifest = match load_manifest(&data_dir) {
        Ok(m) => m,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let action = manifest.restore_instruction(path);
    match action {
        crate::manifest::tracker::RestoreAction::ReRender {
            path: p,
            ledger_seq,
        } => {
            println!("restore: re-render {} (last tracked seq {})", p, ledger_seq);
            EXIT_SUCCESS
        }
        crate::manifest::tracker::RestoreAction::GitCheckout { path: p } => {
            println!("restore: git checkout -- {}", p);
            EXIT_SUCCESS
        }
        crate::manifest::tracker::RestoreAction::NotTracked { path: p } => {
            eprintln!("error: '{}' not tracked", p);
            EXIT_USAGE_ERROR
        }
    }
}
