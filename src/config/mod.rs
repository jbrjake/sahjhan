// src/config/mod.rs
//
// Unified protocol configuration and validation.
//
// ## Index
// - ProtocolConfig          — unified config loaded from protocol directory (includes guards, hooks, monitors)
// - [validate]              ProtocolConfig::validate()       — basic structural validation
// - [validate-deep]         ProtocolConfig::validate_deep()  — file/alias/gate/render/ledger/branching checks
// - [validate-gate]         ProtocolConfig::validate_gate()  — recursive gate validator (composite + leaf)
// - [resolve-gate-since]    ProtocolConfig::resolve_gate_since()   — a gate's `since` param as written → baseline event type
// - [resolve-since-anchor]  ProtocolConfig::resolve_since_anchor() — `since` form → baseline event type, or why not
// - SinceAnchorError        — a non-string value, an unrecognized form, or a prefixed form naming an undeclared event type
// - initial_state()         — find the state with initial = true
// - [compute-config-seals]  compute_config_seals()           — SHA-256 hashes of all eight sealed config files

pub mod events;
pub mod hooks;
pub mod protocol;
pub mod renders;
pub mod states;
pub mod transitions;
pub mod vault_policy;

pub use events::{EventConfig, EventFieldConfig, ProducerConfig};
pub use hooks::{
    AutoRecordConfig, HookCheck, HookConfig, HookEvent, HookFilter, HooksFile, MonitorConfig,
    MonitorTrigger,
};
pub use protocol::{
    AttestationConfig, BatchConfig, BatchStep, BoundaryConfig, BoundaryEdge, CheckpointConfig,
    DaemonConfig, GuardsConfig, LedgerTemplateConfig, LintConfig, NamedQuery, PathsConfig,
    ProtocolMeta, SetConfig, WriteGatedConfig,
};
pub use renders::RenderConfig;
pub use states::{StateConfig, StateParam};
pub use transitions::{GateConfig, IntegrityConfig, TransitionConfig};
pub use vault_policy::{VaultAccess, VaultPolicy};

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// Why a `ledger_has_event_since` gate's `since` value names no baseline.
///
/// Every variant used to resolve to seq 0 or to the default anchor — both of
/// them values a *correct* config also produces, which is what made a typo
/// invisible rather than merely wrong (sahjhan #34).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SinceAnchorError {
    /// Not a string at all: `since = 42` rather than `since = "42"`. TOML is
    /// typed, so this never reached the anchor forms below — it read as an
    /// absent `since` and took the default.
    NotAString(String),
    /// Not one of the forms the engine knows how to read.
    UnrecognizedForm(String),
    /// `last_event_of_type:<type>` naming an event type nothing declares.
    UndeclaredEvent(String),
}

impl std::fmt::Display for SinceAnchorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SinceAnchorError::NotAString(found) => write!(
                f,
                "has a non-string since anchor (found {}; expected the string \
                 \"last_transition\" or \"last_event_of_type:<event type>\")",
                found
            ),
            SinceAnchorError::UnrecognizedForm(value) => write!(
                f,
                "has unrecognized since anchor '{}' (expected \"last_transition\" \
                 or \"last_event_of_type:<event type>\")",
                value
            ),
            SinceAnchorError::UndeclaredEvent(event_type) => write!(
                f,
                "anchors on undeclared event type '{}' (since = \
                 \"last_event_of_type:{}\" — declare [events.{}] in events.toml)",
                event_type, event_type, event_type
            ),
        }
    }
}

