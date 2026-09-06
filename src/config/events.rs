// src/config/events.rs
//
// Deserialization structs for events.toml.
//
// ## Index
// - EventsFile              — top-level wrapper
// - EventConfig             — single event type definition; `restricted` marks HMAC-only events; `attestation` names its evidence strength
// - EventFieldConfig        — field name, type, pattern, allowed values, optional flag, `stamped` provenance
// - ProducerConfig          — a declared producer of an event, with an optional state window
// - ENGINE_EVENTS           — event types the engine itself writes; part of the vocabulary without being declared
// - is_engine_event()       — whether an event type is one of those

use serde::Deserialize;
use std::collections::HashMap;

/// Event types the engine writes on its own behalf. Nothing in a consumer's
/// config produces these, they are never declared in events.toml, and their
/// absence from it is not a defect — so anything asking "is this a real event
/// type" has to admit them alongside the declared ones.
pub const ENGINE_EVENTS: &[&str] = &[
    "genesis",
    "state_transition",
    "gate_attestation",
    "set_member_complete",
    "checkpoint",
    "config_reseal",
];

/// Whether `event` is written by the engine itself.
pub fn is_engine_event(event: &str) -> bool {
    ENGINE_EVENTS.contains(&event)
}

/// Wrapper for the full events.toml file.
#[derive(Debug, Deserialize)]
pub struct EventsFile {
    pub events: HashMap<String, EventConfig>,
}

/// A single event definition.
#[derive(Debug, Deserialize, Clone)]
pub struct EventConfig {
    pub description: String,
    #[serde(default)]
    pub restricted: Option<bool>,
    /// Who can record this event. Optional; absent means the engine only knows
    /// about producers it can infer (transition `emits`, hook `auto_record`).
    /// Consumed by `lint` checks L1 and L2 — see [`ProducerConfig`].
    #[serde(default)]
    pub producers: Vec<ProducerConfig>,
    /// How strong this event's evidence is, as a level from the consumer's
    /// `[attestation] levels` ordering. Opaque to the engine: lint check L7 only
    /// compares it against what a transition or gate requires.
    #[serde(default)]
    pub attestation: Option<String>,
    pub fields: Vec<EventFieldConfig>,
}

/// A declared producer of an event.
///
/// ```toml
/// [[events.context_reset.producers]]
/// id = "hook:session-start"
/// available_in_states = ["awaiting_clear"]
/// ```
///
/// `id` is opaque: the engine never interprets it, it only reports it. The
/// declaration is what makes "can anything actually record this event, and
/// could it have run before the gate that needs it?" decidable at rest.
/// Verifying that the declared producer is the *real* one — that the hook is
/// registered, that its script is hash-pinned — stays with the consumer.
#[derive(Debug, Deserialize, Clone)]
pub struct ProducerConfig {
    pub id: String,
    /// States in which this producer can run. Absent means unconstrained; the
    /// engine then makes no temporal claim about it (L2 stays silent).
    #[serde(default)]
    pub available_in_states: Option<Vec<String>>,
}

/// One field within an event.
#[derive(Debug, Deserialize, Clone)]
pub struct EventFieldConfig {
    pub name: String,
    #[serde(rename = "type")]
    pub field_type: String,
    pub pattern: Option<String>,
    pub values: Option<Vec<String>>,
    #[serde(default)]
    pub optional: bool,
    /// The engine owns this field's value: it records which write path the
    /// event arrived on, and a caller that supplies it is refused (#50).
    ///
    /// ```toml
    /// { name = "recorded_by", type = "string", stamped = true }
    /// ```
    ///
    /// This is per *field* where `restricted` is per event, which is the whole
    /// reason it exists: an event with transition `emits` cannot be restricted
    /// at all, so before #50 it could carry no provenance whatsoever. A stamped
    /// field leaves every existing writer working and gives a SQL gate a column
    /// that says which one wrote the row — see [`crate::provenance`] for the
    /// values, and lint L8 for what it checks about them.
    ///
    /// A stamped field is always present in the recorded event, so `optional`
    /// on one is inert; its value never comes from the caller, so `pattern` and
    /// `values` on one constrain nothing. Config validation rejects all three
    /// rather than accepting a line that does nothing.
    #[serde(default)]
    pub stamped: bool,
}
