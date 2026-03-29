// src/cli/transition.rs
//
// Transition, gate check, and event recording commands.
//
// ## Index
// - [cmd-transition] cmd_transition() — execute a named transition (runs gates)
// - [cmd-gate-check] cmd_gate_check() — dry-run gate evaluation
// - [cmd-event] cmd_event() — record a protocol event

use std::collections::HashMap;
use std::path::PathBuf;

use crate::gates::evaluator::{evaluate_gates, GateContext};
use crate::render::engine::RenderEngine;
use crate::state::machine::StateMachine;

use super::commands::{
    build_state_params, guard_event_only, load_config, load_manifest, open_targeted_ledger,
    resolve_config_dir, resolve_data_dir, save_manifest, track_ledger_in_manifest, LedgerTargeting,
    EXIT_GATE_FAILED, EXIT_INTEGRITY_ERROR, EXIT_SUCCESS, EXIT_USAGE_ERROR,
};

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

    let (ledger, mode) = match open_targeted_ledger(&config, targeting) {
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
    let from_state = machine.current_state().to_string();

    match machine.transition(name, args) {
        Ok(()) => {
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

            if render_count > 0 {
                println!(
                    "{} \u{2192} {} ({} rendered)",
                    from_state,
                    machine.current_state(),
                    render_count
                );
            } else {
                println!("{} \u{2192} {}", from_state, machine.current_state());
            }

            EXIT_SUCCESS
        }
        Err(crate::state::machine::StateError::GateBlocked { gate_type, reason }) => {
            eprintln!("\u{2717} {}: {}", gate_type, reason);
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
) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let (ledger, mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    // Guard: event-only ledgers cannot check gates
    if let Err((code, msg)) = guard_event_only(&mode, "check gates") {
        eprintln!("{}", msg);
        return code;
    }

    let machine = StateMachine::new(&config, ledger);
    let current_state = machine.current_state().to_string();

    // Find the transition
    let transition = match config
        .transitions
        .iter()
        .find(|t| t.command == transition_name && t.from == current_state)
    {
        Some(t) => t.clone(),
        None => {
            eprintln!(
                "error: no transition '{}' from state '{}'",
                transition_name, current_state
            );
            return EXIT_USAGE_ERROR;
        }
    };

    println!("gate-check: {}", transition_name);

    if transition.gates.is_empty() {
        println!("result: ready (no gates)");
        return EXIT_SUCCESS;
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

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let ctx = GateContext {
        ledger: machine.ledger(),
        config: &config,
        current_state: &current_state,
        state_params,
        working_dir: cwd,
        event_fields: None,
    };

    let results = evaluate_gates(&transition.gates, &ctx);
    let all_passed = results.iter().all(|r| r.passed);

    for result in &results {
        if result.passed {
            println!("  \u{2713} {}", result.description);
        } else {
            println!(
                "  \u{2717} {}: {} \u{2014} {}",
                result.gate_type,
                result.reason.as_deref().unwrap_or("failed"),
                result
                    .intent
                    .as_deref()
                    .unwrap_or("gate condition must be met")
            );
        }
    }

    if all_passed {
        println!("result: ready");
    } else {
        println!("result: blocked");
    }

    EXIT_SUCCESS // dry-run always returns 0
}

// ---------------------------------------------------------------------------
// event
// ---------------------------------------------------------------------------

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

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting) {
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
        // Check required fields are present
        for field_def in &event_config.fields {
            if !fields.contains_key(&field_def.name) {
                eprintln!(
                    "error: missing field '{}' for event '{}'",
                    field_def.name, event_type
                );
                return EXIT_USAGE_ERROR;
            }
        }

        // Validate field values against patterns if defined
        for field_def in &event_config.fields {
            if let Some(pattern) = &field_def.pattern {
                if let Some(value) = fields.get(&field_def.name) {
                    if let Ok(re) = regex::Regex::new(pattern) {
                        if !re.is_match(value) {
                            eprintln!(
                                "error: field '{}' value '{}' doesn't match pattern '{}'",
                                field_def.name, value, pattern
                            );
                            return EXIT_USAGE_ERROR;
                        }
                    }
                }
            }
            // Validate against allowed values if defined
            if let Some(allowed) = &field_def.values {
                if let Some(value) = fields.get(&field_def.name) {
                    if !allowed.contains(value) {
                        eprintln!(
                            "error: field '{}' value '{}' not in allowed values {:?}",
                            field_def.name, value, allowed
                        );
                        return EXIT_USAGE_ERROR;
                    }
                }
            }
        }
    }

    let mut machine = StateMachine::new(&config, ledger);

    match machine.record_event(event_type, fields) {
        Ok(()) => {
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

            // Trigger on_event renders
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
                        "on_event",
                        Some(event_type),
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
