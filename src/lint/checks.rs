// src/lint/checks.rs
//
// The checks themselves. Each returns findings and, where a later check
// depends on it, records its verdict on the shared `Analysis`.
//
// ## Index
// - [check-l1]  l1_unsatisfiable_gates()  — a required event nothing can produce
// - [check-l2]  l2_temporally_unsatisfiable() — a producer that can never run before the gate needing it
// - [check-l3]  l3_boundary_route_around() — a path that reaches a boundary's target without crossing it
// - [check-l4]  l4_dead_end_states()      — a non-terminal state with no usable exit
// - [check-l5]  l5_dead_vocabulary()      — a declared event nothing produces or consumes
// - [check-l6]  l6_predicate_drift()      — inline predicates near-identical to a named query or to each other
// - [check-l7]  l7_forgeable_evidence()   — a gate requiring evidence stronger than its producer supplies
// - [inline-predicates] inline_query_predicates() — every inline query gate SQL, with its location

use crate::config::GateConfig;

use super::index::{gate_event_refs, is_engine_event, EventRef};
use super::similarity;
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

// [check-l2]
/// L2 — a required event's producers must be able to run *before* the gate that
/// needs them.
///
/// A gate on a transition out of state `S` reads events recorded at any earlier
/// point in the run, so its producers must be able to run somewhere in
/// `ancestors(S) ∪ {S}`. When every declared producer of a required event has
/// an `available_in_states` window and none of those windows intersects that
/// set, the gate is unsatisfiable — not because nothing can record the event,
/// but because nothing can record it *in time*. The agent will read the intent,
/// find the producer, and discover it only runs in a state the run has already
/// left, or has not reached and cannot reach without passing this gate.
///
/// This is the check that has to live in the engine. A consumer can grep its own
/// config for producers and windows; it cannot compute "which states can precede
/// S" without reimplementing the state machine. That reachability relation is
/// the engine's alone.
///
/// A producer with no declared window is unconstrained, so one such producer is
/// enough to silence the check: the engine makes no temporal claim it cannot
/// support. Records blocked transitions in `analysis.unsatisfiable` for L4.
pub fn l2_temporally_unsatisfiable(analysis: &mut Analysis) -> Vec<LintFinding> {
    let config = analysis.config;
    let mut findings = Vec::new();
    let mut blocked: Vec<(usize, String)> = Vec::new();

    for (idx, t) in config.transitions.iter().enumerate() {
        // States from which this gate could be evaluated: everything that can
        // reach `t.from`, plus `t.from` itself (events recorded while sitting
        // in that state precede the transition out of it).
        let mut reachable_before = analysis.graph.ancestors_of(&t.from);
        reachable_before.insert(t.from.as_str());

        let location = analysis.transition_location(idx);

        for gate in &t.gates {
            for r in gate_event_refs(gate, config) {
                if !r.required || is_engine_event(&r.event) {
                    continue;
                }
                let producers = analysis.producers.producers_of(&r.event);
                if producers.is_empty() {
                    // L1's business, not this check's.
                    continue;
                }
                // Any producer without a window can run anywhere.
                if producers.iter().any(|p| p.available_in_states.is_none()) {
                    continue;
                }
                let usable = producers.iter().any(|p| {
                    p.available_in_states
                        .as_ref()
                        .map(|states| states.iter().any(|s| reachable_before.contains(s.as_str())))
                        .unwrap_or(true)
                });
                if usable {
                    continue;
                }

                let windows: Vec<String> = producers
                    .iter()
                    .map(|p| {
                        format!(
                            "{} (in {})",
                            p.id,
                            p.available_in_states
                                .as_ref()
                                .map(|s| if s.is_empty() {
                                    "no state".to_string()
                                } else {
                                    s.join(", ")
                                })
                                .unwrap_or_else(|| "any state".to_string())
                        )
                    })
                    .collect();

                let message = format!(
                    "gate '{}' requires event '{}', but every producer runs only in states that cannot precede '{}' — {}",
                    r.gate_type, r.event, t.from, windows.join("; ")
                );

                if r.disjunctive {
                    findings.push(
                        LintFinding::warning(
                            "L2",
                            location.clone(),
                            format!(
                                "{} (one branch of an any_of/k_of_n — the gate may still pass by another)",
                                message
                            ),
                        )
                        .with_hint(
                            "widen the producer's available_in_states, or move the gate to a transition the producer can precede",
                        ),
                    );
                } else {
                    blocked.push((idx, message.clone()));
                    findings.push(
                        LintFinding::error("L2", location.clone(), message).with_hint(
                            "widen the producer's available_in_states, or move the gate to a transition the producer can precede",
                        ),
                    );
                }
            }
        }
    }

    for (idx, reason) in blocked {
        analysis.unsatisfiable.entry(idx).or_insert(reason);
    }

    findings
}

