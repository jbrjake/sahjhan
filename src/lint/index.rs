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
// - [producible-stamps]   producible_stamps()      — the provenance values some writer of an event can stamp
// - [stamped-columns]     stamped_columns()        — field name -> the events declaring it `stamped`
// - ProvenanceFilter      — one comparison a predicate makes against a stamped column
// - [provenance-filters]  provenance_filters()     — those comparisons, and whether each is an equality

use std::collections::{HashMap, HashSet};

use crate::config::{GateConfig, ProtocolConfig};
use crate::provenance::Recorder;

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

// [producible-stamps]
/// Every provenance value that some writer of `events` can stamp.
///
/// An empty `events` means "any declared event" — used when a predicate names
/// no declared event, so the only sound question left is whether the value is
/// producible *anywhere* in this protocol.
///
/// The enumeration mirrors [`crate::provenance::Recorder`] one arm at a time,
/// because each arm is reachable under different config:
///
/// - `agent:cli` / `authed:cli` — `sahjhan event` records any declared,
///   non-restricted event; `authed-event` records the restricted ones.
/// - `engine:emit:<command>` — one per transition that emits the event.
/// - `agent:hook-eval` — one per hook `auto_record` targeting it.
/// - `hook:<path>` — one per entry in `trusted-callers.toml`. With no manifest
///   the daemon authenticates nobody, so what it can stamp is
///   `daemon:unverified` and nothing else.
pub fn producible_stamps(config: &ProtocolConfig, events: &[String]) -> HashSet<String> {
    let names: Vec<&String> = if events.is_empty() {
        config.events.keys().collect()
    } else {
        events
            .iter()
            .filter(|e| config.events.contains_key(*e))
            .collect()
    };

    let mut out = HashSet::new();

    // The daemon's record_event is not scoped to an event type, so its
    // identities are available wherever any event is.
    if !names.is_empty() {
        match &config.trusted_callers {
            Some(manifest) => {
                for path in manifest.callers.keys() {
                    out.insert(Recorder::TrustedCaller(path.clone()).id());
                }
            }
            None => {
                out.insert(Recorder::UnverifiedPeer.id());
            }
        }
    }

    for name in names {
        let Some(event) = config.events.get(name) else {
            continue;
        };
        if event.restricted == Some(true) {
            out.insert(Recorder::AuthedCli.id());
        } else {
            out.insert(Recorder::AgentCli.id());
        }
        for t in &config.transitions {
            if t.emits.iter().any(|e| &e.event == name) {
                out.insert(Recorder::Emit(t.command.clone()).id());
            }
        }
        for hook in &config.hooks {
            if hook
                .auto_record
                .as_ref()
                .is_some_and(|a| &a.event_type == name)
            {
                out.insert(Recorder::AgentHookEval.id());
            }
        }
    }

    out
}

// [stamped-columns]
/// Every field name some declared event marks `stamped = true`, mapped to the
/// events that mark it.
///
/// This is what makes L8 a check on the *declaration* rather than on how a
/// value happens to be spelled. An earlier cut matched literals against a
/// reserved set of prefixes (`hook:`, `engine:`, …), which meant an ordinary
/// field holding an ordinary value — `reason = 'hook:something'` — was read as
/// a provenance filter and reported. Reserved-string rules also teach the
/// reader that the syntax is a pile of special cases; the config already says
/// which columns are provenance, so ask it.
pub fn stamped_columns(config: &ProtocolConfig) -> HashMap<&str, Vec<String>> {
    let mut out: HashMap<&str, Vec<String>> = HashMap::new();
    for (name, event) in &config.events {
        for field in crate::provenance::stamped_fields(event) {
            out.entry(field).or_default().push(name.clone());
        }
    }
    for events in out.values_mut() {
        events.sort();
    }
    out
}

/// One comparison a predicate makes against a stamped column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProvenanceFilter {
    /// The stamped field the literal is compared against, table qualifier
    /// stripped (`d.recorded_by` → `recorded_by`).
    pub column: String,
    /// The literal it is compared against.
    pub value: String,
    /// Whether a bare `=` reaches it, as opposed to `!=` / `<>` / `NOT (… = …)`
    /// / an `IN` list.
    pub positive_equality: bool,
}

