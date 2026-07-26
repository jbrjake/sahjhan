// src/lint/checks.rs
//
// The checks themselves. Each returns findings and, where a later check
// depends on it, records its verdict on the shared `Analysis`.
//
// ## Index
// - [check-l1]  l1_unsatisfiable_gates()  — a required event nothing can produce
// - [check-l4]  l4_dead_end_states()      — a non-terminal state with no usable exit
// - [check-l5]  l5_dead_vocabulary()      — a declared event nothing produces or consumes

use crate::config::GateConfig;

use super::index::{gate_event_refs, is_engine_event, EventRef};
use super::{Analysis, LintFinding};

// [check-l1]
/// L1 — every event a gate *requires* must have at least one producer.
///
/// A gate that waits for an event nothing can record is not a strict gate; it
/// is a wall. The agent will read the intent, do the work, and still be
/// blocked, with no action available that would change the verdict.
///
/// What counts as a producer is deliberately conservative (see
/// [`super::index::ProducerIndex::build`]). The severity ladder reflects how
/// much the engine actually knows:
///
/// - **restricted event, no producer** — error. Only `authed-event` with an
///   HMAC proof can record it, so the agent cannot unblock itself, and no
///   declared producer says anyone else will.
/// - **`[lint] require_producers = true`** — error for any required event with
///   no producer: the protocol has opted into producer closure.
/// - **otherwise** — silent. `sahjhan event` can record any declared,
///   non-restricted event, so the engine cannot claim the gate is unsatisfiable.
///
/// A reference under `any_of` / `k_of_n` is downgraded to a warning: the branch
/// can never contribute, but the gate as a whole may still pass.
///
/// Records every blocked transition in `analysis.unsatisfiable` for L4.
pub fn l1_unsatisfiable_gates(analysis: &mut Analysis) -> Vec<LintFinding> {
    let config = analysis.config;
    let mut findings = Vec::new();
    let mut blocked: Vec<(usize, String)> = Vec::new();

    for (idx, t) in config.transitions.iter().enumerate() {
        let location = analysis.transition_location(idx);
        for gate in &t.gates {
            for r in gate_event_refs(gate, config) {
                if let Some(defect) = classify(analysis, &r) {
                    if defect.fatal && !r.disjunctive {
                        blocked.push((idx, defect.message.clone()));
                    }
                    findings.push(finding_for(&location, &r, &defect));
                }
            }
        }
    }

    for (idx, reason) in blocked {
        analysis.unsatisfiable.entry(idx).or_insert(reason);
    }

    // Hook gates fire their action when the gate *fails*, so a hook gate that
    // can never pass is a hook that always fires.
    for (idx, hook) in config.hooks.iter().enumerate() {
        let Some(gate) = hook.gate.as_ref() else {
            continue;
        };
        let location = format!("hooks.toml: hook[{}]", idx);
        for r in gate_event_refs(gate, config) {
            if let Some(mut defect) = classify(analysis, &r) {
                let action = hook.action.as_deref().unwrap_or("fire");
                defect.message = format!(
                    "{} — the hook fires when its gate fails, so it would '{}' on every matching tool use",
                    defect.message, action
                );
                findings.push(finding_for(&location, &r, &defect));
            }
        }
    }

    findings
}

/// What is wrong with one event reference, if anything.
struct Defect {
    message: String,
    hint: String,
    /// Whether this makes the gate provably unpassable (as opposed to merely
    /// suspicious).
    fatal: bool,
}

/// Decide whether an event reference is a defect, and how serious.
///
/// Returns `None` for references the engine cannot fault: unrequired ones
/// (`ledger_lacks_event`, `min_elapsed`, anything under a `not`), engine
/// events, and unrestricted events in a protocol that has not opted into
/// producer closure.
fn classify(analysis: &Analysis, r: &EventRef) -> Option<Defect> {
    let config = analysis.config;

    if !r.required || is_engine_event(&r.event) {
        return None;
    }

    let declared = config.events.get(&r.event);

    // An event nobody declared. Legal — `sahjhan event` accepts undeclared
    // types — but far more often a typo, so say so when there is a vocabulary
    // to compare against.
    if declared.is_none() {
        if config.events.is_empty() {
            return None;
        }
        return Some(Defect {
            message: format!(
                "gate '{}' requires event '{}', which is not declared in events.toml",
                r.gate_type, r.event
            ),
            hint: format!(
                "declare [events.{}] — an undeclared event gets no field validation and no producer can be checked",
                r.event
            ),
            fatal: false,
        });
    }

    if analysis.producers.is_produced(&r.event) {
        return None;
    }

    let restricted = declared.and_then(|e| e.restricted).unwrap_or(false);
    if restricted {
        return Some(Defect {
            message: format!(
                "gate '{}' requires restricted event '{}', which has no producer",
                r.gate_type, r.event
            ),
            hint: format!(
                "a restricted event can only be recorded by 'authed-event' with an HMAC proof — \
                 the agent cannot unblock itself. Declare [[events.{}.producers]] naming what signs it",
                r.event
            ),
            fatal: true,
        });
    }

    if config.lint.require_producers {
        return Some(Defect {
            message: format!(
                "gate '{}' requires event '{}', which has no declared producer",
                r.gate_type, r.event
            ),
            hint: format!(
                "declare [[events.{}.producers]], emit it from a transition, or auto_record it from a hook",
                r.event
            ),
            fatal: true,
        });
    }

    None
}

