// src/cli/transition.rs
//
// Transition, gate check, and event recording commands.
//
// ## Index
// - [cmd-transition] cmd_transition() — execute a named transition (runs gates); handles GateBlocked + AllCandidatesBlocked
// - [cmd-gate-check] cmd_gate_check() — dry-run gate evaluation; multi-candidate aware
// - [record-and-render] record_and_render() — shared event recording + render triggering logic
// - validate_event_fields() — validate event fields against an EventConfig (shared by cmd_event + cmd_authed_event)
// - [cmd-event] cmd_event() — record a protocol event

use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::events::EventConfig;
use crate::gates::evaluator::{evaluate_gates, GateContext};
use crate::render::engine::RenderEngine;
use crate::state::machine::StateMachine;

use super::commands::{
    build_state_params, guard_event_only, load_config, load_manifest, open_targeted_ledger,
    resolve_config_dir, resolve_data_dir, save_manifest, track_ledger_in_manifest,
    write_status_cache, LedgerTargeting, EXIT_GATE_FAILED, EXIT_INTEGRITY_ERROR, EXIT_SUCCESS,
    EXIT_USAGE_ERROR,
};
use super::output::{CandidateData, CommandOutput, CommandResult, GateCheckData, GateResultData};

// ---------------------------------------------------------------------------
// transition
// ---------------------------------------------------------------------------

// [cmd-transition]
pub fn cmd_transition(
    config_dir: &str,
    name: &str,
    args: &[String],
    targeting: &LedgerTargeting,
) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let (ledger, mode) = match open_targeted_ledger(&config, targeting, &config_path) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    // Guard: event-only ledgers cannot transition
    if let Err((code, msg)) = guard_event_only(&mode, "execute a transition") {
        eprintln!("{}", msg);
        return code;
    }

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let mut manifest = match load_manifest(&data_dir) {
        Ok(m) => m,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let mut machine = StateMachine::new(&config, ledger);

    match machine.transition(name, args) {
        Ok(outcome) => {
            // Update manifest with ledger
            if let Err((code, msg)) =
                track_ledger_in_manifest(&mut manifest, &data_dir, machine.ledger())
            {
                eprintln!("{}", msg);
                return code;
            }
            if let Err((code, msg)) = save_manifest(&mut manifest, &data_dir) {
                eprintln!("{}", msg);
                return code;
            }

            let mut render_count = 0usize;

            // Trigger on_transition renders
            if !config.renders.is_empty() {
                let registry_path = super::commands::registry_path_from_config(&config);
                if let Ok(engine) = RenderEngine::new(&config, &config_path) {
                    let mut engine = engine.with_registry(registry_path);
                    if let Some(ref name) = targeting.ledger_name {
                        engine = engine.with_active_ledger_name(name.clone());
                    }
                    let render_dir = resolve_data_dir(&config.paths.render_dir);
                    let ledger_seq = machine
                        .ledger()
                        .entries()
                        .last()
                        .map(|e| e.seq)
                        .unwrap_or(0);
                    match engine.render_triggered(
                        "on_transition",
                        None,
                        machine.ledger(),
                        &render_dir,
                        &mut manifest,
                        ledger_seq,
                    ) {
                        Ok(rendered) => {
                            render_count = rendered.len();
                            if !rendered.is_empty() {
                                let _ = save_manifest(&mut manifest, &data_dir);
                            }
                        }
                        Err(e) => {
                            eprintln!("error: render: {}", e);
                        }
                    }
                }
            }

            // Update status cache with new state
            write_status_cache(&data_dir, &config, &config_path, &outcome.to);

            if render_count > 0 {
                println!(
                    "{} \u{2192} {} ({} rendered)",
                    outcome.from, outcome.to, render_count
                );
            } else {
                println!("{} \u{2192} {}", outcome.from, outcome.to);
            }

            EXIT_SUCCESS
        }
        Err(crate::state::machine::StateError::GateBlocked { gate_type, reason }) => {
            eprintln!("\u{2717} {}: {}", gate_type, reason);
            EXIT_GATE_FAILED
        }
        Err(crate::state::machine::StateError::AllCandidatesBlocked {
            command: _,
            state: _,
            candidates,
        }) => {
            for (target, gate_type, reason) in &candidates {
                eprintln!(
                    "\u{2717} \u{2192} {} blocked by {}: {}",
                    target, gate_type, reason
                );
            }
            EXIT_GATE_FAILED
        }
        Err(crate::state::machine::StateError::NoTransition { command, state }) => {
            eprintln!("error: no transition '{}' from state '{}'", command, state);
            EXIT_USAGE_ERROR
        }
        Err(e) => {
            eprintln!("error: transition failed: {}", e);
            EXIT_INTEGRITY_ERROR
        }
    }
}

// ---------------------------------------------------------------------------
// gate check (dry-run)
// ---------------------------------------------------------------------------

