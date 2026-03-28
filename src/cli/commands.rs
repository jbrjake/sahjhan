// src/cli/commands.rs
//
// Shared types, exit codes, and helper functions used by all command modules.
//
// ## Index
// - [exit-codes] EXIT_SUCCESS, EXIT_GATE_FAILED, etc. — process exit codes
// - [ledger-targeting] LedgerTargeting — global --ledger / --ledger-path flags
// - [load-config] load_config() — load and validate protocol config
// - [resolve-config-dir] resolve_config_dir() — resolve config path relative to cwd
// - [resolve-data-dir] resolve_data_dir() — resolve data path relative to cwd
// - [ledger-path] ledger_path() — canonical ledger file path
// - [manifest-path] manifest_path() — canonical manifest file path
// - [open-ledger] open_ledger() — open ledger from data_dir
// - [load-manifest] load_manifest() — load manifest from data_dir
// - [save-manifest] save_manifest() — save manifest to data_dir
// - [track-ledger] track_ledger_in_manifest() — track ledger in manifest
// - [pathdiff] pathdiff() — compute relative path
// - [resolve-ledger] resolve_ledger_from_targeting() — resolve ledger path from flags
// - [open-targeted] open_targeted_ledger() — open ledger via targeting
// - [registry-path] registry_path_from_config() — registry path from config
// - [resolve-registry] resolve_registry_path() — resolve registry entry path
// - [guard-event-only] guard_event_only() — block stateful ops on event-only ledgers
// - [build-state-params] build_state_params() — build state params for gate context
// - [compute-registry-path] compute_registry_path() — compute registry-storable path for a ledger file
// - [hex-encode-short] hex_encode_short() — short hex encoding of hash bytes
// - [atty-check] atty_check() — check if stdin is a TTY

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::ProtocolConfig;
use crate::ledger::chain::Ledger;
use crate::ledger::registry::{LedgerMode, LedgerRegistry};
use crate::manifest::tracker::Manifest;

// ---------------------------------------------------------------------------
// Exit codes (E18)
// ---------------------------------------------------------------------------

// [exit-codes]
pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_GATE_FAILED: i32 = 1;
pub const EXIT_INTEGRITY_ERROR: i32 = 2;
pub const EXIT_CONFIG_ERROR: i32 = 3;
pub const EXIT_USAGE_ERROR: i32 = 4;

// ---------------------------------------------------------------------------
// Ledger targeting (Task 14)
// ---------------------------------------------------------------------------

// [ledger-targeting]
/// Captures global --ledger / --ledger-path flags for ledger resolution.
pub struct LedgerTargeting {
    pub ledger_name: Option<String>,
    pub ledger_path: Option<String>,
}

// ---------------------------------------------------------------------------
// Helper: load config with validation
// ---------------------------------------------------------------------------

// [load-config]
pub(crate) fn load_config(config_dir: &Path) -> Result<ProtocolConfig, (i32, String)> {
    let config = ProtocolConfig::load(config_dir)
        .map_err(|e| (EXIT_CONFIG_ERROR, format!("Configuration error: {}", e)))?;

    let errors = config.validate();
    if !errors.is_empty() {
        return Err((
            EXIT_CONFIG_ERROR,
            format!(
                "Configuration validation failed:\n{}",
                errors
                    .iter()
                    .map(|e| format!("  - {}", e))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        ));
    }

    Ok(config)
}

// [resolve-config-dir]
/// Resolve config_dir relative to cwd.
pub(crate) fn resolve_config_dir(config_dir: &str) -> PathBuf {
    let p = PathBuf::from(config_dir);
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(p)
    }
}

// [resolve-data-dir]
/// Resolve data_dir relative to cwd.
pub(crate) fn resolve_data_dir(data_dir: &str) -> PathBuf {
    let p = PathBuf::from(data_dir);
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(p)
    }
}

// [ledger-path]
pub(crate) fn ledger_path(data_dir: &Path) -> PathBuf {
    data_dir.join("ledger.jsonl")
}

// [manifest-path]
pub(crate) fn manifest_path(data_dir: &Path) -> PathBuf {
    data_dir.join("manifest.json")
}

// [open-ledger]
pub(crate) fn open_ledger(data_dir: &Path) -> Result<Ledger, (i32, String)> {
    Ledger::open(&ledger_path(data_dir))
        .map_err(|e| (EXIT_INTEGRITY_ERROR, format!("Cannot open ledger: {}", e)))
}

// [load-manifest]
pub(crate) fn load_manifest(data_dir: &Path) -> Result<Manifest, (i32, String)> {
    Manifest::load(&manifest_path(data_dir))
        .map_err(|e| (EXIT_INTEGRITY_ERROR, format!("Cannot load manifest: {}", e)))
}

