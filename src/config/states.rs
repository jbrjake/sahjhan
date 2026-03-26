use serde::Deserialize;
use std::collections::HashMap;

/// Wrapper for the full states.toml file.
#[derive(Debug, Deserialize)]
pub struct StatesFile {
    pub states: HashMap<String, StateConfig>,
}

/// A single state definition.
#[derive(Debug, Deserialize, Clone)]
pub struct StateConfig {
    pub label: String,
    pub initial: Option<bool>,
    pub terminal: Option<bool>,
    pub params: Option<Vec<StateParam>>,
    pub metadata: Option<HashMap<String, String>>,
}

/// A parameter bound to a set (used for state context).
#[derive(Debug, Deserialize, Clone)]
pub struct StateParam {
    pub name: String,
    pub set: String,
}
