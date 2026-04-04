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
// - authenticate_peer         — PID-based caller authentication for Unix socket connections

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

pub fn extract_script_path(args: &[String]) -> Option<String> {
    for arg in args.iter().skip(1) {
        if !arg.starts_with('-') {
            return Some(arg.clone());
        }
    }
    None
}

/// Authenticate a connected peer via PID resolution + manifest check.
///
/// For CLI-mediated connections (peer exe matches our own binary):
///   peer PID → parent PID → cmdline → script path → manifest lookup → hash check
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

    // If peer is our own binary (CLI-mediated), walk up to parent.
    let target_pid = if peer_exe == our_exe {
        platform::get_parent_pid(peer_pid)
            .map_err(|e| AuthError::Platform(format!("cannot get parent PID: {}", e)))?
    } else {
        peer_pid
    };

    let cmdline = platform::get_cmdline(target_pid)
        .map_err(|e| {
            AuthError::Platform(format!("cannot get cmdline for PID {}: {}", target_pid, e))
        })?;

    let script_path_str = extract_script_path(&cmdline).ok_or(AuthError::NoScriptPath)?;

    let script_path = std::path::Path::new(&script_path_str);
    let canonical = script_path.canonicalize().map_err(|e| {
        AuthError::Platform(format!("cannot canonicalize {}: {}", script_path_str, e))
    })?;
    let plugin_root_canonical = plugin_root
        .canonicalize()
        .map_err(|e| AuthError::Platform(format!("cannot canonicalize plugin root: {}", e)))?;

    let relative = canonical
        .strip_prefix(&plugin_root_canonical)
        .map_err(|_| AuthError::NotInManifest {
            path: canonical.display().to_string(),
        })?;

    let relative_str = relative.to_string_lossy();
    manifest.verify_caller(&plugin_root_canonical, &relative_str)
}