// [check-l3]
/// L3 — a boundary edge must not be routable around.
///
/// A `[[boundaries]]` entry declares that every path from `must_traverse.from`
/// to `must_traverse.to` crosses an edge tagged with the boundary's name. The
/// check is one graph operation: delete every tagged edge, then ask whether the
/// target is still reachable from the source. If it is, that surviving path is
/// the bypass, and it is printed verbatim.
///
/// This is the check that most repays being in the engine. A consumer can grep
/// its own transitions.toml for the tag; it cannot see that a second transition
/// added months later — sharing a command name, deliberately ungated, meant for
/// an unrelated pause state — now offers a way around. That is a property of
/// the graph, not of any one declaration.
pub fn l3_boundary_route_around(analysis: &Analysis) -> Vec<LintFinding> {
    let config = analysis.config;
    let mut findings = Vec::new();

    for boundary in &config.boundaries {
        let location = format!("protocol.toml: boundary '{}'", boundary.name);
        let from = boundary.must_traverse.from.as_str();
        let to = boundary.must_traverse.to.as_str();

        // Unknown states make the boundary vacuous rather than satisfied.
        for state in [from, to] {
            if !config.states.contains_key(state) {
                findings.push(
                    LintFinding::error(
                        "L3",
                        location.clone(),
                        format!(
                            "boundary '{}' names unknown state '{}' in must_traverse",
                            boundary.name, state
                        ),
                    )
                    .with_hint("must_traverse.from and .to must be declared states"),
                );
            }
        }
        if !config.states.contains_key(from) || !config.states.contains_key(to) {
            continue;
        }

        let tagged: Vec<&str> = config
            .transitions
            .iter()
            .filter(|t| t.boundary.as_deref() == Some(boundary.name.as_str()))
            .map(|t| t.command.as_str())
            .collect();

        if tagged.is_empty() {
            findings.push(
                LintFinding::error(
                    "L3",
                    location.clone(),
                    format!(
                        "boundary '{}' is declared but no transition carries boundary = \"{}\" — nothing enforces it",
                        boundary.name, boundary.name
                    ),
                )
                .with_hint(format!(
                    "tag the transition that performs the boundary with boundary = \"{}\"",
                    boundary.name
                )),
            );
            continue;
        }

        // Reachability with every tagged edge removed: what survives is a bypass.
        let name = boundary.name.as_str();
        let pruned =
            super::graph::Graph::build_filtered(config, |t| t.boundary.as_deref() != Some(name));

        if let Some(path) = pruned.path_between(from, to) {
            findings.push(
                LintFinding::error(
                    "L3",
                    location,
                    format!(
                        "boundary '{}' can be routed around: {} reaches {} without crossing it \u{2014} {}",
                        boundary.name,
                        from,
                        to,
                        super::graph::format_path(&path)
                    ),
                )
                .with_hint(format!(
                    "tag that path's edge with boundary = \"{}\", or remove the route (tagged today: {})",
                    boundary.name,
                    tagged.join(", ")
                )),
            );
        }
    }

    findings
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

// [check-l6]
/// L6 — a predicate that decides a fact should exist once, not twice.
///
/// Two gates that must agree about the same fact, each carrying its own copy of
/// the SQL, are two strings hoped to be equal. They stay equal exactly as long
/// as nobody edits one of them. holtz #77 is what happens when someone does:
/// a blocking condition and the escape hatch it printed drifted apart until the
/// block was strictly stronger than its own escape, and the run deadlocked
/// while telling the agent exactly which impossible thing to do.
///
/// The check is textual on purpose. Two predicates that mean the same thing but
/// read differently are not lint's business; two that read *almost* the same
/// are, because that is what one predicate looks like after someone edited a
/// copy of it. Reported as warnings — duplication is a maintainability defect,
/// not a protocol that is provably broken today.
///
/// The fix in both cases is `[queries.<name>]`: give the fact a name and let
/// both gates reference it, so they are the same object rather than two strings.
pub fn l6_predicate_drift(analysis: &Analysis) -> Vec<LintFinding> {
    let config = analysis.config;
    let mut findings = Vec::new();

    let threshold = config
        .lint
        .similarity_threshold
        .unwrap_or(similarity::DEFAULT_THRESHOLD);

    let inline = inline_query_predicates(config);
    let normalized: Vec<String> = inline
        .iter()
        .map(|(_, sql)| similarity::normalize_sql(sql))
        .collect();

    let named: Vec<(&String, String)> = {
        let mut v: Vec<(&String, String)> = config
            .queries
            .iter()
            .map(|(name, q)| (name, similarity::normalize_sql(&q.sql)))
            .collect();
        v.sort_by(|a, b| a.0.cmp(b.0));
        v
    };

    // 1. Inline predicate vs. named query.
    let mut matched_named: Vec<Option<&str>> = vec![None; inline.len()];
    for (i, (location, _)) in inline.iter().enumerate() {
        for (name, named_sql) in &named {
            let score = similarity::similarity(&normalized[i], named_sql);
            if score < threshold {
                continue;
            }
            matched_named[i] = Some(name.as_str());
            let message = if &normalized[i] == named_sql {
                format!(
                    "inline predicate is an exact copy of [queries.{}] — the copy will not follow the original",
                    name
                )
            } else {
                format!(
                    "inline predicate is {:.0}% identical to [queries.{}] but not the same — one of them has already drifted",
                    score * 100.0,
                    name
                )
            };
            findings.push(
                LintFinding::warning("L6", location.clone(), message)
                    .with_hint(format!("replace the inline sql with query = \"{}\"", name)),
            );
            break;
        }
    }

    // 2. Inline predicate vs. inline predicate.
    for i in 0..inline.len() {
        for j in (i + 1)..inline.len() {
            // Both already pointed at the same named query — same fix, said once.
            if matched_named[i].is_some() && matched_named[i] == matched_named[j] {
                continue;
            }
            let score = similarity::similarity(&normalized[i], &normalized[j]);
            if score < threshold {
                continue;
            }
            let message = if normalized[i] == normalized[j] {
                format!(
                    "predicate is duplicated verbatim at {} — two copies of one fact",
                    inline[j].0
                )
            } else {
                format!(
                    "predicate is {:.0}% identical to the one at {} but not the same — they decide the same fact two ways",
                    score * 100.0,
                    inline[j].0
                )
            };
            findings.push(
                LintFinding::warning("L6", inline[i].0.clone(), message).with_hint(
                    "declare it once as [queries.<name>] and reference it from both gates with query = \"<name>\"",
                ),
            );
        }
    }

    findings
}

// [check-l7]
/// L7 — evidence must be at least as strong as the gate relying on it.
///
/// A transition can declare how much it trusts its own evidence:
///
/// ```toml
/// [transitions.integrity]
/// requires_attestation = "host"
/// ```
///
/// and each event declares how strong it is (`attestation = "host"`). Both are
/// positions in the consumer's `[attestation] levels` ordering; the engine
/// compares them and knows nothing else about them. If a gate demands
/// host-level proof but is satisfied by an event the agent can write itself,
/// the gate enforces nothing — it reads as a strong check and behaves as a
/// weak one, which is worse than no check at all because it stops anyone
/// looking. That is holtz #79.
///
/// A gate may carry `requires_attestation` directly, overriding its
/// transition's requirement for that gate alone.
///
/// Silent unless the protocol declares an ordering: with no `[attestation]`
/// section there is nothing to compare, and the check has no opinion.
pub fn l7_forgeable_evidence(analysis: &Analysis) -> Vec<LintFinding> {
    let config = analysis.config;
    let mut findings = Vec::new();

    if config.attestation.is_empty() {
        return findings;
    }

    for (idx, t) in config.transitions.iter().enumerate() {
        let transition_level = t
            .integrity
            .as_ref()
            .and_then(|i| i.requires_attestation.as_deref());
        let location = analysis.transition_location(idx);

        // Undeclared level names are reported once per transition rather than
        // once per leaf that inherits them.
        let mut levels_in_force: Vec<&str> = Vec::new();
        if let Some(level) = transition_level {
            levels_in_force.push(level);
        }
        for gate in &t.gates {
            collect_required_levels(gate, &mut levels_in_force);
        }
        levels_in_force.sort_unstable();
        levels_in_force.dedup();
        for level in &levels_in_force {
            if config.attestation.rank(level).is_none() {
                findings.push(
                    LintFinding::error(
                        "L7",
                        location.clone(),
                        format!(
                            "requires_attestation '{}' is not one of [attestation] levels ({})",
                            level,
                            config.attestation.levels.join(", ")
                        ),
                    )
                    .with_hint("use a declared level, or add it to [attestation] levels"),
                );
            }
        }

        for gate in &t.gates {
            collect_attested_refs(
                gate,
                config,
                transition_level,
                true,
                &location,
                &mut findings,
            );
        }
    }

    findings
}

/// Every `requires_attestation` value named anywhere in a gate tree.
fn collect_required_levels<'a>(gate: &'a GateConfig, out: &mut Vec<&'a str>) {
    if let Some(level) = gate
        .params
        .get("requires_attestation")
        .and_then(|v| v.as_str())
    {
        out.push(level);
    }
    for child in &gate.gates {
        collect_required_levels(child, out);
    }
}

