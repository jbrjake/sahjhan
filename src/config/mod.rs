// src/config/mod.rs
pub mod events;
pub mod protocol;
pub mod renders;
pub mod states;
pub mod transitions;

pub use events::{EventConfig, EventFieldConfig};
pub use protocol::{PathsConfig, ProtocolMeta, SetConfig};
pub use renders::RenderConfig;
pub use states::{StateConfig, StateParam};
pub use transitions::{GateConfig, TransitionConfig};

use std::collections::HashMap;
use std::path::Path;

/// The unified configuration loaded from a protocol directory.
#[derive(Debug, Clone)]
pub struct ProtocolConfig {
    pub protocol: ProtocolMeta,
    pub paths: PathsConfig,
    pub sets: HashMap<String, SetConfig>,
    pub aliases: HashMap<String, String>,
    pub states: HashMap<String, StateConfig>,
    pub transitions: Vec<TransitionConfig>,
    pub events: HashMap<String, EventConfig>,
    pub renders: Vec<RenderConfig>,
}

impl ProtocolConfig {
    /// Load all TOML files from `dir` and assemble a `ProtocolConfig`.
    ///
    /// `events.toml` and `renders.toml` are optional; missing files
    /// result in empty collections rather than an error.
    pub fn load(dir: &Path) -> Result<Self, String> {
        // --- protocol.toml (required) ---
        let proto_path = dir.join("protocol.toml");
        let proto_src = std::fs::read_to_string(&proto_path)
            .map_err(|e| format!("cannot read {}: {}", proto_path.display(), e))?;
        let proto_file: protocol::ProtocolFile = toml::from_str(&proto_src)
            .map_err(|e| format!("parse error in {}: {}", proto_path.display(), e))?;

        // --- states.toml (required) ---
        let states_path = dir.join("states.toml");
        let states_src = std::fs::read_to_string(&states_path)
            .map_err(|e| format!("cannot read {}: {}", states_path.display(), e))?;
        let states_file: states::StatesFile = toml::from_str(&states_src)
            .map_err(|e| format!("parse error in {}: {}", states_path.display(), e))?;

        // --- transitions.toml (required) ---
        let transitions_path = dir.join("transitions.toml");
        let transitions_src = std::fs::read_to_string(&transitions_path)
            .map_err(|e| format!("cannot read {}: {}", transitions_path.display(), e))?;
        let transitions_file: transitions::TransitionsFile = toml::from_str(&transitions_src)
            .map_err(|e| format!("parse error in {}: {}", transitions_path.display(), e))?;

        // --- events.toml (optional) ---
        let events_map = {
            let events_path = dir.join("events.toml");
            match std::fs::read_to_string(&events_path) {
                Ok(src) => {
                    let ef: events::EventsFile = toml::from_str(&src)
                        .map_err(|e| format!("parse error in {}: {}", events_path.display(), e))?;
                    ef.events
                }
                Err(_) => HashMap::new(),
            }
        };

        // --- renders.toml (optional) ---
        let renders_vec = {
            let renders_path = dir.join("renders.toml");
            match std::fs::read_to_string(&renders_path) {
                Ok(src) => {
                    let rf: renders::RendersFile = toml::from_str(&src)
                        .map_err(|e| format!("parse error in {}: {}", renders_path.display(), e))?;
                    rf.renders
                }
                Err(_) => vec![],
            }
        };

        Ok(ProtocolConfig {
            protocol: proto_file.protocol,
            paths: proto_file.paths,
            sets: proto_file.sets,
            aliases: proto_file.aliases,
            states: states_file.states,
            transitions: transitions_file.transitions,
            events: events_map,
            renders: renders_vec,
        })
    }

    /// Return the name of the state that has `initial = true`, if any.
    pub fn initial_state(&self) -> Option<&str> {
        self.states
            .iter()
            .find(|(_, s)| s.initial.unwrap_or(false))
            .map(|(name, _)| name.as_str())
    }

    /// Validate the loaded config. Returns a list of human-readable error strings.
    ///
    /// Checks:
    /// - Exactly one state is marked `initial = true`.
    /// - All transition `from`/`to` fields reference existing state names.
    /// - All `set_covered` gates reference existing set names.
    /// - All sets referenced in state params exist.
    /// - Event field types are one of "string", "number", "boolean".
    pub fn validate(&self) -> Vec<String> {
        let mut errors: Vec<String> = Vec::new();

        // 1. Exactly one initial state.
        let initial_count = self
            .states
            .values()
            .filter(|s| s.initial.unwrap_or(false))
            .count();
        if initial_count == 0 {
            errors.push("no state has initial = true".to_string());
        } else if initial_count > 1 {
            errors.push(format!(
                "multiple states have initial = true ({})",
                initial_count
            ));
        }

        // 2. Transitions reference existing states.
        for t in &self.transitions {
            if !self.states.contains_key(&t.from) {
                errors.push(format!(
                    "transition '{}' has unknown from state '{}'",
                    t.command, t.from
                ));
            }
            if !self.states.contains_key(&t.to) {
                errors.push(format!(
                    "transition '{}' has unknown to state '{}'",
                    t.command, t.to
                ));
            }

            // 3. set_covered gates reference existing sets.
            for gate in &t.gates {
                if gate.gate_type == "set_covered" {
                    if let Some(toml::Value::String(set_name)) = gate.params.get("set") {
                        if !self.sets.contains_key(set_name) {
                            errors.push(format!(
                                "gate in transition '{}' references unknown set '{}'",
                                t.command, set_name
                            ));
                        }
                    }
                }
            }
        }

        // 4. Sets referenced in state params exist.
        for (state_name, state) in &self.states {
            if let Some(params) = &state.params {
                for p in params {
                    if !self.sets.contains_key(&p.set) {
                        errors.push(format!(
                            "state '{}' param '{}' references unknown set '{}'",
                            state_name, p.name, p.set
                        ));
                    }
                }
            }
        }

        // 5. Event field types are valid.
        let valid_types = ["string", "number", "boolean"];
        for (event_name, event) in &self.events {
            for field in &event.fields {
                if !valid_types.contains(&field.field_type.as_str()) {
                    errors.push(format!(
                        "event '{}' field '{}' has unknown type '{}'",
                        event_name, field.name, field.field_type
                    ));
                }
            }
        }

        errors
    }
}