// [provenance-filters]
/// Every literal in a SQL predicate compared against one of `columns`.
///
/// Syntactic, like the rest of lint, and this is not a SQL parser. The column
/// is found by scanning left from the literal past whitespace, comparison and
/// list punctuation, other literals, and the words `IN` / `NOT`, stopping at
/// the first identifier — so `col = 'x'`, `col != 'x'`, `NOT (col = 'x')` and
/// `col NOT IN ('x','y')` all resolve to `col`. A bare literal with no column
/// to its left lands on whatever keyword precedes it, which is not a stamped
/// column, so it is ignored rather than guessed at. A column written in double
/// quotes is read as a literal by the same scan and so is missed; that is the
/// price of one pass over the characters.
///
/// The equality flag is decided by the operator immediately before the opening
/// quote. Anything unreadable that way counts as *not* an equality, so the
/// check depending on it stays quiet rather than guessing — a false "this
/// filter is decoration" is worse than a miss.
pub fn provenance_filters(
    sql: &str,
    columns: &HashMap<&str, Vec<String>>,
) -> Vec<ProvenanceFilter> {
    let chars: Vec<char> = sql.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c != '\'' && c != '"' {
            i += 1;
            continue;
        }
        let quote = c;
        let open = i;
        let mut value = String::new();
        i += 1;
        while i < chars.len() && chars[i] != quote {
            value.push(chars[i]);
            i += 1;
        }
        i += 1; // step past the closing quote (or off the end)

        let Some(column) = column_before(&chars, open) else {
            continue;
        };
        if !columns.contains_key(column.as_str()) {
            continue;
        }
        out.push(ProvenanceFilter {
            column,
            value,
            positive_equality: positive_equality_before(&chars, open),
        });
    }
    out
}

/// The column a literal opening at `open` is being compared against.
fn column_before(chars: &[char], open: usize) -> Option<String> {
    let mut j = open;
    loop {
        j = skip_ws_back(chars, j);
        if j == 0 {
            return None;
        }
        let c = chars[j - 1];
        // Comparison and list punctuation sits between a column and its values.
        if matches!(c, '=' | '!' | '<' | '>' | '(' | ',') {
            j -= 1;
            continue;
        }
        // Another value in the same `IN` list.
        if c == '\'' || c == '"' {
            let mut m = j - 1;
            if m == 0 {
                return None;
            }
            m -= 1;
            while m > 0 && chars[m] != c {
                m -= 1;
            }
            if chars[m] != c {
                return None;
            }
            j = m;
            continue;
        }
        if !is_identifier_char(c) {
            return None;
        }
        let end = j;
        while j > 0 && is_identifier_char(chars[j - 1]) {
            j -= 1;
        }
        let word: String = chars[j..end].iter().collect();
        // `IN` and `NOT` stand between the column and its list; anything else
        // that reads as an identifier is the column (or a keyword that is not a
        // stamped column, which the caller drops).
        if word.eq_ignore_ascii_case("in") || word.eq_ignore_ascii_case("not") {
            continue;
        }
        // Strip a table qualifier: `d.recorded_by` → `recorded_by`.
        return Some(word.rsplit('.').next().unwrap_or(&word).to_string());
    }
}

fn is_identifier_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '.'
}

/// Whether the literal opening at `open` is compared with a bare `=` that no
/// visible negation inverts.
///
/// Three spellings of "not this writer" are recognized and excluded: `!=`,
/// `<>`, and `NOT (<column> = '…')`. A negation spelled any other way — one
/// scoped further out, a `CASE`, a subquery — reads as positive here, which is
/// the cost of not shipping a SQL parser into a lint pass.
fn positive_equality_before(chars: &[char], open: usize) -> bool {
    let j = skip_ws_back(chars, open);
    if j == 0 || chars[j - 1] != '=' {
        return false;
    }
    let eq = j - 1;
    // `!=`, `<>`, `>=`, `<=` — anything but a lone `=`.
    if eq > 0 && matches!(chars[eq - 1], '!' | '<' | '>') {
        return false;
    }
    !wrapped_in_not(chars, eq)
}