/// Walk a gate tree comparing each required event's attestation against the
/// level in force (the gate's own `requires_attestation`, else the one
/// inherited from its parent gate or the transition).
///
/// Composite gates only recurse — their leaves carry the event references —
/// and `not` flips polarity, since an event a gate requires to be *absent*
/// carries no evidentiary weight.
fn collect_attested_refs(
    gate: &GateConfig,
    config: &crate::config::ProtocolConfig,
    inherited: Option<&str>,
    positive: bool,
    location: &str,
    findings: &mut Vec<LintFinding>,
) {
    let required = gate
        .params
        .get("requires_attestation")
        .and_then(|v| v.as_str())
        .or(inherited);

    if matches!(
        gate.gate_type.as_str(),
        "any_of" | "all_of" | "not" | "k_of_n"
    ) {
        let child_positive = if gate.gate_type == "not" {
            !positive
        } else {
            positive
        };
        for child in &gate.gates {
            collect_attested_refs(child, config, required, child_positive, location, findings);
        }
        return;
    }

    if !positive {
        return;
    }
    let Some(required) = required else {
        return;
    };
    // Undeclared levels are reported once per transition by the caller.
    let Some(required_rank) = config.attestation.rank(required) else {
        return;
    };

    for r in gate_event_refs(gate, config) {
        if !r.required || is_engine_event(&r.event) {
            continue;
        }
        let Some(event) = config.events.get(&r.event) else {
            continue;
        };

        match event.attestation.as_deref() {
            None => findings.push(
                LintFinding::warning(
                    "L7",
                    location.to_string(),
                    format!(
                        "gate requires attestation '{}' but event '{}' declares none — its strength is unknown",
                        required, r.event
                    ),
                )
                .with_hint(format!(
                    "declare attestation on [events.{}] so the requirement can be checked",
                    r.event
                )),
            ),
            Some(level) => match config.attestation.rank(level) {
                None => findings.push(
                    LintFinding::error(
                        "L7",
                        location.to_string(),
                        format!(
                            "event '{}' declares attestation '{}', which is not one of [attestation] levels ({})",
                            r.event,
                            level,
                            config.attestation.levels.join(", ")
                        ),
                    )
                    .with_hint("use a declared level, or add it to [attestation] levels"),
                ),
                Some(rank) if rank < required_rank => findings.push(
                    LintFinding::error(
                        "L7",
                        location.to_string(),
                        format!(
                            "gate requires attestation '{}' but event '{}' only supplies '{}' — the gate reads as a strong check and enforces a weak one",
                            required, r.event, level
                        ),
                    )
                    .with_hint(format!(
                        "raise [events.{}] to '{}' and make its producer supply that, or lower the requirement",
                        r.event, required
                    )),
                ),
                Some(_) => {}
            },
        }
    }
}

