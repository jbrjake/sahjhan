// src/config/protocol.rs
//
// Deserialization structs for protocol.toml.
//
// ## Index
// - ProtocolFile            — top-level wrapper (protocol, paths, sets, aliases, checkpoints, ledgers, guards)
// - ProtocolMeta            — name, version, description
// - PathsConfig             — managed, data_dir, render_dir
// - SetConfig               — description + ordered values
// - CheckpointConfig        — checkpoint interval
// - LedgerTemplateConfig     — ledger declaration (path or path_template)
// - GuardsConfig            — read_blocked + write_gated paths
// - WriteGatedConfig        — path whose writability is gated by protocol state

use serde::Deserialize;
use std::collections::HashMap;

/// Represents the full contents of protocol.toml as deserialized from disk.
#[derive(Debug, Deserialize)]
pub struct ProtocolFile {
    pub protocol: ProtocolMeta,
    pub paths: PathsConfig,
    #[serde(default)]
    pub sets: HashMap<String, SetConfig>,
    #[serde(default)]
    pub aliases: HashMap<String, String>,
    #[serde(default)]
    pub checkpoints: CheckpointConfig,
    #[serde(default)]
    pub ledgers: HashMap<String, LedgerTemplateConfig>,
    pub guards: Option<GuardsConfig>,
}

/// Configuration for the `[guards]` section of protocol.toml.
///
/// Lists paths that enforcement hooks should block agents from reading,
/// and paths whose writability is gated by protocol state.
///
/// ```toml
/// [guards]
/// read_blocked = [".sahjhan/session.key", "enforcement/quiz-bank.json"]
///
/// [[guards.write_gated]]
/// path = "src/main.rs"
/// writable_in = ["coding", "review"]
/// message = "Source files are only writable during coding and review states"
/// ```
#[derive(Debug, Deserialize, Clone, Default)]
pub struct GuardsConfig {
    #[serde(default)]
    pub read_blocked: Vec<String>,
    #[serde(default)]
    pub write_gated: Vec<WriteGatedConfig>,
}

/// A path whose writability is gated by protocol state.
#[derive(Debug, Deserialize, Clone)]
pub struct WriteGatedConfig {
    pub path: String,
    pub writable_in: Vec<String>,
    pub message: String,
}

/// Configuration for the `[checkpoints]` section of protocol.toml.
///
/// ```toml
/// [checkpoints]
/// interval = 100  # 0 = disabled
/// ```
#[derive(Debug, Deserialize, Default, Clone)]
pub struct CheckpointConfig {
    /// How often (in events) to auto-checkpoint. `0` means disabled.
    #[serde(default)]
    pub interval: u64,
}

/// The `[protocol]` section.
#[derive(Debug, Deserialize, Clone)]
pub struct ProtocolMeta {
    pub name: String,
    pub version: String,
    pub description: String,
}

/// The `[paths]` section.
#[derive(Debug, Deserialize, Clone)]
pub struct PathsConfig {
    pub managed: Vec<String>,
    pub data_dir: String,
    pub render_dir: String,
}

/// A named set of values (e.g. `[sets.check]`).
#[derive(Debug, Deserialize, Clone)]
pub struct SetConfig {
    pub description: String,
    pub values: Vec<String>,
}

/// A ledger declaration in protocol.toml.
///
/// Two forms:
/// - **Template** (`path_template`): pattern with `{template.instance_id}` / `{template.name}`
/// - **Fixed** (`path`): single known path, no instantiation
///
/// These are mutually exclusive.
#[derive(Debug, Deserialize, Clone)]
pub struct LedgerTemplateConfig {
    pub description: String,
    /// Fixed path (for singleton ledgers).
    pub path: Option<String>,
    /// Path template with `{template.instance_id}` and `{template.name}` variables.
    pub path_template: Option<String>,
}
