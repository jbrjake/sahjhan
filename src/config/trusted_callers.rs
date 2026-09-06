// src/config/trusted_callers.rs
//
// Deserialization struct for trusted-callers.toml — the daemon's manifest of
// which scripts may speak the socket protocol, keyed by config-dir-relative
// path and pinned by SHA-256.
//
// It lives with the rest of the sealed config rather than under `daemon`
// because two readers need it and one parse is the point: the daemon
// authenticates a peer against it (see `daemon::auth`), and lint L8 asks the
// static question — can any writer produce the `hook:<path>` provenance this
// gate filters on, or does it name a script the manifest never lists.
//
// ## Index
// - TrustedCallersManifest       — path -> "sha256:<hex>"
// - TrustedCallersManifest::load — read and parse the file

use serde::Deserialize;
use std::collections::HashMap;
use std::path::Path;

/// The `[callers]` table of trusted-callers.toml.
///
/// Keys are paths relative to the config directory; values are `sha256:<hex>`
/// of the script's contents. An *absent* file and an empty table are different
/// things — see `DaemonServer::new` — so this struct only ever describes a file
/// that exists.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct TrustedCallersManifest {
    pub callers: HashMap<String, String>,
}

impl TrustedCallersManifest {
    /// Read and parse the manifest at `path`.
    pub fn load(path: &Path) -> Result<Self, String> {
        let content = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {}", path.display(), e))?;
        toml::from_str(&content).map_err(|e| format!("parse error in {}: {}", path.display(), e))
    }
}