/// Turn a defect into a finding, downgrading disjunctive references.
fn finding_for(location: &str, r: &EventRef, defect: &Defect) -> LintFinding {
    let disjunctive_note = " (one branch of an any_of/k_of_n — the gate may still pass by another)";
    if defect.fatal && !r.disjunctive {
        LintFinding::error("L1", location.to_string(), defect.message.clone())
            .with_hint(&defect.hint)
    } else if r.disjunctive {
        LintFinding::warning(
            "L1",
            location.to_string(),
            format!("{}{}", defect.message, disjunctive_note),
        )
        .with_hint(&defect.hint)
    } else {
        LintFinding::warning("L1", location.to_string(), defect.message.clone())
            .with_hint(&defect.hint)
    }
}

// [check-l4]
/// L4 — every non-terminal state must have at least one exit that can be taken.
///
/// Two ways to fail: no outgoing transition at all, or outgoing transitions
/// that are all blocked by an unsatisfiable gate. Either way the run stops
/// there with no command the agent can issue to move on, which is exactly the
/// failure that reads to a user as "the tool is broken".
///
/// Deliberately weak: it only knows about gates L1 (and L2) proved
/// unsatisfiable. A gate that is merely very hard to satisfy is not this
/// check's business.
pub fn l4_dead_end_states(analysis: &Analysis) -> Vec<LintFinding> {
    let config = analysis.config;
    let mut findings = Vec::new();

    let mut names: Vec<&String> = config.states.keys().collect();
    names.sort();

    for name in names {
        let state = &config.states[name];
        if state.terminal.unwrap_or(false) {
            continue;
        }

        let outgoing = analysis.graph.outgoing(name);
        let location = format!("states.toml: state '{}'", name);

        if outgoing.is_empty() {
            findings.push(
                LintFinding::error(
                    "L4",
                    location,
                    format!(
                        "non-terminal state '{}' has no outgoing transition — a run that reaches it cannot continue",
                        name
                    ),
                )
                .with_hint("add a transition out of it, or mark the state terminal = true"),
            );
            continue;
        }

        let usable: Vec<&&super::graph::Edge> = outgoing
            .iter()
            .filter(|e| !analysis.unsatisfiable.contains_key(&e.index))
            .collect();

        if usable.is_empty() {
            let blocked: Vec<String> = outgoing
                .iter()
                .map(|e| {
                    format!(
                        "'{}': {}",
                        e.command,
                        analysis
                            .unsatisfiable
                            .get(&e.index)
                            .map(|s| s.as_str())
                            .unwrap_or("blocked")
                    )
                })
                .collect();
            findings.push(
                LintFinding::error(
                    "L4",
                    location,
                    format!(
                        "every exit from non-terminal state '{}' is blocked by an unsatisfiable gate — {}",
                        name,
                        blocked.join("; ")
                    ),
                )
                .with_hint("fix the L1 findings on those transitions, or add an escape transition"),
            );
        }
    }

    findings
}

// [check-l5]
/// L5 — every declared event should be produced or consumed by something.
///
/// An event that nothing records and nothing reads is vocabulary that has
/// drifted out of the protocol: usually the remains of a removed gate, and a
/// trap for the next reader who assumes it is load-bearing.
///
/// Warning only — declaring an event ahead of the gate that will read it is a
/// legitimate order of work.
pub fn l5_dead_vocabulary(analysis: &Analysis) -> Vec<LintFinding> {
    let config = analysis.config;
    let mut findings = Vec::new();

    let mut names: Vec<&String> = config.events.keys().collect();
    names.sort();

    for name in names {
        if is_engine_event(name) {
            continue;
        }
        let produced = config
            .events
            .get(name)
            .map(|e| !e.producers.is_empty())
            .unwrap_or(false)
            || analysis.producers.is_produced(name);
        let consumed = analysis.consumed.contains(name);

        if !produced && !consumed {
            findings.push(
                LintFinding::warning(
                    "L5",
                    format!("events.toml: event '{}'", name),
                    format!(
                        "event '{}' is declared but never produced or consumed by any gate, render, hook, or emit",
                        name
                    ),
                )
                .with_hint("remove it, or wire it to the gate/render that was meant to read it"),
            );
        }
    }

    findings
}

/// Whether a gate tree contains a gate of the given type (helper for later checks).
#[allow(dead_code)]
pub(crate) fn gate_tree_contains(gate: &GateConfig, gate_type: &str) -> bool {
    gate.gate_type == gate_type || gate.gates.iter().any(|g| gate_tree_contains(g, gate_type))
}