// [cmd-gate-check]
pub fn cmd_gate_check(
    config_dir: &str,
    transition_name: &str,
    args: &[String],
    targeting: &LedgerTargeting,
) -> Box<dyn CommandOutput> {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            return Box::new(CommandResult::<GateCheckData>::err(
                "gate_check",
                code,
                "config_error",
                msg,
            ));
        }
    };

    let (ledger, mode) = match open_targeted_ledger(&config, targeting, &config_path) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            let error_code = if code == EXIT_INTEGRITY_ERROR {
                "integrity_error"
            } else {
                "config_error"
            };
            return Box::new(CommandResult::<GateCheckData>::err(
                "gate_check",
                code,
                error_code,
                msg,
            ));
        }
    };

    // Guard: event-only ledgers cannot check gates
    if let Err((code, msg)) = guard_event_only(&mode, "check gates") {
        return Box::new(CommandResult::<GateCheckData>::err(
            "gate_check",
            code,
            "usage_error",
            msg,
        ));
    }

    let machine = StateMachine::new(&config, ledger);
    let current_state = machine.current_state().to_string();

    // Collect ALL candidates matching this command + current state.
    let candidates: Vec<_> = config
        .transitions
        .iter()
        .filter(|t| t.command == transition_name && t.from == current_state)
        .cloned()
        .collect();

    if candidates.is_empty() {
        return Box::new(CommandResult::<GateCheckData>::err(
            "gate_check",
            EXIT_USAGE_ERROR,
            "usage_error",
            format!(
                "no transition '{}' from state '{}'",
                transition_name, current_state
            ),
        ));
    }

    let multi = candidates.len() > 1;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let mut would_take: Option<String> = None;
    let mut candidate_data: Vec<CandidateData> = Vec::new();

    for transition in candidates.iter() {
        if transition.gates.is_empty() {
            if would_take.is_none() {
                would_take = Some(transition.to.clone());
            }
            candidate_data.push(CandidateData {
                from: transition.from.clone(),
                to: transition.to.clone(),
                gates: vec![],
                all_passed: true,
            });
            // Single candidate with no gates: short-circuit
            if !multi {
                let data = GateCheckData {
                    transition: transition_name.to_string(),
                    current_state: current_state.clone(),
                    candidates: candidate_data,
                    result: "ready (no gates)".to_string(),
                    would_take,
                };
                return Box::new(CommandResult::ok("gate_check", data));
            }
            continue;
        }

        let mut state_params = build_state_params(&config, &transition.to, machine.ledger());

        // Map CLI args into state_params.
        // - key=value args are inserted directly (override state params)
        // - Positional args (no '=') are mapped to declared transition.args names
        let mut positional_idx = 0;
        for arg in args {
            if let Some((key, value)) = arg.split_once('=') {
                state_params.insert(key.to_string(), value.to_string());
            } else if positional_idx < transition.args.len() {
                state_params.insert(transition.args[positional_idx].clone(), arg.clone());
                positional_idx += 1;
            }
        }

        let ctx = GateContext {
            ledger: machine.ledger(),
            config: &config,
            current_state: &current_state,
            state_params,
            working_dir: cwd.clone(),
            event_fields: None,
        };

        let results = evaluate_gates(&transition.gates, &ctx);
        let all_passed = results.iter().all(|r| r.passed);

        let gates: Vec<GateResultData> = results
            .iter()
            .map(|r| GateResultData {
                passed: r.passed,
                evaluable: r.evaluable,
                gate_type: r.gate_type.clone(),
                description: r.description.clone(),
                reason: r.reason.clone(),
                intent: r.intent.clone(),
            })
            .collect();

        if all_passed && would_take.is_none() {
            would_take = Some(transition.to.clone());
        }

        candidate_data.push(CandidateData {
            from: transition.from.clone(),
            to: transition.to.clone(),
            gates,
            all_passed,
        });
    }

    let result_str = if multi {
        if let Some(ref target) = would_take {
            format!("would take \u{2192} {}", target)
        } else {
            "blocked".to_string()
        }
    } else if would_take.is_some() {
        "ready".to_string()
    } else {
        "blocked".to_string()
    };

    let data = GateCheckData {
        transition: transition_name.to_string(),
        current_state,
        candidates: candidate_data,
        result: result_str,
        would_take,
    };

    Box::new(CommandResult::ok("gate_check", data))
    // dry-run always returns 0
}

// ---------------------------------------------------------------------------
// shared event recording + render triggering
// ---------------------------------------------------------------------------