/// Whether the comparison whose `=` sits at `eq` is written `NOT (<column> = …`.
fn wrapped_in_not(chars: &[char], eq: usize) -> bool {
    // Back over the column expression on the left of the `=`.
    let mut j = skip_ws_back(chars, eq);
    while j > 0 && (chars[j - 1].is_alphanumeric() || matches!(chars[j - 1], '_' | '.' | '"')) {
        j -= 1;
    }
    j = skip_ws_back(chars, j);
    if j == 0 || chars[j - 1] != '(' {
        return false;
    }
    j = skip_ws_back(chars, j - 1);
    if j < 3 {
        return false;
    }
    let word: String = chars[j - 3..j].iter().collect();
    // A word boundary, so `cannot (` is not read as a negation.
    word.eq_ignore_ascii_case("not") && (j == 3 || !chars[j - 4].is_alphanumeric())
}

/// The index `k` such that `chars[k - 1]` is the first non-whitespace character
/// before `from`, or 0.
fn skip_ws_back(chars: &[char], from: usize) -> usize {
    let mut k = from;
    while k > 0 && chars[k - 1].is_whitespace() {
        k -= 1;
    }
    k
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `recorded_by` is declared stamped; `reason` is an ordinary field.
    fn columns() -> HashMap<&'static str, Vec<String>> {
        HashMap::from([("recorded_by", vec!["finding_deferred".to_string()])])
    }

    fn filters(sql: &str) -> Vec<ProvenanceFilter> {
        provenance_filters(sql, &columns())
    }

    #[test]
    fn only_comparisons_against_a_stamped_column_count() {
        // The declaration decides, not the literal's spelling. An ordinary
        // field holding a stamp-shaped value is none of L8's business.
        assert!(filters("WHERE reason='hook:something'").is_empty());
        assert!(filters("WHERE type='finding_deferred'").is_empty());
        assert_eq!(
            filters("WHERE recorded_by='agent:cli'"),
            vec![ProvenanceFilter {
                column: "recorded_by".to_string(),
                value: "agent:cli".to_string(),
                positive_equality: true,
            }]
        );
    }

    #[test]
    fn a_misspelled_value_is_still_a_provenance_filter() {
        // It reaches the check because of the column it is compared against,
        // so the typo is caught rather than skipped for not matching a prefix.
        let f = filters("WHERE recorded_by='engine:emits:defer_low'");
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].value, "engine:emits:defer_low");
    }

    #[test]
    fn negations_are_not_equalities() {
        for sql in [
            "WHERE recorded_by <> 'agent:cli'",
            "WHERE recorded_by != 'agent:cli'",
            "WHERE recorded_by!='agent:cli'",
            "WHERE NOT (recorded_by = 'agent:cli')",
            "WHERE NOT(d.recorded_by='agent:cli')",
            "WHERE recorded_by IN ('agent:cli')",
            "WHERE recorded_by NOT IN ('agent:cli','authed:cli')",
            "WHERE recorded_by >= 'agent:cli'",
        ] {
            let f = filters(sql);
            assert!(!f.is_empty(), "`{sql}` names a stamped column");
            assert!(
                f.iter().all(|x| !x.positive_equality),
                "`{sql}` is not a positive equality"
            );
        }
    }

    #[test]
    fn equalities_are_recognized_through_whitespace_and_qualifiers() {
        for sql in [
            "WHERE recorded_by='agent:cli'",
            "WHERE recorded_by = 'agent:cli'",
            "WHERE d.recorded_by   =   'agent:cli'",
            "WHERE cannot (recorded_by = 'agent:cli')",
        ] {
            let f = filters(sql);
            assert_eq!(f.len(), 1, "{sql}");
            assert!(f[0].positive_equality, "`{sql}` is a positive equality");
        }
    }

    #[test]
    fn an_in_list_resolves_every_value_to_its_column() {
        let f = filters("WHERE recorded_by IN ('hook:a', 'hook:b')");
        assert_eq!(f.len(), 2, "{f:?}");
        assert!(f.iter().all(|x| x.column == "recorded_by"));
        assert_eq!(f[0].value, "hook:a");
        assert_eq!(f[1].value, "hook:b");
    }

    #[test]
    fn a_literal_with_no_column_to_its_left_is_ignored() {
        // Lands on a keyword, which is not a stamped column, so it is dropped
        // rather than guessed at.
        assert!(filters("WHERE type='finding_deferred' AND 'agent:cli'").is_empty());
    }

    #[test]
    fn every_comparison_in_a_predicate_is_reported() {
        let f = filters("WHERE recorded_by='hook:hooks/courier.py' OR recorded_by <> 'agent:cli'");
        assert_eq!(f.len(), 2, "{f:?}");
        assert!(f[0].positive_equality);
        assert!(!f[1].positive_equality);
    }
}
