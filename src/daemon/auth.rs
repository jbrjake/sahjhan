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

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
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
        let expected_hash = self
            .callers
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
