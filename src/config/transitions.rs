// src/config/transitions.rs
//
// Deserialization structs for transitions.toml.
//
// ## Index
// - TransitionsFile         — top-level wrapper
// - TransitionConfig        — from, to, command, args (positional params), gates
// - GateConfig              — gate_type + flattened params

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
    /// Named positional arguments for template variable resolution.
    ///
    /// When a transition declares `args = ["item_id"]`, the first positional
    /// CLI argument (one without `=`) is mapped to `item_id` in state_params
    /// before gate evaluation.
    #[serde(default)]
    pub args: Vec<String>,
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
