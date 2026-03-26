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