// [record-and-render]
/// Shared event recording + render triggering logic.
/// Used by both cmd_event and cmd_authed_event after their respective validations.
#[allow(clippy::too_many_arguments)]
pub fn record_and_render(
    config: &crate::config::ProtocolConfig,
    config_path: &std::path::Path,
    machine: &mut StateMachine,
    manifest: &mut crate::manifest::tracker::Manifest,
    data_dir: &std::path::Path,
    event_type: &str,
    fields: HashMap<String, String>,
    targeting: &LedgerTargeting,
) -> i32 {
    match machine.record_event(event_type, fields) {
        Ok(()) => {
            if let Err((code, msg)) = track_ledger_in_manifest(manifest, data_dir, machine.ledger())
            {
                eprintln!("{}", msg);
                return code;
            }
            if let Err((code, msg)) = save_manifest(manifest, data_dir) {
                eprintln!("{}", msg);
                return code;
            }

            let mut render_count = 0usize;

            if !config.renders.is_empty() {
                let registry_path = super::commands::registry_path_from_config(config);
                if let Ok(engine) = RenderEngine::new(config, config_path) {
                    let mut engine = engine.with_registry(registry_path);
                    if let Some(ref name) = targeting.ledger_name {
                        engine = engine.with_active_ledger_name(name.clone());
                    }
                    let render_dir = resolve_data_dir(&config.paths.render_dir);
                    let ledger_seq = machine
                        .ledger()
                        .entries()
                        .last()
                        .map(|e| e.seq)
                        .unwrap_or(0);
                    match engine.render_triggered(
                        "on_event",
                        Some(event_type),
                        machine.ledger(),
                        &render_dir,
                        manifest,
                        ledger_seq,
                    ) {
                        Ok(rendered) => {
                            render_count = rendered.len();
                            if !rendered.is_empty() {
                                let _ = save_manifest(manifest, data_dir);
                            }
                        }
                        Err(e) => {
                            eprintln!("error: render: {}", e);
                        }
                    }
                }
            }

            if render_count > 0 {
                println!("recorded: {} ({} rendered)", event_type, render_count);
            } else {
                println!("recorded: {}", event_type);
            }

            EXIT_SUCCESS
        }
        Err(e) => {
            eprintln!("error: cannot record event: {}", e);
            EXIT_INTEGRITY_ERROR
        }
    }
}

// ---------------------------------------------------------------------------
// event
// ---------------------------------------------------------------------------

/// Validate event fields against an event definition.
///
/// Checks that required fields are present, and validates pattern/values
/// constraints on all provided fields (including optional ones).
pub fn validate_event_fields(
    event_config: &EventConfig,
    fields: &HashMap<String, String>,
    event_type: &str,
) -> Result<(), (i32, String)> {
    // Check required fields are present (skip optional fields)
    for field_def in &event_config.fields {
        if !field_def.optional && !fields.contains_key(&field_def.name) {
            return Err((
                EXIT_USAGE_ERROR,
                format!(
                    "error: missing field '{}' for event '{}'",
                    field_def.name, event_type
                ),
            ));
        }
    }

    // Validate provided field values against patterns and allowed values
    for field_def in &event_config.fields {
        if let Some(value) = fields.get(&field_def.name) {
            if let Some(pattern) = &field_def.pattern {
                if let Ok(re) = regex::Regex::new(pattern) {
                    if !re.is_match(value) {
                        return Err((
                            EXIT_USAGE_ERROR,
                            format!(
                                "error: field '{}' value '{}' doesn't match pattern '{}'",
                                field_def.name, value, pattern
                            ),
                        ));
                    }
                }
            }
            if let Some(allowed) = &field_def.values {
                if !allowed.contains(value) {
                    return Err((
                        EXIT_USAGE_ERROR,
                        format!(
                            "error: field '{}' value '{}' not in allowed values {:?}",
                            field_def.name, value, allowed
                        ),
                    ));
                }
            }
        }
    }

    Ok(())
}

// [cmd-event]
pub fn cmd_event(
    config_dir: &str,
    event_type: &str,
    field_strs: &[String],
    targeting: &LedgerTargeting,
) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting, &config_path) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    // Check if event type is restricted
    if let Some(event_config) = config.events.get(event_type) {
        if event_config.restricted == Some(true) {
            eprintln!(
                "error: event type '{}' is restricted. Use 'sahjhan authed-event' with a valid proof.",
                event_type
            );
            return EXIT_USAGE_ERROR;
        }
    }

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let mut manifest = match load_manifest(&data_dir) {
        Ok(m) => m,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    // Parse --field key=value pairs
    let mut fields: HashMap<String, String> = HashMap::new();
    for f in field_strs {
        if let Some((key, value)) = f.split_once('=') {
            fields.insert(key.to_string(), value.to_string());
        } else {
            eprintln!("error: invalid field '{}': expected key=value", f);
            return EXIT_USAGE_ERROR;
        }
    }

    // Validate fields against events.toml definitions (E11)
    if let Some(event_config) = config.events.get(event_type) {
        if let Err((code, msg)) = validate_event_fields(event_config, &fields, event_type) {
            eprintln!("{}", msg);
            return code;
        }
    }

    let mut machine = StateMachine::new(&config, ledger);

    record_and_render(
        &config,
        &config_path,
        &mut machine,
        &mut manifest,
        &data_dir,
        event_type,
        fields,
        targeting,
    )
}
