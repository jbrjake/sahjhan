// src/config/mod.rs
//
// Unified protocol configuration and validation.
//
// ## Index
// - ProtocolConfig          — unified config loaded from protocol directory (includes guards, hooks, monitors)
// - [validate]              ProtocolConfig::validate()       — basic structural validation
// - [validate-deep]         ProtocolConfig::validate_deep()  — file/alias/gate/render/ledger/branching checks
// - [validate-gate]         ProtocolConfig::validate_gate()  — recursive gate validator (composite + leaf)
// - initial_state()         — find the state with initial = true
// - [compute-config-seals]  compute_config_seals()           — SHA-256 hashes of all six TOML config files

pub mod events;
pub mod hooks;
pub mod protocol;
pub mod renders;
pub mod states;
pub mod transitions;

pub use events::{EventConfig, EventFieldConfig};
pub use hooks::{
    AutoRecordConfig, HookCheck, HookConfig, HookEvent, HookFilter, HooksFile, MonitorConfig,
    MonitorTrigger,
};
pub use protocol::{
    CheckpointConfig, GuardsConfig, LedgerTemplateConfig, PathsConfig, ProtocolMeta, SetConfig,
    WriteGatedConfig,
};
pub use renders::RenderConfig;
pub use states::{StateConfig, StateParam};
pub use transitions::{GateConfig, TransitionConfig};

