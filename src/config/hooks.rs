// src/config/hooks.rs
//
// Deserialization structs for hooks.toml.
//
// ## Index
// - HooksFile               — top-level wrapper
// - HookConfig              — single hook rule (gate, check, or auto_record)
// - HookEvent               — PreToolUse | PostToolUse | Stop
// - HookFilter              — path glob filters for tool arguments
// - HookCheck               — threshold/pattern check config
// - AutoRecordConfig        — auto-record event config
// - MonitorConfig           — monitor rule
// - MonitorTrigger          — monitor trigger condition

use serde::Deserialize;
use std::collections::HashMap;

use super::transitions::GateConfig;

/// Top-level wrapper for hooks.toml.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct HooksFile {
    #[serde(default)]
    pub hooks: Vec<HookConfig>,
    #[serde(default)]
    pub monitors: Vec<MonitorConfig>,
}

/// The event that triggers a hook.
#[derive(Debug, Deserialize, Clone, PartialEq)]
pub enum HookEvent {
    PreToolUse,
    PostToolUse,
    Stop,
}

/// A single hook rule.
#[derive(Debug, Deserialize, Clone)]
pub struct HookConfig {
    pub event: HookEvent,
    #[serde(default)]
    pub tools: Option<Vec<String>>,
    #[serde(default)]
    pub states: Option<Vec<String>>,
    #[serde(default)]
    pub states_not: Option<Vec<String>>,
    #[serde(default)]
    pub action: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub gate: Option<GateConfig>,
    #[serde(default)]
    pub check: Option<HookCheck>,
    #[serde(default)]
    pub auto_record: Option<AutoRecordConfig>,
    #[serde(default)]
    pub filter: Option<HookFilter>,
}

/// Path glob filters for tool arguments.
#[derive(Debug, Deserialize, Clone)]
pub struct HookFilter {
    #[serde(default)]
    pub path_matches: Option<String>,
    #[serde(default)]
    pub path_not_matches: Option<String>,
}

/// Threshold/pattern check config.
#[derive(Debug, Deserialize, Clone)]
pub struct HookCheck {
    #[serde(rename = "type")]
    pub check_type: String,
    #[serde(default)]
    pub sql: Option<String>,
    #[serde(default)]
    pub compare: Option<String>,
    #[serde(default)]
    pub threshold: Option<i64>,
    #[serde(default)]
    pub patterns: Option<Vec<String>>,
}

/// Auto-record event config.
#[derive(Debug, Deserialize, Clone)]
pub struct AutoRecordConfig {
    pub event_type: String,
    #[serde(default)]
    pub fields: HashMap<String, String>,
}

/// A monitor rule.
#[derive(Debug, Deserialize, Clone)]
pub struct MonitorConfig {
    pub name: String,
    #[serde(default)]
    pub states: Option<Vec<String>>,
    pub action: String,
    pub message: String,
    pub trigger: MonitorTrigger,
}

/// Monitor trigger condition.
#[derive(Debug, Deserialize, Clone)]
pub struct MonitorTrigger {
    #[serde(rename = "type")]
    pub trigger_type: String,
    pub threshold: u64,
}
