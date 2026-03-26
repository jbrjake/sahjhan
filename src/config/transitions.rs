use serde::Deserialize;
use std::collections::HashMap;

/// Wrapper for the full transitions.toml file.
#[derive(Debug, Deserialize)]
pub struct TransitionsFile {
    pub transitions: Vec<TransitionConfig>,
}

/// A single transition definition.
#[derive(Debug, Deserialize, Clone)]
pub struct TransitionConfig {
    pub from: String,
    pub to: String,
    pub command: String,
    #[serde(default)]
    pub gates: Vec<GateConfig>,
}

/// A gate condition attached to a transition.
///
/// The `type` field identifies the gate kind; all remaining fields are
/// captured in `params` via `#[serde(flatten)]` so that we can handle
/// different gate shapes without needing an exhaustive enum.
#[derive(Debug, Deserialize, Clone)]
pub struct GateConfig {
    #[serde(rename = "type")]
    pub gate_type: String,
    #[serde(flatten)]
    pub params: HashMap<String, toml::Value>,
}
