// src/cli/status.rs
//
// Status display and set management commands.
//
// ## Index
// - [cmd-status] cmd_status() — show current state, progress, gate status
// - [cmd-set-status] cmd_set_status() — show completion status for a set
// - [cmd-set-complete] cmd_set_complete() — record member completion (runs gates)

use std::collections::HashMap;
use std::path::PathBuf;

use crate::gates::evaluator::{evaluate_gates, GateContext};
use crate::ledger::registry::LedgerMode;
use crate::render::engine::RenderEngine;
use crate::state::machine::StateMachine;

use super::commands::{
    build_state_params, load_config, load_manifest, open_targeted_ledger,
    registry_path_from_config, resolve_config_dir, resolve_data_dir, save_manifest,
    track_ledger_in_manifest, LedgerTargeting, EXIT_INTEGRITY_ERROR, EXIT_SUCCESS,
    EXIT_USAGE_ERROR,
};

// ---------------------------------------------------------------------------
// status
// ---------------------------------------------------------------------------

// [cmd-status]
pub fn cmd_status(config_dir: &str, targeting: &LedgerTargeting) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("error: {}", msg);
            return code;
        }
    };

    let (ledger, mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("error: {}", msg);
            return code;
        }
    };

    // Event-only ledger: terse single-line output
    if let Some(LedgerMode::EventOnly) = mode {
        let ledger_len = ledger.len();
        let chain_status = match ledger.verify() {
            Ok(()) => "chain valid".to_string(),
            Err(e) => format!("chain INVALID ({})", e),
        };
        println!("event-only: {} events, {}", ledger_len, chain_status);
        return EXIT_SUCCESS;
    }

    let _data_dir = resolve_data_dir(&config.paths.data_dir);
    let _manifest = match load_manifest(&_data_dir) {
        Ok(m) => m,
        Err((code, msg)) => {
            eprintln!("error: {}", msg);
            return code;
        }
    };

    let machine = StateMachine::new(&config, ledger);
    let current_state = machine.current_state().to_string();

    let ledger_len = machine.ledger().len();
    let chain_status = match machine.ledger().verify() {
        Ok(()) => "chain valid".to_string(),
        Err(e) => format!("chain INVALID ({})", e),
    };

    // Line 1: state: {current_state} ({event_count} events, {chain_status})
    println!(
        "state: {} ({} events, {})",
        current_state, ledger_len, chain_status
    );

    // Sets: one line each, only if there are sets
    if !config.sets.is_empty() {
        println!("sets:");
        for set_name in config.sets.keys() {
            let set_status = machine.set_status(set_name);
            let members_str: Vec<String> = set_status
                .members
                .iter()
                .map(|m| {
                    if m.done {
                        format!("\u{2713} {}", m.name)
                    } else {
                        format!("\u{00B7} {}", m.name)
                    }
                })
                .collect();
            println!(
                "  {}: {}/{} [{}]",
                set_name,
                set_status.completed,
                set_status.total,
                members_str.join(", ")
            );
        }
    }

    // Next transitions from current state
    let available_transitions: Vec<_> = config
        .transitions
        .iter()
        .filter(|t| t.from == current_state)
        .collect();

    if !available_transitions.is_empty() {
        println!("next:");
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        for transition in &available_transitions {
            let state_params = build_state_params(&config, &transition.to, machine.ledger());
            let ctx = GateContext {
                ledger: machine.ledger(),
                config: &config,
                current_state: &current_state,
                state_params,
                working_dir: cwd.clone(),
                event_fields: None,
            };

            let results = if transition.gates.is_empty() {
                vec![]
            } else {
                evaluate_gates(&transition.gates, &ctx)
            };

            let all_passed = results.iter().all(|r| r.passed);
            let readiness = if all_passed { "ready" } else { "blocked" };
            println!("  {}: {}", transition.command, readiness);

            for r in &results {
                if r.passed {
                    println!("    \u{2713} {}", r.description);
                } else {
                    let intent = r.intent.as_deref().unwrap_or("gate condition must be met");
                    println!(
                        "    \u{2717} {}: {} \u{2014} {}",
                        r.gate_type,
                        r.reason.as_deref().unwrap_or("failed"),
                        intent
                    );
                }
            }
        }
    }

    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------
// set status
// ---------------------------------------------------------------------------

// [cmd-set-status]
pub fn cmd_set_status(config_dir: &str, set_name: &str, targeting: &LedgerTargeting) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("error: {}", msg);
            return code;
        }
    };

    if !config.sets.contains_key(set_name) {
        eprintln!("error: unknown set '{}'", set_name);
        return EXIT_USAGE_ERROR;
    }

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("error: {}", msg);
            return code;
        }
    };

    let machine = StateMachine::new(&config, ledger);
    let status = machine.set_status(set_name);

    let members_str: Vec<String> = status
        .members
        .iter()
        .map(|m| {
            if m.done {
                format!("\u{2713} {}", m.name)
            } else {
                format!("\u{00B7} {}", m.name)
            }
        })
        .collect();

    println!(
        "{}: {}/{} [{}]",
        set_name,
        status.completed,
        status.total,
        members_str.join(", ")
    );

    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------
// set complete
// ---------------------------------------------------------------------------

// [cmd-set-complete]
pub fn cmd_set_complete(
    config_dir: &str,
    set_name: &str,
    member: &str,
    targeting: &LedgerTargeting,
) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("error: {}", msg);
            return code;
        }
    };

    // Validate set exists
    let set_config = match config.sets.get(set_name) {
        Some(s) => s,
        None => {
            eprintln!("error: unknown set '{}'", set_name);
            return EXIT_USAGE_ERROR;
        }
    };

    // Validate member exists in set
    if !set_config.values.contains(&member.to_string()) {
        eprintln!("error: unknown member '{}' in set '{}'", member, set_name);
        return EXIT_USAGE_ERROR;
    }

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("error: {}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let mut manifest = match load_manifest(&data_dir) {
        Ok(m) => m,
        Err((code, msg)) => {
            eprintln!("error: {}", msg);
            return code;
        }
    };

    let mut machine = StateMachine::new(&config, ledger);

    let mut fields = HashMap::new();
    fields.insert("set".to_string(), set_name.to_string());
    fields.insert("member".to_string(), member.to_string());

    match machine.record_event("set_member_complete", fields) {
        Ok(()) => {
            if let Err((code, msg)) =
                track_ledger_in_manifest(&mut manifest, &data_dir, machine.ledger())
            {
                eprintln!("error: {}", msg);
                return code;
            }
            if let Err((code, msg)) = save_manifest(&mut manifest, &data_dir) {
                eprintln!("error: {}", msg);
                return code;
            }

            // Trigger on_event renders BEFORE printing, so we have render_count
            let mut render_count = 0usize;
            if !config.renders.is_empty() {
                let registry_path = registry_path_from_config(&config);
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
                        Some("set_member_complete"),
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

            let status = machine.set_status(set_name);
            if render_count > 0 {
                println!(
                    "set {}: {} done ({}/{}, {} rendered)",
                    set_name, member, status.completed, status.total, render_count
                );
            } else {
                println!(
                    "set {}: {} done ({}/{})",
                    set_name, member, status.completed, status.total
                );
            }

            EXIT_SUCCESS
        }
        Err(e) => {
            eprintln!("error: {}", e);
            EXIT_INTEGRITY_ERROR
        }
    }
}
