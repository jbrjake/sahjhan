// src/daemon/auth.rs
//
// Caller authentication for the daemon. Loads a trusted-callers manifest,
// resolves the calling script from PID metadata, and verifies its hash.
//
// ## Index
// - TrustedCallersManifest    — manifest struct + loader
// - TrustedCallersManifest::verify_caller — path lookup + SHA-256 verification
// - extract_script_path       — extract script path from interpreter cmdline
// - AuthError                 — authentication error type
// - AuthError::reason_code    — map error to diagnostic reason code (issue #26)
// - find_trusted_ancestor     — walk process ancestor chain looking for trusted script
// - authenticate_peer         — PID-based caller authentication via ancestor walk

use crate::daemon::platform;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("caller not in manifest: {path}")]
    NotInManifest { path: String },
    #[error("hash mismatch for {path}: expected {expected}, got {actual}")]
    HashMismatch {
        path: String,
        expected: String,
        actual: String,
    },
    #[error("script file not found: {0}")]
    ScriptNotFound(PathBuf),
    #[error("no script path found in caller cmdline")]
    NoScriptPath,
    #[error("manifest load error: {0}")]
    ManifestLoad(#[from] std::io::Error),
    #[error("manifest parse error: {0}")]
    ManifestParse(#[from] toml::de::Error),
    #[error("platform error: {0}")]
    Platform(String),
}

#[derive(Debug, Deserialize)]
pub struct TrustedCallersManifest {
    pub callers: HashMap<String, String>,
}

impl TrustedCallersManifest {
    pub fn load(path: &Path) -> Result<Self, AuthError> {
        let content = std::fs::read_to_string(path)?;
        let manifest: TrustedCallersManifest = toml::from_str(&content)?;
        Ok(manifest)
    }

    pub fn verify_caller(&self, plugin_root: &Path, relative_path: &str) -> Result<(), AuthError> {
        let expected_hash =
            self.callers
                .get(relative_path)
                .ok_or_else(|| AuthError::NotInManifest {
                    path: relative_path.to_string(),
                })?;

        let full_path = plugin_root.join(relative_path);
        if !full_path.exists() {
            return Err(AuthError::ScriptNotFound(full_path));
        }

        let content = std::fs::read(&full_path).map_err(AuthError::ManifestLoad)?;
        let actual_hash = format!("sha256:{}", hex::encode(Sha256::digest(&content)));

        if actual_hash != *expected_hash {
            return Err(AuthError::HashMismatch {
                path: relative_path.to_string(),
                expected: expected_hash.clone(),
                actual: actual_hash,
            });
        }

        Ok(())
    }
}

impl AuthError {
    /// Map this error to one of the reason codes specified in issue #26.
    ///
    /// - `pid_resolution_failed` — could not resolve caller PID to a script path
    /// - `hash_mismatch` — script resolved but hash doesn't match trusted-callers.toml
    /// - `peer_cred_unavailable` — platform doesn't support LOCAL_PEERCRED or equivalent
    pub fn reason_code(&self) -> &'static str {
        match self {
            AuthError::HashMismatch { .. } => "hash_mismatch",
            AuthError::Platform(msg) if msg.contains("peer PID") || msg.contains("PEERCRED") => {
                "peer_cred_unavailable"
            }
            // Everything else is a PID resolution chain failure
            _ => "pid_resolution_failed",
        }
    }
}

pub fn extract_script_path(args: &[String]) -> Option<String> {
    for arg in args.iter().skip(1) {
        if !arg.starts_with('-') {
            return Some(arg.clone());
        }
    }
    None
}

/// Walk up the process ancestor chain looking for a trusted script.
///
/// Starting from `start_pid`, examines the cmdline of each ancestor for a
/// script path under `plugin_root` that appears in the manifest. Walks up
/// to `MAX_ANCESTOR_DEPTH` levels to avoid runaway loops.
///
/// Returns the relative path of the matched script on success.
fn find_trusted_ancestor(
    start_pid: u32,
    manifest: &TrustedCallersManifest,
    plugin_root_canonical: &Path,
) -> Result<(), AuthError> {
    const MAX_ANCESTOR_DEPTH: usize = 10;
    let mut current_pid = start_pid;

    for depth in 0..MAX_ANCESTOR_DEPTH {
        if current_pid <= 1 {
            // Hit init/launchd — no more ancestors to check
            break;
        }

        let cmdline = match platform::get_cmdline(current_pid) {
            Ok(args) => args,
            Err(e) => {
                eprintln!(
                    "auth: depth {}: cannot get cmdline for PID {}: {}",
                    depth, current_pid, e
                );
                break;
            }
        };

        eprintln!(
            "auth: depth {}: PID {} cmdline = {:?}",
            depth, current_pid, cmdline
        );

        if let Some(script_path_str) = extract_script_path(&cmdline) {
            let script_path = std::path::Path::new(&script_path_str);
            if let Ok(canonical) = script_path.canonicalize() {
                if let Ok(relative) = canonical.strip_prefix(plugin_root_canonical) {
                    let relative_str = relative.to_string_lossy();
                    eprintln!(
                        "auth: depth {}: found candidate script '{}'",
                        depth, relative_str
                    );
                    // Found a script under plugin root — verify its hash
                    return manifest.verify_caller(plugin_root_canonical, &relative_str);
                }
            }
        }

        // Move to parent
        current_pid = match platform::get_parent_pid(current_pid) {
            Ok(ppid) => ppid,
            Err(e) => {
                eprintln!(
                    "auth: depth {}: cannot get parent PID of {}: {}",
                    depth, current_pid, e
                );
                break;
            }
        };
    }

    Err(AuthError::NoScriptPath)
}

/// Authenticate a connected peer via PID resolution + ancestor walk.
///
/// Resolves the peer PID from the socket, then walks up the process tree
/// looking for a script that matches the trusted-callers manifest. If the
/// peer is our own binary (CLI-mediated connection like `sahjhan sign`),
/// starts the walk from the parent; otherwise starts from the peer itself.
///
/// This ancestor-chain walk handles deep process trees common on macOS
/// where shell intermediaries (bash, zsh) sit between the hook script
/// and the sahjhan CLI process.
pub fn authenticate_peer(
    stream: &UnixStream,
    manifest: &TrustedCallersManifest,
    plugin_root: &Path,
) -> Result<(), AuthError> {
    let peer_pid = platform::get_peer_pid(stream)
        .map_err(|e| AuthError::Platform(format!("cannot get peer PID: {}", e)))?;

    let peer_exe = platform::get_exe_path(peer_pid)
        .map_err(|e| AuthError::Platform(format!("cannot get peer exe: {}", e)))?;
    let our_exe = std::env::current_exe()
        .map_err(|e| AuthError::Platform(format!("cannot get own exe: {}", e)))?;

    eprintln!(
        "auth: peer PID={}, exe={}, our_exe={}",
        peer_pid,
        peer_exe.display(),
        our_exe.display()
    );

    // Start from parent if peer is our own binary (CLI-mediated),
    // otherwise start from the peer itself
    let start_pid = if peer_exe == our_exe {
        platform::get_parent_pid(peer_pid)
            .map_err(|e| AuthError::Platform(format!("cannot get parent PID: {}", e)))?
    } else {
        peer_pid
    };

    let plugin_root_canonical = plugin_root
        .canonicalize()
        .map_err(|e| AuthError::Platform(format!("cannot canonicalize plugin root: {}", e)))?;

    find_trusted_ancestor(start_pid, manifest, &plugin_root_canonical)
}
