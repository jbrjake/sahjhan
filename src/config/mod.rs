// src/config/mod.rs
//
// Unified protocol configuration and validation.
//
// ## Index
// - ProtocolConfig          — unified config loaded from protocol directory
// - [validate]              ProtocolConfig::validate()       — basic structural validation
// - [validate-deep]         ProtocolConfig::validate_deep()  — file/alias/gate/render checks
// - initial_state()         — find the state with initial = true

pub mod events;
pub mod protocol;
pub mod renders;
pub mod states;
pub mod transitions;

pub use events::{EventConfig, EventFieldConfig};
pub use protocol::{CheckpointConfig, LedgerTemplateConfig, PathsConfig, ProtocolMeta, SetConfig};
pub use renders::RenderConfig;
pub use states::{StateConfig, StateParam};
pub use transitions::{GateConfig, TransitionConfig};

use std::collections::{HashMap, HashSet};
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
    pub checkpoints: CheckpointConfig,
    pub ledgers: HashMap<String, LedgerTemplateConfig>,
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
            checkpoints: proto_file.checkpoints,
            ledgers: proto_file.ledgers,
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
    // [validate]
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

        // 4b. State param source values are valid.
        let valid_sources = ["values", "current", "last_completed"];
        for (state_name, state) in &self.states {
            if let Some(params) = &state.params {
                for p in params {
                    if let Some(ref source) = p.source {
                        if !valid_sources.contains(&source.as_str()) {
                            errors.push(format!(
                                "state '{}' param '{}' has invalid source '{}' (valid: {})",
                                state_name,
                                p.name,
                                source,
                                valid_sources.join(", ")
                            ));
                        }
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

    /// Deep validation that includes file-system and cross-reference checks.
    ///
    /// This extends the basic `validate()` with:
    /// - Gate type validation (known types + required params)
    /// - Template file existence (renders.toml paths relative to config_dir)
    /// - Alias target validation (alias values resolve to valid commands)
    /// - Render event type validation (on_event triggers reference defined events)
    /// - Terminal state outgoing transition warnings
    /// - Unreachable state detection warnings
    ///
    /// Returns `(errors, warnings)` — errors are hard failures, warnings are advisory.
    // [validate-deep]
    pub fn validate_deep(&self, config_dir: &Path) -> (Vec<String>, Vec<String>) {
        // Start with the basic checks.
        let mut errors = self.validate();
        let mut warnings: Vec<String> = Vec::new();

        // Known gate types and their required parameters.
        let known_gates: HashMap<&str, Vec<&str>> = HashMap::from([
            ("file_exists", vec!["path"]),
            ("files_exist", vec!["paths"]),
            ("command_succeeds", vec!["cmd"]),
            ("command_output", vec!["cmd", "expect"]),
            ("ledger_has_event", vec!["event"]),
            ("ledger_has_event_since", vec!["event"]),
            ("set_covered", vec!["set"]),
            ("min_elapsed", vec!["event", "seconds"]),
            ("no_violations", vec![]),
            ("field_not_empty", vec!["field"]),
            ("snapshot_compare", vec!["cmd", "extract", "reference"]),
            ("query", vec!["sql"]),
        ]);

        // 6. Gate type validation.
        for t in &self.transitions {
            for gate in &t.gates {
                match known_gates.get(gate.gate_type.as_str()) {
                    None => {
                        errors.push(format!(
                            "transitions.toml: transition '{}' has unknown gate type '{}'",
                            t.command, gate.gate_type
                        ));
                    }
                    Some(required_params) => {
                        for &param in required_params {
                            if !gate.params.contains_key(param) {
                                errors.push(format!(
                                    "transitions.toml: gate '{}' in transition '{}' missing required parameter '{}'",
                                    gate.gate_type, t.command, param
                                ));
                            }
                        }
                    }
                }
            }
        }

        // 7. Template file existence.
        for render in &self.renders {
            let template_path = config_dir.join(&render.template);
            if !template_path.exists() {
                errors.push(format!(
                    "renders.toml: template '{}' does not exist (looked at {})",
                    render.template,
                    template_path.display()
                ));
            }
        }

        // 8. Alias target validation.
        // Build the set of valid transition commands and event types.
        let transition_commands: HashSet<&str> = self
            .transitions
            .iter()
            .map(|t| t.command.as_str())
            .collect();
        let event_types: HashSet<&str> = self.events.keys().map(|k| k.as_str()).collect();

        for (alias_name, alias_target) in &self.aliases {
            let parts: Vec<&str> = alias_target.splitn(2, ' ').collect();
            if parts.len() < 2 {
                errors.push(format!(
                    "protocol.toml: alias '{}' has malformed target '{}' (expected 'command arg')",
                    alias_name, alias_target
                ));
                continue;
            }
            match parts[0] {
                "transition" => {
                    if !transition_commands.contains(parts[1]) {
                        errors.push(format!(
                            "protocol.toml: alias '{}' targets transition '{}' which is not defined",
                            alias_name, parts[1]
                        ));
                    }
                }
                "event" => {
                    if !event_types.contains(parts[1]) {
                        errors.push(format!(
                            "protocol.toml: alias '{}' targets event type '{}' which is not defined",
                            alias_name, parts[1]
                        ));
                    }
                }
                // Other command targets (set, log, status, etc.) are built-in — skip.
                _ => {}
            }
        }

        // 9. Render event type validation.
        for render in &self.renders {
            if render.trigger == "on_event" {
                if let Some(ref types) = render.event_types {
                    for et in types {
                        if !event_types.contains(et.as_str()) {
                            errors.push(format!(
                                "renders.toml: render for '{}' references undefined event type '{}'",
                                render.target, et
                            ));
                        }
                    }
                }
            }
        }

        // 10. Terminal state with outgoing transitions (warning).
        let terminal_states: HashSet<&str> = self
            .states
            .iter()
            .filter(|(_, s)| s.terminal.unwrap_or(false))
            .map(|(name, _)| name.as_str())
            .collect();

        for t in &self.transitions {
            if terminal_states.contains(t.from.as_str()) {
                warnings.push(format!(
                    "transitions.toml: terminal state '{}' has outgoing transition '{}' — this transition can never fire",
                    t.from, t.command
                ));
            }
        }

        // 11. Unreachable state detection (warning).
        // A state is reachable if it is initial, or if it appears as a `to` in some transition.
        let mut reachable: HashSet<&str> = HashSet::new();
        for (name, state) in &self.states {
            if state.initial.unwrap_or(false) {
                reachable.insert(name.as_str());
            }
        }
        for t in &self.transitions {
            reachable.insert(t.to.as_str());
        }
        for name in self.states.keys() {
            if !reachable.contains(name.as_str()) {
                warnings.push(format!(
                    "states.toml: state '{}' is unreachable (no incoming transitions and not initial)",
                    name
                ));
            }
        }

        (errors, warnings)
    }
}