// [save-manifest]
pub(crate) fn save_manifest(manifest: &mut Manifest, data_dir: &Path) -> Result<(), (i32, String)> {
    manifest
        .save(&manifest_path(data_dir))
        .map_err(|e| (EXIT_INTEGRITY_ERROR, format!("Cannot save manifest: {}", e)))
}

// [track-ledger]
pub(crate) fn track_ledger_in_manifest(
    manifest: &mut Manifest,
    data_dir: &Path,
    ledger: &Ledger,
) -> Result<(), (i32, String)> {
    let lp = ledger_path(data_dir);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let rel = pathdiff(&lp, &cwd);
    manifest
        .track(
            &rel,
            &lp,
            "ledger_append",
            ledger.entries().last().unwrap().seq,
        )
        .map_err(|e| (EXIT_INTEGRITY_ERROR, format!("Cannot track ledger: {}", e)))
}

// [pathdiff]
/// Compute a relative path from `base` to `target`.
pub(crate) fn pathdiff(target: &Path, base: &Path) -> String {
    // Try to strip the base prefix; if it fails, use the target as-is.
    target
        .strip_prefix(base)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| target.to_string_lossy().to_string())
}

// ---------------------------------------------------------------------------
// Ledger resolution (Task 14)
// ---------------------------------------------------------------------------

// [resolve-ledger]
/// Resolve a ledger path from targeting flags.
///
/// 1. If --ledger-path given, use that file directly.
/// 2. If --ledger given, resolve from registry.
/// 3. If neither, try registry first entry; else fall back to config data_dir/ledger.jsonl.
pub(crate) fn resolve_ledger_from_targeting(
    config: &ProtocolConfig,
    targeting: &LedgerTargeting,
) -> Result<(PathBuf, Option<LedgerMode>), (i32, String)> {
    // 1. Direct path
    if let Some(ref lp) = targeting.ledger_path {
        let p = PathBuf::from(lp);
        return Ok((p, None));
    }

    // 2. Named ledger from registry
    if let Some(ref name) = targeting.ledger_name {
        let reg_path = registry_path_from_config(config);
        let registry = LedgerRegistry::new(&reg_path).map_err(|e| {
            (
                EXIT_CONFIG_ERROR,
                format!("Cannot load ledger registry: {}", e),
            )
        })?;
        let entry = registry.resolve(Some(name)).map_err(|e| {
            (
                EXIT_CONFIG_ERROR,
                format!("Ledger resolution failed: {}", e),
            )
        })?;
        let resolved = resolve_registry_path(&entry.path, config);
        return Ok((resolved, Some(entry.mode.clone())));
    }

    // 3. Default: try registry first, else fall back to data_dir/ledger.jsonl
    let reg_path = registry_path_from_config(config);
    if reg_path.exists() {
        if let Ok(registry) = LedgerRegistry::new(&reg_path) {
            if let Ok(entry) = registry.resolve(None) {
                let resolved = resolve_registry_path(&entry.path, config);
                return Ok((resolved, Some(entry.mode.clone())));
            }
        }
    }

    // Fall back to default ledger path
    let data_dir = resolve_data_dir(&config.paths.data_dir);
    Ok((ledger_path(&data_dir), None))
}

// [open-targeted]
/// Open a ledger using targeting flags.
pub(crate) fn open_targeted_ledger(
    config: &ProtocolConfig,
    targeting: &LedgerTargeting,
) -> Result<(Ledger, Option<LedgerMode>), (i32, String)> {
    let (path, mode) = resolve_ledger_from_targeting(config, targeting)?;
    let ledger = Ledger::open(&path)
        .map_err(|e| (EXIT_INTEGRITY_ERROR, format!("Cannot open ledger: {}", e)))?;
    Ok((ledger, mode))
}

// [registry-path]
/// Compute the registry path relative to the config's data_dir.
pub(crate) fn registry_path_from_config(config: &ProtocolConfig) -> PathBuf {
    let data_dir = resolve_data_dir(&config.paths.data_dir);
    data_dir.join("ledgers.toml")
}

// [resolve-registry]
/// Resolve a registry entry path (relative to data_dir) to an absolute path.
pub(crate) fn resolve_registry_path(entry_path: &str, config: &ProtocolConfig) -> PathBuf {
    let p = PathBuf::from(entry_path);
    if p.is_absolute() {
        p
    } else {
        let data_dir = resolve_data_dir(&config.paths.data_dir);
        data_dir.join(p)
    }
}

