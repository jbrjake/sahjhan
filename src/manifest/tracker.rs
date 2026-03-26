use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// A single tracked file entry in the manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ManifestEntry {
    pub sha256: String,
    pub last_operation: String,
    pub last_updated: String,
    pub ledger_seq: u64,
}

/// The file integrity manifest that tracks SHA-256 hashes of managed files.
///
/// Stored at `.sahjhan/manifest.json`, the manifest detects unauthorized
/// modifications to files under managed paths by comparing recorded hashes
/// against current file contents on disk.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub managed_paths: Vec<String>,
    pub entries: HashMap<String, ManifestEntry>,
    pub manifest_hash: String,
}

/// Instruction returned by `restore_instruction` indicating how to restore
/// a tracked file to its last known good state.
#[derive(Debug, Clone, PartialEq)]
pub enum RestoreAction {
    /// File was produced by the render engine; re-render from ledger state.
    ReRender {
        path: String,
        ledger_seq: u64,
    },
    /// File was agent-authored; restore via `git checkout`.
    GitCheckout {
        path: String,
    },
    /// Path is not tracked in the manifest.
    NotTracked {
        path: String,
    },
}

impl Manifest {
    /// Create a new manifest for the given managed paths.
    ///
    /// Validates that `data_dir` is under one of the `managed_paths` (E12).
    /// Returns an error if the constraint is violated.
    pub fn init(data_dir: &str, managed_paths: Vec<String>) -> Result<Self, String> {
        // E12: data_dir must be under a managed path
        let data_dir_normalized = normalize_path(data_dir);
        let is_under_managed = managed_paths.iter().any(|mp| {
            let mp_normalized = normalize_path(mp);
            data_dir_normalized.starts_with(&mp_normalized)
                || data_dir_normalized == mp_normalized
        });

        if !is_under_managed {
            return Err(format!(
                "E12: data_dir '{}' is not under any managed path {:?}",
                data_dir, managed_paths
            ));
        }

        let mut manifest = Manifest {
            version: 1,
            managed_paths,
            entries: HashMap::new(),
            manifest_hash: String::new(),
        };
        manifest.manifest_hash = manifest.compute_manifest_hash();
        Ok(manifest)
    }

    /// Load a manifest from a JSON file on disk.
    pub fn load(path: &Path) -> Result<Self, String> {
        let content = fs::read_to_string(path)
            .map_err(|e| format!("cannot read manifest at {}: {}", path.display(), e))?;
        let manifest: Manifest = serde_json::from_str(&content)
            .map_err(|e| format!("invalid manifest JSON at {}: {}", path.display(), e))?;
        Ok(manifest)
    }

    /// Save the manifest to a JSON file, recomputing `manifest_hash` first.
    pub fn save(&mut self, path: &Path) -> Result<(), String> {
        self.manifest_hash = self.compute_manifest_hash();

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("cannot create directory {}: {}", parent.display(), e))?;
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("cannot serialize manifest: {}", e))?;
        fs::write(path, json)
            .map_err(|e| format!("cannot write manifest to {}: {}", path.display(), e))?;
        Ok(())
    }

    /// Track a file by computing its SHA-256 hash and recording metadata.
    ///
    /// `file_path` is the path relative to the project root (as stored in entries).
    /// `abs_path` is the absolute path to the file on disk (used to read contents).
    pub fn track(
        &mut self,
        file_path: &str,
        abs_path: &Path,
        operation: &str,
        ledger_seq: u64,
    ) -> Result<(), String> {
        let hash = compute_file_sha256(abs_path)?;
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);

        let entry = ManifestEntry {
            sha256: hash,
            last_operation: operation.to_string(),
            last_updated: now,
            ledger_seq,
        };

        self.entries.insert(file_path.to_string(), entry);
        self.manifest_hash = self.compute_manifest_hash();
        Ok(())
    }

    /// Compute the manifest hash: SHA-256 of the deterministically serialized entries.
    ///
    /// Uses sorted keys (via BTreeMap) to ensure deterministic output.
    pub fn compute_manifest_hash(&self) -> String {
        // Convert to BTreeMap for deterministic key ordering
        let sorted: BTreeMap<&String, &ManifestEntry> = self.entries.iter().collect();
        let serialized = serde_json::to_string(&sorted).unwrap_or_default();

        let mut hasher = Sha256::new();
        hasher.update(serialized.as_bytes());
        let result = hasher.finalize();
        hex_encode(&result)
    }

    /// Return a restore instruction for the given path.
    ///
    /// If the file's `last_operation` contains "render", it should be re-rendered.
    /// Otherwise, it should be restored via `git checkout`.
    pub fn restore_instruction(&self, path: &str) -> RestoreAction {
        match self.entries.get(path) {
            Some(entry) => {
                if entry.last_operation.contains("render") {
                    RestoreAction::ReRender {
                        path: path.to_string(),
                        ledger_seq: entry.ledger_seq,
                    }
                } else {
                    RestoreAction::GitCheckout {
                        path: path.to_string(),
                    }
                }
            }
            None => RestoreAction::NotTracked {
                path: path.to_string(),
            },
        }
    }
}

/// Compute SHA-256 of a file on disk, returning the hex-encoded hash.
pub fn compute_file_sha256(path: &Path) -> Result<String, String> {
    let content = fs::read(path)
        .map_err(|e| format!("cannot read file {}: {}", path.display(), e))?;
    let mut hasher = Sha256::new();
    hasher.update(&content);
    let result = hasher.finalize();
    Ok(hex_encode(&result))
}

/// Hex-encode a byte slice.
fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Normalize a path string by stripping trailing slashes for consistent comparison.
fn normalize_path(p: &str) -> String {
    p.trim_end_matches('/').to_string()
}
