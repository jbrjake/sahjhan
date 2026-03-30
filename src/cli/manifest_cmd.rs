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
use super::output::{CommandOutput, CommandResult, ManifestVerifyData, MismatchData};

// ---------------------------------------------------------------------------
// manifest verify
// ---------------------------------------------------------------------------

// [cmd-manifest-verify]
pub fn cmd_manifest_verify(config_dir: &str) -> Box<dyn CommandOutput> {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            return Box::new(CommandResult::<ManifestVerifyData>::err(
                "manifest_verify",
                code,
                "config_error",
                msg,
            ));
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let manifest = match load_manifest(&data_dir) {
        Ok(m) => m,
        Err((code, msg)) => {
            return Box::new(CommandResult::<ManifestVerifyData>::err(
                "manifest_verify",
                code,
                "integrity_error",
                msg,
            ));
        }
    };

    let tracked_count = manifest.entries.len();
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let result = manifest_verify::verify(&manifest, &cwd);

    let mismatches: Vec<MismatchData> = result
        .mismatches
        .iter()
        .map(|m| MismatchData {
            path: m.path.clone(),
            expected: m.expected.clone(),
            actual: m.actual.clone(),
        })
        .collect();

    let clean = result.clean;
    let data = ManifestVerifyData {
        clean,
        tracked_count,
        mismatches,
    };

    if clean {
        Box::new(CommandResult::ok("manifest_verify", data))
    } else {
        Box::new(CommandResult::ok_with_exit_code(
            "manifest_verify",
            data,
            EXIT_INTEGRITY_ERROR,
        ))
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
