// src/config/events.rs
//
// Deserialization structs for events.toml.
//
// ## Index
// - EventsFile              — top-level wrapper
// - EventConfig             — single event type definition
// - EventFieldConfig        — field name, type, pattern, allowed values

use serde::Deserialize;
use std::collections::HashMap;

/// Wrapper for the full events.toml file.
#[derive(Debug, Deserialize)]
pub struct EventsFile {
    pub events: HashMap<String, EventConfig>,
}

/// A single event definition.
#[derive(Debug, Deserialize, Clone)]
pub struct EventConfig {
    pub description: String,
    pub fields: Vec<EventFieldConfig>,
}

/// One field within an event.
#[derive(Debug, Deserialize, Clone)]
pub struct EventFieldConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub pattern: Option<String>,
    pub values: Option<Vec<String>>,
}
