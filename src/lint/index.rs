// src/lint/index.rs
//
// Who produces an event, who consumes it, and what a gate actually requires.
// Everything here is read out of config — no ledger, no execution.
//
// ## Index
// - Producer              — one thing that can record an event, with its availability window
// - ProducerIndex         — event -> producers (declared + inferred + engine)
// - [build-producers]     ProducerIndex::build()   — assemble from events.toml, emits, hooks
// - EventRef              — one event a gate refers to, with polarity
// - [gate-event-refs]     gate_event_refs()        — recursive walk over a gate tree
// - [consumed-events]     consumed_events()        — every event name any config surface reads
// - [sql-event-mentions]  sql_event_mentions()     — declared event names quoted inside a SQL predicate

use std::collections::{HashMap, HashSet};

use crate::config::{GateConfig, ProtocolConfig};

// The engine's own event vocabulary lives with the rest of the vocabulary, in
// config::events — the `since` anchor validator needs it too, and one list is
// the point.
pub use crate::config::events::{is_engine_event, ENGINE_EVENTS};

/// Something that can record an event.
///
/// `id` is opaque to the engine — it exists so a finding can name the producer
/// the consumer declared. Verifying that a declared producer is the real one
/// (that the hook is actually registered, that its script is hash-pinned) is
/// unavoidably the consumer's job; the engine only checks closure and windows.
#[derive(Debug, Clone)]
pub struct Producer {
    pub id: String,
    /// States in which this producer can run. `None` means unconstrained —
    /// the engine knows of no window, so no temporal claim can be made.
    pub available_in_states: Option<Vec<String>>,
    /// Whether the consumer declared this producer (as opposed to the engine
    /// inferring it from an emit / auto_record / built-in).
    pub declared: bool,
}

/// Event name -> everything that can produce it.
pub struct ProducerIndex {
    map: HashMap<String, Vec<Producer>>,
}

impl ProducerIndex {
    // [build-producers]
    /// Assemble the producer index from every source the engine can see:
    ///
    /// 1. `[[events.X.producers]]` — declared by the consumer,
    /// 2. transition `emits` — the transition records the event itself,
    /// 3. hook `auto_record` — the harness records it after a matching tool use,
    /// 4. engine built-ins (`state_transition`, `set_member_complete`, …).
    ///
    /// Deliberately *not* included: `sahjhan event`, which can record any
    /// declared non-restricted event. That is why L1 stays quiet about
    /// unrestricted events unless the protocol opts into `require_producers`.
    pub fn build(config: &ProtocolConfig) -> Self {
        let mut map: HashMap<String, Vec<Producer>> = HashMap::new();

        // 1. Declared producers.
        for (event_name, event) in &config.events {
            for p in &event.producers {
                map.entry(event_name.clone()).or_default().push(Producer {
                    id: p.id.clone(),
                    available_in_states: p.available_in_states.clone(),
                    declared: true,
                });
            }
        }

        // 2. Transition emits — available exactly where the transition can fire.
        for t in &config.transitions {
            for emit in &t.emits {
                map.entry(emit.event.clone()).or_default().push(Producer {
                    id: format!("transition:{}", t.command),
                    available_in_states: Some(vec![t.from.clone()]),
                    declared: false,
                });
            }
        }

        // 3. Hook auto_record — available in the hook's states, if it scopes any.
        for (idx, hook) in config.hooks.iter().enumerate() {
            if let Some(ref auto) = hook.auto_record {
                map.entry(auto.event_type.clone())
                    .or_default()
                    .push(Producer {
                        id: format!("hook[{}]", idx),
                        available_in_states: hook.states.clone(),
                        declared: false,
                    });
            }
        }

        // 4. Engine built-ins.
        for event in ENGINE_EVENTS {
            map.entry((*event).to_string()).or_default().push(Producer {
                id: "engine".to_string(),
                available_in_states: None,
                declared: false,
            });
        }

        ProducerIndex { map }
    }