// [guard-event-only]
/// Guard: check if a ledger mode is event-only and block stateful operations.
pub(crate) fn guard_event_only(
    mode: &Option<LedgerMode>,
    operation: &str,
) -> Result<(), (i32, String)> {
    if let Some(LedgerMode::EventOnly) = mode {
        Err((
            EXIT_CONFIG_ERROR,
            format!(
                "Cannot {} on an event-only ledger. This ledger has no state machine.",
                operation
            ),
        ))
    } else {
        Ok(())
    }
}

// [build-state-params]
/// Build state_params for a target state (mirrors StateMachine::build_state_params).
///
/// Supports `StateParam.source`:
/// - `"values"` (default): comma-joined set values
/// - `"current"`: first incomplete member of the set (requires ledger scan)
/// - `"last_completed"`: most recently completed member (requires ledger scan)
pub(crate) fn build_state_params(
    config: &ProtocolConfig,
    state_name: &str,
    ledger: &crate::ledger::chain::Ledger,
) -> HashMap<String, String> {
    let mut params = HashMap::new();
    if let Some(state_config) = config.states.get(state_name) {
        if let Some(state_params) = &state_config.params {
            for param in state_params {
                let source = param.source.as_deref().unwrap_or("values");
                match source {
                    "current" => {
                        if let Some(set_config) = config.sets.get(&param.set) {
                            let completed = completed_members_for_set(ledger, &param.set);
                            if let Some(current) =
                                set_config.values.iter().find(|v| !completed.contains(v))
                            {
                                params.insert(param.name.clone(), current.clone());
                            }
                        }
                    }
                    "last_completed" => {
                        let completed = completed_members_for_set(ledger, &param.set);
                        if let Some(last) = completed.last() {
                            params.insert(param.name.clone(), last.clone());
                        }
                    }
                    _ => {
                        if let Some(set_config) = config.sets.get(&param.set) {
                            params.insert(param.name.clone(), set_config.values.join(","));
                        }
                    }
                }
            }
        }
    }
    params
}

/// Scan ledger for completed members of a set.
fn completed_members_for_set(ledger: &crate::ledger::chain::Ledger, set_name: &str) -> Vec<String> {
    let mut covered = Vec::new();
    for entry in ledger.events_of_type("set_member_complete") {
        let set_matches = entry
            .fields
            .get("set")
            .map(|v| v.as_str() == set_name)
            .unwrap_or(false);
        if set_matches {
            if let Some(member) = entry.fields.get("member") {
                if !covered.contains(member) {
                    covered.push(member.clone());
                }
            }
        }
    }
    covered
}

// [compute-registry-path]
/// Compute the path to store in the registry for a ledger file.
///
/// If `file` is under `data_dir`, returns the relative sub-path so that
/// `resolve_registry_path` (which joins relative paths with `data_dir`) will
/// round-trip correctly. Otherwise returns the absolute path.
pub(crate) fn compute_registry_path(file: &Path, data_dir: &Path) -> String {
    match file.strip_prefix(data_dir) {
        Ok(rel) => rel.to_string_lossy().to_string(),
        Err(_) => file.to_string_lossy().to_string(),
    }
}

// [hex-encode-short]
pub(crate) fn hex_encode_short(bytes: &[u8; 32], len: usize) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()[..len]
        .to_string()
}

// [atty-check]
/// Check if stdin is a TTY (rough heuristic).
pub(crate) fn atty_check() -> bool {
    // Simple heuristic: check if stdin is a terminal via libc.
    // For portability, we'll just return true and note this is a stub.
    // A full implementation would use the `atty` crate or libc isatty.
    unsafe { libc_isatty() }
}

#[cfg(unix)]
unsafe fn libc_isatty() -> bool {
    extern "C" {
        fn isatty(fd: i32) -> i32;
    }
    unsafe { isatty(0) != 0 }
}

#[cfg(not(unix))]
unsafe fn libc_isatty() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn test_compute_registry_path_under_data_dir() {
        let data_dir = PathBuf::from("/project/.sahjhan");
        let file = PathBuf::from("/project/.sahjhan/runs/25/ledger.jsonl");
        let result = compute_registry_path(&file, &data_dir);
        assert_eq!(result, "runs/25/ledger.jsonl");
    }

    #[test]
    fn test_compute_registry_path_outside_data_dir() {
        let data_dir = PathBuf::from("/project/.sahjhan");
        let file = PathBuf::from("/project/docs/runs/25/ledger.jsonl");
        let result = compute_registry_path(&file, &data_dir);
        assert_eq!(result, "/project/docs/runs/25/ledger.jsonl");
    }

    #[test]
    fn test_compute_registry_path_absolute_preserved() {
        let data_dir = PathBuf::from("/project/.sahjhan");
        let file = PathBuf::from("/tmp/ledger.jsonl");
        let result = compute_registry_path(&file, &data_dir);
        assert_eq!(result, "/tmp/ledger.jsonl");
    }
}
