// src/daemon/auth.rs
//
// Caller authentication for the daemon. Loads a trusted-callers manifest,
// resolves the calling script from PID metadata, and verifies its hash.
//
// Scope, stated honestly: this is hardening, not the security boundary. A
// same-user process can defeat any peer inspection — fork a connected fd
// into an exec of a genuine hook, rewrite its own /proc/pid/cmdline on
// Linux, or load code into a trusted interpreter. The boundary against the
// agent is the OS sandbox (see daemon/fuse.rs). This check exists to reject
// accidents and crude impersonation, and to keep authority from being
// inherited: only the process *directly* holding the socket is examined.
//
// ## Index
// - TrustedCallersManifest    — manifest struct + loader
// - TrustedCallersManifest::verify_caller — path lookup + SHA-256 verification
// - extract_script_path       — extract script path from interpreter cmdline
// - AuthError                 — authentication error type
// - AuthError::reason_code    — map error to diagnostic reason code (issue #26)
// - authenticate_peer         — direct-peer caller authentication (no ancestor walk)

use crate::daemon::platform;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use subtle::ConstantTimeEq;
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

        // Constant-time comparison to prevent timing side-channels
        let hashes_equal: bool = {
            let a = actual_hash.as_bytes();
            let b = expected_hash.as_bytes();
            a.len() == b.len() && a.ct_eq(b).into()
        };
        if !hashes_equal {
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

/// Authenticate the process directly holding the socket.
///
/// The peer PID's kernel-recorded cmdline must name a script that
/// canonicalizes under `plugin_root` (the `--config-dir`) and appears in
/// the trusted-callers manifest with a matching SHA-256.
///
/// Deliberately absent, both removed with the ancestor walk:
///
/// - **No walk up the process tree.** Manifest authority must not be
///   inheritable — under the walk, *every* descendant of a trusted script
///   (a shell it spawned, a command that shell ran) authenticated as it.
/// - **No exemption for our own binary.** A CLI-mediated connection
///   (`sahjhan sign`) does not authenticate, whoever its ancestors are: an
///   agent that can run the CLI must not reach privileged ops through it.
///   Trusted consumers speak the socket protocol directly from their hooks.
pub fn authenticate_peer(
    stream: &UnixStream,
    manifest: &TrustedCallersManifest,
    plugin_root: &Path,
) -> Result<(), AuthError> {
    let peer_pid = platform::get_peer_pid(stream)
        .map_err(|e| AuthError::Platform(format!("cannot get peer PID: {}", e)))?;

    let cmdline = platform::get_cmdline(peer_pid).map_err(|e| {
        AuthError::Platform(format!("cannot get cmdline for PID {}: {}", peer_pid, e))
    })?;

    let script_path_str = extract_script_path(&cmdline).ok_or(AuthError::NoScriptPath)?;
    let script_path = Path::new(&script_path_str);
    // Relative cmdline paths resolve against the daemon's own cwd, which is
    // generally not the peer's — consumers must invoke hooks by absolute
    // path for auth to succeed.
    let canonical = script_path
        .canonicalize()
        .map_err(|_| AuthError::ScriptNotFound(script_path.to_path_buf()))?;

    let plugin_root_canonical = plugin_root
        .canonicalize()
        .map_err(|e| AuthError::Platform(format!("cannot canonicalize plugin root: {}", e)))?;

    let relative = canonical
        .strip_prefix(&plugin_root_canonical)
        .map_err(|_| AuthError::NotInManifest {
            path: canonical.display().to_string(),
        })?;

    manifest.verify_caller(&plugin_root_canonical, &relative.to_string_lossy())
}
