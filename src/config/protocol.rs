// src/config/protocol.rs
//
// Deserialization structs for protocol.toml.
//
// ## Index
// - ProtocolFile            — top-level wrapper (protocol, paths, sets, aliases, checkpoints)
// - ProtocolMeta            — name, version, description
// - PathsConfig             — managed, data_dir, render_dir
// - SetConfig               — description + ordered values
// - CheckpointConfig        — checkpoint interval

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
