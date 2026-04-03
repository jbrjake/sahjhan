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
    status_cache_path, track_ledger_in_manifest, LedgerTargeting, EXIT_CONFIG_ERROR,
    EXIT_INTEGRITY_ERROR, EXIT_SUCCESS, EXIT_USAGE_ERROR,
};
use super::output::{
    CommandOutput, CommandResult, EventOnlyStatusData, GateResultData, MemberData, SetSummaryData,
    StatusData, TransitionSummaryData,
};

// ---------------------------------------------------------------------------
// status
// ---------------------------------------------------------------------------

// [cmd-status]
pub fn cmd_status(config_dir: &str, targeting: &LedgerTargeting) -> Box<dyn CommandOutput> {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            return Box::new(CommandResult::<StatusData>::err(
                "status",
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
            } else if code == EXIT_CONFIG_ERROR {
                "config_error"
            } else {
                "usage_error"
            };
            return Box::new(CommandResult::<StatusData>::err(
                "status", code, error_code, msg,
            ));
        }
    };

    // Event-only ledger: terse single-line output
    if let Some(LedgerMode::EventOnly) = mode {
        let event_count = ledger.len();
        let (chain_valid, chain_error) = match ledger.verify() {
            Ok(()) => (true, None),
            Err(e) => (false, Some(e.to_string())),
        };
        return Box::new(CommandResult::ok(
            "status",
            EventOnlyStatusData {
                event_count,
                chain_valid,
                chain_error,
            },
        ));
    }

    let _data_dir = resolve_data_dir(&config.paths.data_dir);
    let _manifest = match load_manifest(&_data_dir) {
        Ok(m) => m,
        Err((code, msg)) => {
            return Box::new(CommandResult::<StatusData>::err(
                "status",
                code,
                "integrity_error",
                msg,
            ));
        }
    };

    let machine = StateMachine::new(&config, ledger);
    let current_state = machine.current_state().to_string();

    let event_count = machine.ledger().len();
    let (chain_valid, chain_error) = match machine.ledger().verify() {
        Ok(()) => (true, None),
        Err(e) => (false, Some(e.to_string())),
    };

    // Build sets summary
    let sets: Vec<SetSummaryData> = config
        .sets
        .keys()
        .map(|set_name| {
            let set_status = machine.set_status(set_name);
            let members: Vec<MemberData> = set_status
                .members
                .iter()
                .map(|m| MemberData {
                    name: m.name.clone(),
                    done: m.done,
                })
                .collect();
            SetSummaryData {
                name: set_name.clone(),
                completed: set_status.completed,
                total: set_status.total,
                members,
            }
        })
        .collect();

    // Build transitions summary
    let available_transitions: Vec<_> = config
        .transitions
        .iter()
        .filter(|t| t.from == current_state)
        .collect();

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let transitions: Vec<TransitionSummaryData> = available_transitions
        .iter()
        .map(|transition| {
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

            TransitionSummaryData {
                command: transition.command.clone(),
                from: transition.from.clone(),
                to: transition.to.clone(),
                ready: all_passed,
                gates,
            }
        })
        .collect();

    // Self-diagnostics: check for status cache
    let mut warnings: Vec<String> = Vec::new();
    if !status_cache_path(&_data_dir).exists() {
        warnings.push(
            "status-cache.json not found \u{2014} enforcement hooks may be inactive".to_string(),
        );
    }

    Box::new(CommandResult::ok(
        "status",
        StatusData {
            state: current_state,
            event_count,
            chain_valid,
            chain_error,
            warnings,
            sets,
            transitions,
        },
    ))
}

// ---------------------------------------------------------------------------
// set status
// ---------------------------------------------------------------------------

// [cmd-set-status]
pub fn cmd_set_status(
    config_dir: &str,
    set_name: &str,
    targeting: &LedgerTargeting,
) -> Box<dyn CommandOutput> {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            return Box::new(CommandResult::<SetSummaryData>::err(
                "set_status",
                code,
                "config_error",
                msg,
            ));
        }
    };

    if !config.sets.contains_key(set_name) {
        return Box::new(CommandResult::<SetSummaryData>::err(
            "set_status",
            EXIT_USAGE_ERROR,
            "usage_error",
            format!("unknown set '{}'", set_name),
        ));
    }

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting, &config_path) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            let error_code = if code == EXIT_INTEGRITY_ERROR {
                "integrity_error"
            } else {
                "config_error"
            };
            return Box::new(CommandResult::<SetSummaryData>::err(
                "set_status",
                code,
                error_code,
                msg,
            ));
        }
    };

    let machine = StateMachine::new(&config, ledger);
    let status = machine.set_status(set_name);

    let members: Vec<MemberData> = status
        .members
        .iter()
        .map(|m| MemberData {
            name: m.name.clone(),
            done: m.done,
        })
        .collect();

    Box::new(CommandResult::ok(
        "set_status",
        SetSummaryData {
            name: set_name.to_string(),
            completed: status.completed,
            total: status.total,
            members,
        },
    ))
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

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting, &config_path) {
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