impl std::error::Error for SinceAnchorError {}

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
    /// Named SQL predicates (`[queries.<name>]`), referenced by query gates as
    /// `query = "<name>"`. Empty when none are declared.
    pub queries: HashMap<String, protocol::NamedQuery>,
    /// Bulk transitions (`[batches.<name>]`), run by `sahjhan batch <name>`.
    /// Empty when none are declared.
    pub batches: HashMap<String, protocol::BatchConfig>,
    /// Edges that must not be routed around (`[[boundaries]]`), checked by
    /// lint L3. Empty when none are declared.
    pub boundaries: Vec<BoundaryConfig>,
    /// Consumer-declared evidence-strength ordering (`[attestation] levels`),
    /// compared but never interpreted by the engine. Checked by lint L7.
    pub attestation: AttestationConfig,
    /// The `[lint]` section — strictness knobs for static analysis. Defaults
    /// apply when the section is absent.
    pub lint: LintConfig,
    /// The `[daemon]` section — daemon-mode knobs (`require_sandbox` arms the
    /// sandbox fuse). Defaults apply when the section is absent.
    pub daemon: DaemonConfig,
    pub hooks: Vec<hooks::HookConfig>,
    pub monitors: Vec<hooks::MonitorConfig>,
    /// Per-key state-based vault access policies, keyed by vault entry name.
    /// Empty when no `vault.toml` is present (all keys unrestricted).
    pub vault_policies: HashMap<String, vault_policy::VaultPolicy>,
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

        // --- vault.toml (optional) ---
        let vault_policies_map = {
            let vault_path = dir.join("vault.toml");
            match std::fs::read_to_string(&vault_path) {
                Ok(src) => {
                    let vf: vault_policy::VaultPolicyFile = toml::from_str(&src)
                        .map_err(|e| format!("parse error in {}: {}", vault_path.display(), e))?;
                    vf.policies
                        .into_iter()
                        .map(|p| (p.name.clone(), p))
                        .collect()
                }
                Err(_) => HashMap::new(),
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
            queries: proto_file.queries,
            batches: proto_file.batches,
            boundaries: proto_file.boundaries,
            attestation: proto_file.attestation,
            lint: proto_file.lint,
            daemon: proto_file.daemon,
            hooks: hooks_vec,
            monitors: monitors_vec,
            vault_policies: vault_policies_map,
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

            // 3b. Emitted events must be defined and must not be restricted.
            // A transition emit appends directly to the ledger, so allowing it
            // to write a `restricted` event would bypass the HMAC proof that
            // `authed-event` requires for those types.
            for emit in &t.emits {
                match self.events.get(&emit.event) {
                    None => errors.push(format!(
                        "transition '{}' emits unknown event '{}'",
                        t.command, emit.event
                    )),
                    Some(ev) if ev.restricted == Some(true) => errors.push(format!(
                        "transition '{}' emits restricted event '{}' — restricted \
                         events require 'authed-event' with an HMAC proof, not a \
                         transition emit",
                        t.command, emit.event
                    )),
                    _ => {}
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

        // 6. Gate windows resolve — `since` anchors and the filters that scope
        // them. Checked here rather than in `validate_deep` because this is the
        // validation every command runs, including `init` and `reseal` — the
        // two that seal the config. A window the engine cannot read must not be
        // sealable.
        for t in &self.transitions {
            for gate in &t.gates {
                self.check_gate_windows(
                    gate,
                    &format!("transitions.toml: transition '{}'", t.command),
                    &mut errors,
                );
            }
        }
        for (idx, hook) in self.hooks.iter().enumerate() {
            if let Some(ref gate) = hook.gate {
                self.check_gate_windows(gate, &format!("hooks.toml: hook[{}]", idx), &mut errors);
            }
        }

        errors
    }

    /// Recursively check every gate in a tree for a window it cannot express.
    ///
    /// Three defects, each of which used to be silent and each of which makes a
    /// gate *wider* than it reads:
    ///
    /// - a `since` value that names no baseline (sahjhan #34),
    /// - a `since_filter` that can never match a baseline event (sahjhan #35),
    /// - a candidate-side `filter` using the `{{event.<field>}}` correlation
    ///   form, which only means anything on the anchor side.
    ///
    /// Composite gates nest, so this walks children — a defect buried in an
    /// `any_of` is the same defect as one at the top.
    // [check-gate-windows]
    fn check_gate_windows(&self, gate: &GateConfig, location: &str, errors: &mut Vec<String>) {
        if gate.gate_type == "ledger_has_event_since" {
            // A missing `since` is reported by validate_deep's required-param
            // check; the default it resolves to is `last_transition`, which is
            // a recognized form, so there is nothing to say here.
            if let Err(e) = self.resolve_gate_since(gate) {
                errors.push(format!("{}: gate 'ledger_has_event_since' {}", location, e));
            }
            self.check_since_filter(gate, location, errors);
        }

        // A `filter` is matched against the counted event itself, so
        // `{{event.<field>}}` there is either a tautology or a typo for a state
        // param. It resolves to nothing either way.
        let spec = crate::gates::types::filter_spec(gate, "filter");
        for field in crate::gates::types::candidate_refs(&spec) {
            errors.push(format!(
                "{}: gate '{}' filter correlates on '{}{}', which only resolves \
                 in since_filter — a filter is already matched against the \
                 event it counts",
                location,
                gate.gate_type,
                crate::gates::types::CANDIDATE_PREFIX,
                field
            ));
        }

        for child in &gate.gates {
            self.check_gate_windows(child, location, errors);
        }
    }

    /// Check a `ledger_has_event_since` gate's anchor-side filter.
    ///
    /// A `since_filter` naming a field no baseline event carries matches
    /// nothing, and "no baseline matched" resolves to seq 0 — the same value as
    /// "the baseline has not happened yet", which is the legitimate case for an
    /// actor's first turn. The gate cannot tell those apart at run time, so a
    /// typo silently widens the window to the whole run instead of narrowing it
    /// to one actor. That is sahjhan #34's failure mode on a new surface, and
    /// it is why this is an error where the config is sealed rather than a
    /// diagnostic where the gate runs.
    ///
    /// The candidate-side `filter` needs no such check: a typo there matches no
    /// event, which blocks the gate. Only the anchor side fails open.
    // [check-since-filter]
    fn check_since_filter(&self, gate: &GateConfig, location: &str, errors: &mut Vec<String>) {
        let prefix = format!("{}: gate 'ledger_has_event_since'", location);
        let Some(raw) = gate.params.get("since_filter") else {
            return;
        };
        let Some(table) = raw.as_table() else {
            errors.push(format!(
                "{} since_filter must be a table of field = value pairs",
                prefix
            ));
            return;
        };
        for (key, value) in table {
            if !value.is_str() {
                errors.push(format!(
                    "{} since_filter '{}' must be a string — a ledger field is \
                     a string, so nothing else can ever match",
                    prefix, key
                ));
            }
        }

        let spec = crate::gates::types::filter_spec(gate, "since_filter");

        // A key names a field of the *baseline* event: where the window starts.
        // An unreadable `since` is already reported by the caller, and there is
        // no baseline to check these keys against, so stay quiet about it here.
        if let Ok(baseline) = self.resolve_gate_since(gate) {
            for (key, _) in &spec {
                self.check_filter_field(baseline, key, "since_filter", &prefix, errors);
            }
        }

        // A `{{event.<field>}}` placeholder names a field of the *counted*
        // event: what the window is keyed on, per candidate.
        let counted = gate
            .params
            .get("event")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        for field in crate::gates::types::candidate_refs(&spec) {
            self.check_filter_field(counted, &field, "since_filter correlation", &prefix, errors);
        }
        for (key, value) in &spec {
            if crate::gates::template::find_unresolved_vars(value)
                .iter()
                .any(|p| p == crate::gates::types::CANDIDATE_PREFIX)
            {
                errors.push(format!(
                    "{} since_filter '{}' correlates on '{}' with no field name",
                    prefix,
                    key,
                    crate::gates::types::CANDIDATE_PREFIX
                ));
            }
        }
    }

    /// Report a filter field the protocol does not declare on `event_type`.
    ///
    /// Only decidable for an event declared with fields: the engine writes its
    /// own events (`state_transition` and friends) without declaring them, and
    /// an event declared with no fields says nothing about what it carries.
    /// Undecidable stays silent.
    fn check_filter_field(
        &self,
        event_type: &str,
        field: &str,
        surface: &str,
        prefix: &str,
        errors: &mut Vec<String>,
    ) {
        let Some(cfg) = self.events.get(event_type) else {
            return;
        };
        if cfg.fields.is_empty() || cfg.fields.iter().any(|f| f.name == field) {
            return;
        }
        errors.push(format!(
            "{} {} names field '{}', which event '{}' does not declare \
             (declare it under [[events.{}.fields]] — an anchor that matches \
             nothing widens the window to the whole run rather than closing it)",
            prefix, surface, field, event_type, event_type
        ));
    }

    /// Resolve a `ledger_has_event_since` gate's `since` to the event type its
    /// baseline is the last occurrence of.
    ///
    /// Two forms are recognized, and nothing else:
    ///
    /// - `last_transition` — the last `state_transition`.
    /// - `last_event_of_type:<type>` — the last `<type>`, where `<type>` is
    ///   declared in events.toml or written by the engine itself.
    ///
    /// A recognized anchor whose baseline event has not happened yet still
    /// resolves: the gate then measures from the start of the run (sahjhan #31).
    /// That is only safe because the anchor itself was checked — "the event
    /// hasn't happened" and "that isn't an event" are the same seq 0 to the
    /// gate, and telling them apart is what this function is for (sahjhan #34).
    /// Resolve a gate's `since` **parameter as written** to its baseline event
    /// type — the one place the parameter is read, so validation and evaluation
    /// cannot disagree about what a gate's window is.
    ///
    /// Three readings, and the whole point of #34 is that they used to be
    /// indistinguishable at the point it matters:
    ///
    /// - absent → `last_transition`, the gate's documented default;
    /// - a string → whatever [`Self::resolve_since_anchor`] makes of it;
    /// - anything else → an error. TOML is typed, so `since = 42` is not the
    ///   string `"42"`: it misses `as_str()` and reads as an *absent* `since`,
    ///   silently taking the default. That direction narrows rather than widens,
    ///   but it is the same defect — a value no author intended, applied without
    ///   a word, sealing clean. An anchor the engine cannot read must not be
    ///   sealable, whichever way it happens to move the window.
    // [resolve-gate-since]
    pub fn resolve_gate_since<'a>(
        &self,
        gate: &'a GateConfig,
    ) -> Result<&'a str, SinceAnchorError> {
        match gate.params.get("since") {
            None => Ok("state_transition"),
            Some(value) => match value.as_str() {
                Some(since) => self.resolve_since_anchor(since),
                None => Err(SinceAnchorError::NotAString(value.type_str().to_string())),
            },
        }
    }

    // [resolve-since-anchor]
    pub fn resolve_since_anchor<'a>(&self, since: &'a str) -> Result<&'a str, SinceAnchorError> {
        if since == "last_transition" {
            return Ok("state_transition");
        }
        match since.strip_prefix("last_event_of_type:") {
            // `last_event_of_type:` with nothing after it names no type at all,
            // which reads as a malformed anchor rather than a missing event.
            Some("") => Err(SinceAnchorError::UnrecognizedForm(since.to_string())),
            Some(event_type)
                if self.events.contains_key(event_type) || events::is_engine_event(event_type) =>
            {
                Ok(event_type)
            }
            Some(event_type) => Err(SinceAnchorError::UndeclaredEvent(event_type.to_string())),
            None => Err(SinceAnchorError::UnrecognizedForm(since.to_string())),
        }
    }

    /// Deep validation that includes file-system and cross-reference checks.
    ///
    /// This extends the basic `validate()` with:
    /// - Gate type validation (known types + required params)
    /// - Template file existence (renders.toml paths relative to config_dir)
    /// - Alias target validation (alias values resolve to valid commands)
    /// - Batch validation (steps reference declared queries and transitions)
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
            // `query` takes exactly one of `sql` / `query` — checked in
            // validate_gate rather than by the required-params list.
            ("query", vec![]),
        ]);

        // 5b. Named queries must carry a non-empty predicate.
        for (name, q) in &self.queries {
            if q.sql.trim().is_empty() {
                errors.push(format!("protocol.toml: query '{}' has empty sql", name));
            }
        }

        // 6. Gate type validation (recursive for composite gates).
        for t in &self.transitions {
            for gate in &t.gates {
                self.validate_gate(gate, &t.command, &known_gates, &mut errors);
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
                "transition" if !transition_commands.contains(parts[1]) => {
                    errors.push(format!(
                        "protocol.toml: alias '{}' targets transition '{}' which is not defined",
                        alias_name, parts[1]
                    ));
                }
                "event" if !event_types.contains(parts[1]) => {
                    errors.push(format!(
                        "protocol.toml: alias '{}' targets event type '{}' which is not defined",
                        alias_name, parts[1]
                    ));
                }
                // Valid targets and other command kinds (set, log, status,
                // etc.) are built-in — skip.
                _ => {}
            }
        }

        // 8b. Batch validation. A batch names its population and its verb
        // indirectly, so a typo in either is a command that runs, reports
        // nothing, and exits 0 — the failure a caller is least able to see.
        for (batch_name, batch) in &self.batches {
            if batch.steps.is_empty() {
                errors.push(format!(
                    "protocol.toml: batch '{}' declares no steps",
                    batch_name
                ));
            }
            for step in &batch.steps {
                if !self.queries.contains_key(&step.items) {
                    errors.push(format!(
                        "protocol.toml: batch '{}' step '{}' references query '{}' which is not declared",
                        batch_name,
                        step.value.as_deref().unwrap_or(&step.transition),
                        step.items
                    ));
                }
                if !transition_commands.contains(step.transition.as_str()) {
                    errors.push(format!(
                        "protocol.toml: batch '{}' applies transition '{}' which is not defined",
                        batch_name, step.transition
                    ));
                }
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
                self.validate_gate(gate, &format!("hook[{}]", idx), &known_gates, &mut errors);
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

        // 15b. Attestation lattice: levels must be distinct, and every level
        // named by an event or a transition must be one of them. A typo here
        // would otherwise make a requirement silently unenforceable.
        {
            let mut seen_levels: HashSet<&str> = HashSet::new();
            for level in &self.attestation.levels {
                if !seen_levels.insert(level.as_str()) {
                    errors.push(format!(
                        "protocol.toml: [attestation] levels contains '{}' twice — the ordering must be unambiguous",
                        level
                    ));
                }
            }

            let known_level = |level: &str| self.attestation.rank(level).is_some();

            for (event_name, event) in &self.events {
                if let Some(ref level) = event.attestation {
                    if !known_level(level) {
                        errors.push(format!(
                            "events.toml: event '{}' declares attestation '{}' which is not in [attestation] levels ({})",
                            event_name,
                            level,
                            self.attestation.levels.join(", ")
                        ));
                    }
                }
            }

            for t in &self.transitions {
                if let Some(level) = t
                    .integrity
                    .as_ref()
                    .and_then(|i| i.requires_attestation.as_deref())
                {
                    if !known_level(level) {
                        errors.push(format!(
                            "transitions.toml: transition '{}' requires attestation '{}' which is not in [attestation] levels ({})",
                            t.command,
                            level,
                            self.attestation.levels.join(", ")
                        ));
                    }
                }
            }
        }

        // 16a. Declared event producers.
        for (event_name, event) in &self.events {
            let mut producer_ids: HashSet<&str> = HashSet::new();
            for p in &event.producers {
                if !producer_ids.insert(p.id.as_str()) {
                    errors.push(format!(
                        "events.toml: event '{}' declares producer '{}' twice",
                        event_name, p.id
                    ));
                }
                if let Some(ref states) = p.available_in_states {
                    for s in states {
                        if !state_names.contains(s.as_str()) {
                            errors.push(format!(
                                "events.toml: event '{}' producer '{}' available_in_states references unknown state '{}'",
                                event_name, p.id, s
                            ));
                        }
                    }
                }
            }
        }

        // 16b. Boundary declarations and transition tags must agree.
        {
            let mut boundary_names: HashSet<&str> = HashSet::new();
            for b in &self.boundaries {
                if !boundary_names.insert(b.name.as_str()) {
                    errors.push(format!(
                        "protocol.toml: duplicate boundary name '{}'",
                        b.name
                    ));
                }
                for state in [&b.must_traverse.from, &b.must_traverse.to] {
                    if !state_names.contains(state.as_str()) {
                        errors.push(format!(
                            "protocol.toml: boundary '{}' must_traverse references unknown state '{}'",
                            b.name, state
                        ));
                    }
                }
            }
            for t in &self.transitions {
                if let Some(ref tag) = t.boundary {
                    if !boundary_names.contains(tag.as_str()) {
                        errors.push(format!(
                            "transitions.toml: transition '{}' is tagged boundary = \"{}\" which is not declared in protocol.toml [[boundaries]]",
                            t.command, tag
                        ));
                    }
                }
            }
        }

        // 17. Vault policy state-name validation. Every state named in a
        // writable/readable/deletable whitelist must be a real state, else the
        // op would be silently unreachable (never permitted).
        for policy in self.vault_policies.values() {
            for access in [
                vault_policy::VaultAccess::Store,
                vault_policy::VaultAccess::Read,
                vault_policy::VaultAccess::Delete,
            ] {
                if let Some(states) = policy.states_for(access) {
                    for s in states {
                        if !state_names.contains(s.as_str()) {
                            errors.push(format!(
                                "vault.toml: policy '{}' {} references unknown state '{}'",
                                policy.name,
                                access.adjective(),
                                s
                            ));
                        }
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
    /// `known_gates` map for type and required params. `query` gates are a
    /// special case: they take exactly one of `sql` (inline) or `query` (a
    /// reference into `[queries]`), and a named reference must resolve.
    // [validate-gate]
    fn validate_gate(
        &self,
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
                    self.validate_gate(child, transition_command, known_gates, errors);
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
                    self.validate_gate(child, transition_command, known_gates, errors);
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
                    self.validate_gate(child, transition_command, known_gates, errors);
                }
            }
            "query" => {
                // Exactly one of `sql` (inline) or `query` (named reference).
                let inline = gate.params.get("sql").and_then(|v| v.as_str());
                let named = gate.params.get("query").and_then(|v| v.as_str());
                match (inline, named) {
                    (Some(_), Some(_)) => errors.push(format!(
                        "transitions.toml: gate 'query' in transition '{}' has both 'sql' and 'query' — use one",
                        transition_command
                    )),
                    (None, None) => errors.push(format!(
                        "transitions.toml: gate 'query' in transition '{}' missing required parameter 'sql' or 'query'",
                        transition_command
                    )),
                    (None, Some(name)) if !self.queries.contains_key(name) => errors.push(format!(
                        "transitions.toml: gate 'query' in transition '{}' references undeclared query '{}' (declare [queries.{}] in protocol.toml)",
                        transition_command, name, name
                    )),
                    _ => {}
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

/// Compute SHA-256 hashes of all eight sealed config files.
///
/// Missing optional files (events.toml, renders.toml, hooks.toml, trusted-callers.toml)
/// hash as empty bytes. Returns a BTreeMap with keys: config_seal_protocol,
/// config_seal_states, config_seal_transitions, config_seal_events, config_seal_renders,
/// config_seal_hooks, config_seal_trusted_callers.
///
/// `trusted-callers.toml` is the daemon's signing-authority manifest (who may sign
/// restricted events). Sealing it brings the root of trust inside the tamper-evident
/// boundary: an in-place edit trips `ConfigIntegrityViolation`, so changing signing
/// authority must go through `reseal` (HMAC-authenticated, on the permanent record)
/// rather than a silent file edit + daemon restart (holtz #30).
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
        ("config_seal_trusted_callers", "trusted-callers.toml"),
        ("config_seal_vault", "vault.toml"),
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