use std::collections::{BTreeMap, HashMap, HashSet};
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
    pub guards: Option<GuardsConfig>,
    pub hooks: Vec<hooks::HookConfig>,
    pub monitors: Vec<hooks::MonitorConfig>,
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

        // --- hooks.toml (optional) ---
        let (hooks_vec, monitors_vec) = {
            let hooks_path = dir.join("hooks.toml");
            match std::fs::read_to_string(&hooks_path) {
                Ok(src) => {
                    let hf: hooks::HooksFile = toml::from_str(&src)
                        .map_err(|e| format!("parse error in {}: {}", hooks_path.display(), e))?;
                    (hf.hooks, hf.monitors)
                }
                Err(_) => (vec![], vec![]),
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
            guards: proto_file.guards,
            hooks: hooks_vec,
            monitors: monitors_vec,
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
    /// - Branching transitions without a gateless fallback (warning)
    /// - Ledger template validation (exactly one of path/path_template; path_template must contain {template.instance_id})
    /// - Hook validation (action/message required, state refs, gate/check/auto_record mutual exclusion)
    /// - Monitor validation (unique names, action = "warn", state refs, trigger types)
    /// - Write-gated guard validation (writable_in states must exist)
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
            ("ledger_has_event_since", vec!["event", "since"]),
            ("ledger_lacks_event", vec!["event"]),
            ("set_covered", vec!["set"]),
            ("min_elapsed", vec!["event", "seconds"]),
            ("no_violations", vec![]),
            ("field_not_empty", vec!["field"]),
            ("snapshot_compare", vec!["cmd", "extract", "reference"]),
            ("query", vec!["sql"]),
        ]);

        // 6. Gate type validation (recursive for composite gates).
        for t in &self.transitions {
            for gate in &t.gates {
                Self::validate_gate(gate, &t.command, &known_gates, &mut errors);
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

        // 11b. Branching transitions without fallback (warning).
        {
            let mut groups: HashMap<(&str, &str), Vec<&TransitionConfig>> = HashMap::new();
            for t in &self.transitions {
                groups
                    .entry((t.from.as_str(), t.command.as_str()))
                    .or_default()
                    .push(t);
            }
            for ((from, command), members) in &groups {
                if members.len() > 1 && !members.iter().any(|t| t.gates.is_empty()) {
                    warnings.push(format!(
                        "transitions.toml: command '{}' from '{}' has {} candidates but no fallback (all have gates \u{2014} agent may get stuck)",
                        command, from, members.len()
                    ));
                }
            }
        }

        // 12. Ledger template validation.
        for (name, ledger_tmpl) in &self.ledgers {
            match (&ledger_tmpl.path, &ledger_tmpl.path_template) {
                (Some(_), Some(_)) => {
                    errors.push(format!(
                        "protocol.toml: ledger '{}' has both 'path' and 'path_template' — must have exactly one",
                        name
                    ));
                }
                (None, None) => {
                    errors.push(format!(
                        "protocol.toml: ledger '{}' must have either 'path' or 'path_template'",
                        name
                    ));
                }
                (None, Some(tmpl)) => {
                    if !tmpl.contains("{template.instance_id}") {
                        errors.push(format!(
                            "protocol.toml: ledger '{}' path_template must contain '{{template.instance_id}}'",
                            name
                        ));
                    }
                }
                (Some(_), None) => {
                    // Fixed path — valid as-is.
                }
            }
        }

        // 13. Render ledger/ledger_template validation.
        for render in &self.renders {
            if render.ledger.is_some() && render.ledger_template.is_some() {
                errors.push(format!(
                    "renders.toml: render for '{}' has both 'ledger' and 'ledger_template' — use one or the other",
                    render.target
                ));
            }
            if let Some(ref tmpl_name) = render.ledger_template {
                if !self.ledgers.contains_key(tmpl_name) {
                    errors.push(format!(
                        "renders.toml: render for '{}' references ledger_template '{}' which is not declared in protocol.toml [ledgers]",
                        render.target, tmpl_name
                    ));
                }
            }
        }

        // 14. Hook validation.
        let known_check_types: HashSet<&str> = [
            "query",
            "output_contains_any",
            "event_count_since_last_transition",
        ]
        .iter()
        .copied()
        .collect();
        let state_names: HashSet<&str> = self.states.keys().map(|s| s.as_str()).collect();

        for (idx, hook) in self.hooks.iter().enumerate() {
            let label = format!("hooks.toml: hook[{}]", idx);

            // Exactly one of gate, check, or auto_record must be present.
            let mechanism_count = [
                hook.gate.is_some(),
                hook.check.is_some(),
                hook.auto_record.is_some(),
            ]
            .iter()
            .filter(|&&b| b)
            .count();
            if mechanism_count != 1 {
                errors.push(format!(
                    "{}: exactly one of gate, check, or auto_record must be present (found {})",
                    label, mechanism_count
                ));
            }

            // Non-auto_record hooks require action and message.
            if hook.auto_record.is_none() {
                if let Some(ref action) = hook.action {
                    if action != "block" && action != "warn" {
                        errors.push(format!(
                            "{}: action must be 'block' or 'warn', got '{}'",
                            label, action
                        ));
                    }
                } else {
                    errors.push(format!("{}: 'action' is required", label));
                }
                if hook.message.is_none() {
                    errors.push(format!("{}: 'message' is required", label));
                }
            }

            // auto_record hooks must have event = PostToolUse.
            if let Some(ref auto) = hook.auto_record {
                if hook.event != hooks::HookEvent::PostToolUse {
                    errors.push(format!(
                        "{}: auto_record hooks must have event = 'PostToolUse'",
                        label
                    ));
                }
                if !self.events.contains_key(&auto.event_type) {
                    errors.push(format!(
                        "{}: auto_record.event_type '{}' is not defined in events.toml",
                        label, auto.event_type
                    ));
                }
            }

            // states must reference existing states.
            if let Some(ref states) = hook.states {
                for s in states {
                    if !state_names.contains(s.as_str()) {
                        errors.push(format!("{}: references unknown state '{}'", label, s));
                    }
                }
            }

            // states_not must reference existing states.
            if let Some(ref states_not) = hook.states_not {
                for s in states_not {
                    if !state_names.contains(s.as_str()) {
                        errors.push(format!(
                            "{}: states_not references unknown state '{}'",
                            label, s
                        ));
                    }
                }
            }

            // gate validated through recursive validator.
            if let Some(ref gate) = hook.gate {
                Self::validate_gate(gate, &format!("hook[{}]", idx), &known_gates, &mut errors);
            }

            // check.type must be a known check type.
            if let Some(ref check) = hook.check {
                if !known_check_types.contains(check.check_type.as_str()) {
                    errors.push(format!(
                        "{}: unknown check type '{}' (known: {})",
                        label,
                        check.check_type,
                        known_check_types
                            .iter()
                            .copied()
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
            }
        }

        // 15. Monitor validation.
        {
            let known_monitor_trigger_types: HashSet<&str> = ["event_count_since_last_transition"]
                .iter()
                .copied()
                .collect();
            let mut monitor_names: HashSet<String> = HashSet::new();

            for (idx, monitor) in self.monitors.iter().enumerate() {
                let label = format!("hooks.toml: monitor[{}] '{}'", idx, monitor.name);

                // Names must be unique.
                if !monitor_names.insert(monitor.name.clone()) {
                    errors.push(format!("{}: duplicate monitor name", label));
                }

                // action must be "warn".
                if monitor.action != "warn" {
                    errors.push(format!(
                        "{}: action must be 'warn', got '{}'",
                        label, monitor.action
                    ));
                }

                // states must reference existing states.
                if let Some(ref states) = monitor.states {
                    for s in states {
                        if !state_names.contains(s.as_str()) {
                            errors.push(format!("{}: references unknown state '{}'", label, s));
                        }
                    }
                }

                // trigger.type must be known.
                if !known_monitor_trigger_types.contains(monitor.trigger.trigger_type.as_str()) {
                    errors.push(format!(
                        "{}: unknown trigger type '{}' (known: {})",
                        label,
                        monitor.trigger.trigger_type,
                        known_monitor_trigger_types
                            .iter()
                            .copied()
                            .collect::<Vec<_>>()
                            .join(", ")
                    ));
                }
            }
        }

        // 16. Write-gated guard validation.
        if let Some(ref guards) = self.guards {
            for wg in &guards.write_gated {
                for s in &wg.writable_in {
                    if !state_names.contains(s.as_str()) {
                        errors.push(format!(
                            "protocol.toml: write_gated path '{}' references unknown state '{}'",
                            wg.path, s
                        ));
                    }
                }
            }
        }

        (errors, warnings)
    }

    /// Recursively validate a single gate and its children.
    ///
    /// Composite gates (any_of, all_of, not, k_of_n) have structural
    /// requirements checked here; leaf gates are validated against the
    /// `known_gates` map for type and required params.
    // [validate-gate]
    fn validate_gate(
        gate: &GateConfig,
        transition_command: &str,
        known_gates: &HashMap<&str, Vec<&str>>,
        errors: &mut Vec<String>,
    ) {
        match gate.gate_type.as_str() {
            "any_of" | "all_of" => {
                if gate.gates.is_empty() {
                    errors.push(format!(
                        "transitions.toml: gate '{}' in transition '{}' has empty gates list",
                        gate.gate_type, transition_command
                    ));
                }
                for child in &gate.gates {
                    Self::validate_gate(child, transition_command, known_gates, errors);
                }
            }
            "not" => {
                if gate.gates.len() != 1 {
                    errors.push(format!(
                        "transitions.toml: gate 'not' in transition '{}' requires exactly 1 child gate, has {}",
                        transition_command,
                        gate.gates.len()
                    ));
                }
                for child in &gate.gates {
                    Self::validate_gate(child, transition_command, known_gates, errors);
                }
            }
            "k_of_n" => {
                if gate.gates.is_empty() {
                    errors.push(format!(
                        "transitions.toml: gate 'k_of_n' in transition '{}' has empty gates list",
                        transition_command
                    ));
                }
                let k = gate.params.get("k").and_then(|v| v.as_integer());
                match k {
                    None => {
                        errors.push(format!(
                            "transitions.toml: gate 'k_of_n' in transition '{}' missing required parameter 'k'",
                            transition_command
                        ));
                    }
                    Some(k_val) => {
                        if k_val < 1 || k_val as usize > gate.gates.len() {
                            errors.push(format!(
                                "transitions.toml: gate 'k_of_n' in transition '{}' has k={} but {} child gates (k must be 1..=n)",
                                transition_command,
                                k_val,
                                gate.gates.len()
                            ));
                        }
                    }
                }
                for child in &gate.gates {
                    Self::validate_gate(child, transition_command, known_gates, errors);
                }
            }
            _ => {
                // Leaf gate — validate type and required params.
                match known_gates.get(gate.gate_type.as_str()) {
                    None => {
                        errors.push(format!(
                            "transitions.toml: transition '{}' has unknown gate type '{}'",
                            transition_command, gate.gate_type
                        ));
                    }
                    Some(required_params) => {
                        for &param in required_params {
                            if !gate.params.contains_key(param) {
                                errors.push(format!(
                                    "transitions.toml: gate '{}' in transition '{}' missing required parameter '{}'",
                                    gate.gate_type, transition_command, param
                                ));
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Compute SHA-256 hashes of all six TOML config files.
///
/// Missing optional files (events.toml, renders.toml, hooks.toml) hash as empty bytes.
/// Returns a BTreeMap with keys: config_seal_protocol, config_seal_states,
/// config_seal_transitions, config_seal_events, config_seal_renders, config_seal_hooks.
// [compute-config-seals]
pub fn compute_config_seals(dir: &Path) -> BTreeMap<String, String> {
    use sha2::{Digest, Sha256};

    let files = [
        ("config_seal_protocol", "protocol.toml"),
        ("config_seal_states", "states.toml"),
        ("config_seal_transitions", "transitions.toml"),
        ("config_seal_events", "events.toml"),
        ("config_seal_renders", "renders.toml"),
        ("config_seal_hooks", "hooks.toml"),
    ];

    let mut seals = BTreeMap::new();
    for (key, filename) in &files {
        let path = dir.join(filename);
        let bytes = std::fs::read(&path).unwrap_or_default();
        let hash = hex::encode(Sha256::digest(&bytes));
        seals.insert(key.to_string(), hash);
    }
    seals
}