// [inline-predicates]
/// Every query gate carrying inline `sql`, paired with a human-readable
/// location. Gates that reference a named query are already single-sourced and
/// are skipped.
fn inline_query_predicates(config: &crate::config::ProtocolConfig) -> Vec<(String, String)> {
    fn walk(gate: &GateConfig, location: &str, out: &mut Vec<(String, String)>) {
        if gate.gate_type == "query" {
            if let Some(sql) = gate.params.get("sql").and_then(|v| v.as_str()) {
                out.push((location.to_string(), sql.to_string()));
            }
        }
        for child in &gate.gates {
            walk(child, location, out);
        }
    }

    let mut out = Vec::new();
    for t in &config.transitions {
        let location = format!(
            "transitions.toml: transition '{}' ({} \u{2192} {})",
            t.command, t.from, t.to
        );
        for gate in &t.gates {
            walk(gate, &location, &mut out);
        }
    }
    for (idx, hook) in config.hooks.iter().enumerate() {
        let location = format!("hooks.toml: hook[{}]", idx);
        if let Some(ref gate) = hook.gate {
            walk(gate, &location, &mut out);
        }
        if let Some(ref check) = hook.check {
            if let Some(ref sql) = check.sql {
                out.push((location, sql.clone()));
            }
        }
    }
    out
}
