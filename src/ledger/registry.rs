//! Multi-ledger registry — maps human-friendly names to JSONL file paths.
//!
//! The registry is stored at `.sahjhan/ledgers.toml` (or any caller-supplied
//! path) as a TOML array-of-tables.  It is a convenience index, not a critical
//! data path; the ledger files themselves are the source of truth.

use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Whether a ledger tracks mutable state or is append-only event log.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum LedgerMode {
    Stateful,
    EventOnly,
}

/// One row in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerRegistryEntry {
    pub name: String,
    pub path: String,
    pub mode: LedgerMode,
    /// ISO 8601 creation timestamp.
    pub created: String,
}

/// The on-disk TOML shape: `{ ledgers = [...] }`.
#[derive(Debug, Serialize, Deserialize, Default)]
struct RegistryFile {
    #[serde(default)]
    ledgers: Vec<LedgerRegistryEntry>,
}

// ---------------------------------------------------------------------------
// LedgerRegistry
// ---------------------------------------------------------------------------

/// In-memory view of the registry, backed by a TOML file.
pub struct LedgerRegistry {
    file_path: PathBuf,
    entries: Vec<LedgerRegistryEntry>,
}

impl LedgerRegistry {
    // -----------------------------------------------------------------------
    // Construction
    // -----------------------------------------------------------------------

    /// Load the registry from `file_path`, or start with an empty registry if
    /// the file does not yet exist.
    pub fn new(file_path: &Path) -> Result<Self, String> {
        let entries = if file_path.exists() {
            let raw = fs::read_to_string(file_path)
                .map_err(|e| format!("cannot read registry {}: {e}", file_path.display()))?;
            let parsed: RegistryFile = toml::from_str(&raw)
                .map_err(|e| format!("malformed registry TOML: {e}"))?;
            parsed.ledgers
        } else {
            Vec::new()
        };

        Ok(Self {
            file_path: file_path.to_path_buf(),
            entries,
        })
    }

    // -----------------------------------------------------------------------
    // Mutations
    // -----------------------------------------------------------------------

    /// Register a new ledger.  Fails if `name` already exists.
    pub fn create(&mut self, name: &str, path: &str, mode: LedgerMode) -> Result<(), String> {
        if self.entries.iter().any(|e| e.name == name) {
            return Err(format!("ledger '{name}' already exists in the registry"));
        }

        let created = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
        self.entries.push(LedgerRegistryEntry {
            name: name.to_string(),
            path: path.to_string(),
            mode,
            created,
        });

        self.save()
    }

    /// Remove a ledger from the registry by name.  Does NOT delete the file.
    /// Fails if `name` is not found.
    pub fn remove(&mut self, name: &str) -> Result<(), String> {
        let before = self.entries.len();
        self.entries.retain(|e| e.name != name);

        if self.entries.len() == before {
            return Err(format!("ledger '{name}' not found in the registry"));
        }

        self.save()
    }

    // -----------------------------------------------------------------------
    // Queries
    // -----------------------------------------------------------------------

    /// Return the list of registered ledgers in insertion order.
    pub fn list(&self) -> &[LedgerRegistryEntry] {
        &self.entries
    }

    /// Find a ledger by name, or return the first entry when `name` is `None`.
    ///
    /// Returns an error if the registry is empty (when `name` is `None`) or
    /// if the named ledger is not found.
    pub fn resolve(&self, name: Option<&str>) -> Result<&LedgerRegistryEntry, String> {
        match name {
            Some(n) => self
                .entries
                .iter()
                .find(|e| e.name == n)
                .ok_or_else(|| format!("ledger '{n}' not found in the registry")),
            None => self
                .entries
                .first()
                .ok_or_else(|| "registry is empty — no default ledger".to_string()),
        }
    }

    // -----------------------------------------------------------------------
    // Persistence
    // -----------------------------------------------------------------------

    /// Serialize the registry to disk, creating parent directories as needed.
    pub fn save(&self) -> Result<(), String> {
        if let Some(parent) = self.file_path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                format!(
                    "cannot create registry directory {}: {e}",
                    parent.display()
                )
            })?;
        }

        let data = RegistryFile {
            ledgers: self.entries.clone(),
        };
        let toml_str =
            toml::to_string_pretty(&data).map_err(|e| format!("TOML serialization error: {e}"))?;

        fs::write(&self.file_path, toml_str)
            .map_err(|e| format!("cannot write registry {}: {e}", self.file_path.display()))
    }
}