    /// Producers of `event` — empty when nothing visible can record it.
    pub fn producers_of(&self, event: &str) -> &[Producer] {
        self.map.get(event).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Whether any config surface can produce `event`.
    pub fn is_produced(&self, event: &str) -> bool {
        !self.producers_of(event).is_empty()
    }
}

/// One event referenced by a gate.
#[derive(Debug, Clone)]
pub struct EventRef {
    pub event: String,
    /// Gate type that made the reference (for diagnostics).
    pub gate_type: String,
    /// Whether the event must *exist* for the gate to pass. `ledger_lacks_event`,
    /// `min_elapsed`, and anything under a `not` refer to an event without
    /// requiring it, so a missing producer is not a defect for them.
    pub required: bool,
    /// Whether the reference sits under an `any_of` / `k_of_n`, where a branch
    /// that can never pass still leaves the gate satisfiable by other branches.
    pub disjunctive: bool,
}

// [gate-event-refs]
/// Every event a gate tree refers to, with polarity.
///
/// Composite gates are walked recursively: `not` flips `required`, and
/// `any_of` / `k_of_n` mark their children disjunctive.
pub fn gate_event_refs(gate: &GateConfig, config: &ProtocolConfig) -> Vec<EventRef> {
    let mut out = Vec::new();
    collect_refs(gate, config, true, false, &mut out);
    out
}

fn collect_refs(
    gate: &GateConfig,
    config: &ProtocolConfig,
    positive: bool,
    disjunctive: bool,
    out: &mut Vec<EventRef>,
) {
    let param_str = |key: &str| gate.params.get(key).and_then(|v| v.as_str());
    let mut push = |event: &str, required: bool| {
        if event.is_empty() {
            return;
        }
        out.push(EventRef {
            event: event.to_string(),
            gate_type: gate.gate_type.clone(),
            required: required && positive,
            disjunctive,
        });
    };

    match gate.gate_type.as_str() {
        "any_of" | "k_of_n" => {
            for child in &gate.gates {
                collect_refs(child, config, positive, true, out);
            }
        }
        "all_of" => {
            for child in &gate.gates {
                collect_refs(child, config, positive, disjunctive, out);
            }
        }
        "not" => {
            for child in &gate.gates {
                collect_refs(child, config, !positive, disjunctive, out);
            }
        }
        "ledger_has_event" => {
            // `min_count = 0` with a `max_count` ceiling is a budget check —
            // it passes with no events at all, so it requires nothing.
            let min_count = gate
                .params
                .get("min_count")
                .and_then(|v| v.as_integer())
                .unwrap_or(1);
            if let Some(event) = param_str("event") {
                push(event, min_count >= 1);
            }
        }
        "ledger_has_event_since" => {
            if let Some(event) = param_str("event") {
                push(event, true);
            }
            // The `since` baseline is a reference, not a requirement: a missing
            // baseline is treated as the run start.
            if let Some(since) = param_str("since") {
                if since != "last_transition" {
                    let baseline = since.strip_prefix("last_event_of_type:").unwrap_or(since);
                    push(baseline, false);
                }
            }
        }
        "ledger_lacks_event" => {
            if let Some(event) = param_str("event") {
                push(event, false);
            }
        }
        "min_elapsed" => {
            // Passes when the event has never happened — a reference, not a
            // requirement.
            if let Some(event) = param_str("event") {
                push(event, false);
            }
        }
        "set_covered" => {
            push(param_str("event").unwrap_or("set_member_complete"), true);
        }
        "no_violations" => {
            push("protocol_violation", false);
            push("violation_resolved", false);
        }
        "query" => {
            // The predicate is opaque SQL; the best the engine can do is note
            // which declared event names it names. Never a requirement — the
            // predicate may well be satisfied by their absence.
            if let Ok(sql) = crate::gates::query::resolve_gate_sql(gate, config) {
                for event in sql_event_mentions(&sql, config) {
                    push(&event, false);
                }
            }
        }
        _ => {}
    }
}

// [consumed-events]
/// Every event name that some config surface reads.
///
/// Used by L5 to tell dead vocabulary from a live declaration: gates, render
/// triggers, hook checks, and hook auto_record targets all count.
pub fn consumed_events(config: &ProtocolConfig) -> HashSet<String> {
    let mut consumed: HashSet<String> = HashSet::new();

    for t in &config.transitions {
        for gate in &t.gates {
            for r in gate_event_refs(gate, config) {
                consumed.insert(r.event);
            }
        }
    }

    for hook in &config.hooks {
        if let Some(ref gate) = hook.gate {
            for r in gate_event_refs(gate, config) {
                consumed.insert(r.event);
            }
        }
        if let Some(ref check) = hook.check {
            if let Some(ref sql) = check.sql {
                for event in sql_event_mentions(sql, config) {
                    consumed.insert(event);
                }
            }
            if let Some(ref types) = check.event_types {
                consumed.extend(types.iter().cloned());
            }
        }
    }

    for monitor in &config.monitors {
        if let Some(ref types) = monitor.trigger.event_types {
            consumed.extend(types.iter().cloned());
        }
    }

    for render in &config.renders {
        if let Some(ref types) = render.event_types {
            for t in types {
                consumed.insert(t.clone());
            }
        }
    }

    // A named query is a predicate the protocol keeps around on purpose; the
    // events it names are read even if no gate references the query yet.
    for q in config.queries.values() {
        for event in sql_event_mentions(&q.sql, config) {
            consumed.insert(event);
        }
    }

    consumed
}

// [sql-event-mentions]
/// Declared event names appearing as quoted literals in a SQL predicate.
///
/// Deliberately syntactic: this is not a SQL parser, and it neither knows nor
/// needs to know what the predicate means. It exists so an event used only
/// inside a `query` gate is not reported as dead vocabulary.
pub fn sql_event_mentions(sql: &str, config: &ProtocolConfig) -> Vec<String> {
    let mut found = Vec::new();
    for literal in quoted_literals(sql) {
        if config.events.contains_key(&literal) && !found.contains(&literal) {
            found.push(literal);
        }
    }
    found
}

/// Extract single- and double-quoted string literals from `sql`.
fn quoted_literals(sql: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut chars = sql.chars();
    while let Some(c) = chars.next() {
        if c == '\'' || c == '"' {
            let quote = c;
            let mut literal = String::new();
            for c2 in chars.by_ref() {
                if c2 == quote {
                    break;
                }
                literal.push(c2);
            }
            if !literal.is_empty() {
                out.push(literal);
            }
        }
    }
    out
}
