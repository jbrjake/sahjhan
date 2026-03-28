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
use crate::manifest::verify as manifest_verify;
use crate::render::engine::RenderEngine;
use crate::state::machine::StateMachine;

use super::commands::{
    build_state_params, load_config, load_manifest, open_targeted_ledger, resolve_config_dir,
    resolve_data_dir, save_manifest, track_ledger_in_manifest, LedgerTargeting,
    EXIT_INTEGRITY_ERROR, EXIT_SUCCESS, EXIT_USAGE_ERROR,
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

    // Event-only ledger: show metadata without state machine fields
    if let Some(LedgerMode::EventOnly) = mode {
        let ledger_len = ledger.len();
        let chain_status = match ledger.verify() {
            Ok(()) => "valid".to_string(),
            Err(e) => format!("INVALID ({})", e),
        };
        let last_ts = ledger
            .entries()
            .last()
            .map(|e| e.ts.as_str())
            .unwrap_or("none");

        let width = 59;
        let bar = "=".repeat(width);
        println!("{}", bar);
        println!("  sahjhan · event-only ledger");
        println!("{}", bar);
        println!();
        println!("  Mode:      event-only");
        println!("  Events:    {}", ledger_len);
        println!("  Chain:     {}", chain_status);
        println!("  Last:      {}", last_ts);
        println!();
        println!("{}", bar);
        return EXIT_SUCCESS;
    }

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let manifest = match load_manifest(&data_dir) {
        Ok(m) => m,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let machine = StateMachine::new(&config, ledger);
    let current_state = machine.current_state().to_string();

    // Ledger status
    let ledger_len = machine.ledger().len();
    let chain_status = match machine.ledger().verify() {
        Ok(()) => "valid".to_string(),
        Err(e) => format!("INVALID ({})", e),
    };
    let violations = machine.ledger().events_of_type("protocol_violation").len();
    let violation_str = if violations == 0 {
        "clean".to_string()
    } else {
        format!("{} violation(s)", violations)
    };

    // Manifest status
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let manifest_files = manifest.entries.len();
    let verify_result = manifest_verify::verify(&manifest, &cwd);
    let manifest_status = if verify_result.clean {
        "clean".to_string()
    } else {
        format!("{} modified", verify_result.mismatches.len())
    };

    // State label
    let state_label = config
        .states
        .get(&current_state)
        .map(|s| s.label.as_str())
        .unwrap_or(&current_state);

    // Run number = count of protocol_init events (should be 1 normally)
    let run_number = machine.ledger().events_of_type("protocol_init").len();

    // Header
    let width = 59;
    let bar = "=".repeat(width);
    println!("{}", bar);
    println!(
        "  sahjhan · {} v{} · Run {}",
        config.protocol.name, config.protocol.version, run_number
    );
    println!("{}", bar);
    println!();
    println!("  State:     {} ({})", state_label, current_state);
    println!(
        "  Ledger:    {} events, chain {}, {}",
        ledger_len, chain_status, violation_str
    );
    println!(
        "  Manifest:  {} files tracked, {}",
        manifest_files, manifest_status
    );

    // Sets
    for set_name in config.sets.keys() {
        let set_status = machine.set_status(set_name);
        println!();
        println!(
            "  Set: {} ({}/{} complete)",
            set_name, set_status.completed, set_status.total
        );
        for member in &set_status.members {
            let marker = if member.done { "✓" } else { "·" };
            println!("    {} {}", marker, member.name);
        }
    }

    // Next available transition and its gates
    let available_transitions: Vec<_> = config
        .transitions
        .iter()
        .filter(|t| t.from == current_state)
        .collect();

    for transition in &available_transitions {
        if !transition.gates.is_empty() {
            let state_params = build_state_params(&config, &transition.to, machine.ledger());
            let ctx = GateContext {
                ledger: machine.ledger(),
                config: &config,
                current_state: &current_state,
                state_params,
                working_dir: cwd.clone(),
                event_fields: None,
            };

            let results = evaluate_gates(&transition.gates, &ctx);
            println!();
            println!("  Next gate ({}):", transition.command);
            for result in &results {
                let marker = if result.passed { "✓" } else { "✗" };
                let extra = if result.passed {
                    String::new()
                } else {
                    format!(" ({})", result.reason.as_deref().unwrap_or("failed"))
                };
                println!("    {} {}{}", marker, result.description, extra);
            }
        }
    }

    println!();
    if current_state == config.initial_state().unwrap_or("idle") {
        println!("  Awaiting first transition.");
    }
    println!();
    println!("{}", bar);

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
            eprintln!("{}", msg);
            return code;
        }
    };

    if !config.sets.contains_key(set_name) {
        eprintln!(
            "Unknown set '{}'. I know every set in this protocol. That one isn't among them.",
            set_name
        );
        return EXIT_USAGE_ERROR;
    }

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let machine = StateMachine::new(&config, ledger);
    let status = machine.set_status(set_name);

    println!(
        "Set: {} ({}/{} complete)",
        set_name, status.completed, status.total
    );
    for member in &status.members {
        let marker = if member.done { "✓" } else { "·" };
        println!("  {} {}", marker, member.name);
    }

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
            eprintln!("{}", msg);
            return code;
        }
    };

    // Validate set exists
    let set_config = match config.sets.get(set_name) {
        Some(s) => s,
        None => {
            eprintln!(
                "Unknown set '{}'. I know every set in this protocol. That one isn't among them.",
                set_name
            );
            return EXIT_USAGE_ERROR;
        }
    };

    // Validate member exists in set
    if !set_config.values.contains(&member.to_string()) {
        eprintln!(
            "Unknown member '{}' in set '{}'. The valid members are: {}",
            member,
            set_name,
            set_config.values.join(", ")
        );
        return EXIT_USAGE_ERROR;
    }

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let mut manifest = match load_manifest(&data_dir) {
        Ok(m) => m,
        Err((code, msg)) => {
            eprintln!("{}", msg);
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
                eprintln!("{}", msg);
                return code;
            }
            if let Err((code, msg)) = save_manifest(&mut manifest, &data_dir) {
                eprintln!("{}", msg);
                return code;
            }

            let status = machine.set_status(set_name);
            println!(
                "Set {}: {} complete ({}/{}).",
                set_name, member, status.completed, status.total
            );

            // Trigger on_event renders for set_member_complete
            if !config.renders.is_empty() {
                let registry_path = super::commands::registry_path_from_config(&config);
                if let Ok(engine) = RenderEngine::new(&config, &config_path) {
                    let engine = engine.with_registry(registry_path);
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
                            for target in &rendered {
                                println!("  Rendered: {}", target);
                            }
                            if !rendered.is_empty() {
                                let _ = save_manifest(&mut manifest, &data_dir);
                            }
                        }
                        Err(e) => {
                            eprintln!("  Render warning: {}", e);
                        }
                    }
                }
            }

            EXIT_SUCCESS
        }
        Err(e) => {
            eprintln!("Cannot record set completion: {}", e);
            EXIT_INTEGRITY_ERROR
        }
    }
}
