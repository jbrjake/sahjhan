// src/cli/batch.rs
//
// Bulk transitions: apply one transition to every item a named query returns.
//
// ## Index
// - [cmd-batch]      cmd_batch()      — run a `[batches.<name>]` declaration
// - [parse-selector] parse_selector() — read the batch's own `--<param>` flag
// - [select-steps]   select_steps()   — the steps a `--<param>` value list turns on
// - [items-of]       items_of()       — ids returned by a step's named query
// - [apply-item]     apply_item()     — one item: gates outside the manifest lock, appends + seal inside

use std::path::Path;

use crate::config::{BatchConfig, BatchStep, ProtocolConfig};
use crate::state::machine::{StateError, StateMachine, TransitionOutcome};

use super::commands::{
    guard_event_only, load_config, load_manifest, lock_manifest, open_targeted_ledger,
    resolve_config_dir, resolve_data_dir, save_manifest, track_ledger_in_manifest,
    write_status_cache, LedgerTargeting, EXIT_GATE_FAILED, EXIT_INTEGRITY_ERROR, EXIT_SUCCESS,
    EXIT_USAGE_ERROR,
};

// [parse-selector]
/// Read the batch's own selector flag out of the trailing args.
///
/// The flag's name comes from the declaration (`param = "severity"` accepts
/// `--severity low,medium`), so the engine spells a consumer's vocabulary
/// without knowing any of it. Values are comma-separated; a repeated flag adds
/// to the list rather than replacing it.
fn parse_selector(batch: &BatchConfig, raw: &[String]) -> Result<Vec<String>, String> {
    let mut values: Vec<String> = Vec::new();
    let mut i = 0;
    while i < raw.len() {
        let arg = &raw[i];
        let Some(param) = batch.param.as_deref() else {
            return Err(format!(
                "takes no selector, but got '{}' — it declares no `param`",
                arg
            ));
        };
        let expected = format!("--{}", param);
        let value = if let Some(rest) = arg.strip_prefix(&format!("{}=", expected)) {
            i += 1;
            rest.to_string()
        } else if arg == &expected {
            i += 2;
            raw.get(i - 1)
                .ok_or_else(|| format!("{} needs a value", expected))?
                .clone()
        } else {
            return Err(format!(
                "unknown selector '{}' — this batch takes {}",
                arg, expected
            ));
        };
        values.extend(
            value
                .split(',')
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string),
        );
    }
    Ok(values)
}

// [select-steps]
/// The steps a parameter value list turns on, in declaration order.
///
/// A step with no `value` is unconditional. An empty selector runs the whole
/// batch — `sahjhan batch defer` means every step of it.
///
/// A value naming no step is an error rather than a silent no-op: the caller
/// asked for work that will not happen, and a batch that quietly defers
/// nothing looks exactly like one that had nothing to defer.
fn select_steps<'a>(
    steps: &'a [BatchStep],
    selected: &[String],
) -> Result<Vec<&'a BatchStep>, String> {
    if selected.is_empty() {
        return Ok(steps.iter().collect());
    }
    for value in selected {
        if !steps
            .iter()
            .any(|s| s.value.as_deref() == Some(value.as_str()))
        {
            return Err(format!("no step named '{}'", value));
        }
    }
    Ok(steps
        .iter()
        .filter(|s| match &s.value {
            None => true,
            Some(v) => selected.iter().any(|sel| sel == v),
        })
        .collect())
}

// [items-of]
/// Run a step's named query and return the `id` column of every row.
fn items_of(
    step: &BatchStep,
    config: &ProtocolConfig,
    ledger_path: &std::path::Path,
) -> Result<Vec<String>, String> {
    let query = config
        .queries
        .get(&step.items)
        .ok_or_else(|| format!("step references undeclared query '{}'", step.items))?;

    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|e| format!("failed to build tokio runtime: {}", e))?;

    let events = config.events.clone();
    let sql = query.sql.clone();
    let rows = rt
        .block_on(async {
            let engine = crate::query::QueryEngine::from_config(&events);
            engine.query_file(ledger_path, &sql).await
        })
        .map_err(|e| format!("query '{}' failed: {}", step.items, e))?;

    let mut ids = Vec::with_capacity(rows.len());
    for row in &rows {
        match row.get("id") {
            Some(id) if !id.is_empty() => ids.push(id.clone()),
            _ => {
                return Err(format!(
                    "query '{}' returned a row with no 'id' column — a batch \
                     applies a transition to ids, so the query has to name them",
                    step.items
                ))
            }
        }
    }
    Ok(ids)
}

// [apply-item]
/// One item's transition: its gates outside the manifest lock, then its
/// appends and the ledger's seal inside it, as `transition` does.
///
/// The outer `Err` is a manifest failure, which stops the batch; the inner
/// result is the transition's own, whose refusals do not.
fn apply_item(
    machine: &mut StateMachine,
    config: &ProtocolConfig,
    data_dir: &Path,
    transition: &str,
    id: &str,
) -> Result<Result<TransitionOutcome, StateError>, (i32, String)> {
    let prepared = match machine.prepare_transition(transition, &[id.to_string()]) {
        Ok(p) => p,
        Err(e) => return Ok(Err(e)),
    };
    let lock = lock_manifest(data_dir)?;
    let mut manifest = load_manifest(&lock)?;
    let outcome = match machine.commit_transition(prepared) {
        Ok(o) => o,
        Err(e) => return Ok(Err(e)),
    };
    track_ledger_in_manifest(&mut manifest, machine.ledger(), config)?;
    save_manifest(&mut manifest, &lock)?;
    Ok(Ok(outcome))
}

// [cmd-batch]
/// Apply a declared batch: for each selected step, run its query and give every
/// item the step's transition.
///
/// Gate refusals are reported, not fatal. A transition with a budget gate —
/// "defer at most half the MEDIUMs" — is supposed to start refusing partway
/// through, and the whole point of running the batch is to reach that line.
pub fn cmd_batch(
    config_dir: &str,
    name: &str,
    selector: &[String],
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

    let Some(batch) = config.batches.get(name) else {
        eprintln!("error: no batch '{}' declared in protocol.toml", name);
        return EXIT_USAGE_ERROR;
    };

    let selected = match parse_selector(batch, selector) {
        Ok(v) => v,
        Err(reason) => {
            eprintln!("error: batch '{}': {}", name, reason);
            return EXIT_USAGE_ERROR;
        }
    };

    let steps = match select_steps(&batch.steps, &selected) {
        Ok(s) => s,
        Err(reason) => {
            eprintln!("error: batch '{}': {}", name, reason);
            return EXIT_USAGE_ERROR;
        }
    };

    let (ledger, mode) = match open_targeted_ledger(&config, targeting, &config_path) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };
    if let Err((code, msg)) = guard_event_only(&mode, "run a batch") {
        eprintln!("{}", msg);
        return code;
    }

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let ledger_path = ledger.path().to_path_buf();
    let mut machine = StateMachine::new(&config, ledger);

    let mut applied_total = 0usize;
    let mut refused_total = 0usize;
    let mut emitted: Vec<String> = Vec::new();
    let mut last_state: Option<String> = None;

    for step in steps {
        // Read the population per step, not once up front: an earlier step's
        // transitions are already on the ledger, and a later step must see
        // them (the same reason gates re-read the file rather than a cache).
        let ids = match items_of(step, &config, &ledger_path) {
            Ok(ids) => ids,
            Err(reason) => {
                eprintln!("error: batch '{}': {}", name, reason);
                return EXIT_INTEGRITY_ERROR;
            }
        };

        let mut applied = 0usize;
        let mut refused: Vec<(String, String)> = Vec::new();

        for id in &ids {
            let result = match apply_item(&mut machine, &config, &data_dir, &step.transition, id) {
                Ok(r) => r,
                Err((code, msg)) => {
                    eprintln!("{}", msg);
                    return code;
                }
            };
            match result {
                Ok(outcome) => {
                    applied += 1;
                    last_state = Some(outcome.to.clone());
                    for event_type in outcome.emitted_events {
                        if !emitted.contains(&event_type) {
                            emitted.push(event_type);
                        }
                    }
                }
                Err(StateError::GateBlocked {
                    gate_type,
                    reason,
                    intent,
                }) => {
                    refused.push((
                        id.clone(),
                        crate::cli::output::gate_failure_line(
                            &gate_type,
                            &reason,
                            intent.as_deref(),
                        ),
                    ));
                }
                Err(StateError::AllCandidatesBlocked { candidates, .. }) => {
                    let reason = candidates
                        .first()
                        .map(|c| {
                            crate::cli::output::gate_failure_line(
                                &c.gate_type,
                                &c.reason,
                                c.intent.as_deref(),
                            )
                        })
                        .unwrap_or_else(|| "blocked".to_string());
                    refused.push((id.clone(), reason));
                }
                // Something else is moving this machine; every remaining item
                // would be checked against a state that no longer holds.
                Err(e @ StateError::StateMoved { .. }) => {
                    eprintln!(
                        "error: batch '{}': {} on '{}': {}",
                        name, step.transition, id, e
                    );
                    return EXIT_GATE_FAILED;
                }
                Err(e) => {
                    eprintln!(
                        "error: batch '{}': {} on '{}': {}",
                        name, step.transition, id, e
                    );
                    return EXIT_INTEGRITY_ERROR;
                }
            }
        }

        applied_total += applied;
        refused_total += refused.len();
        println!(
            "{}: {} applied, {} refused (of {} in '{}')",
            step.transition,
            applied,
            refused.len(),
            ids.len(),
            step.items
        );
        // Every refusal, named. A batch that stops at a budget has to say
        // which items it left, or the caller cannot tell a spent budget from
        // an empty population.
        for (id, reason) in &refused {
            println!("  - {} {}", id, reason);
        }
    }

    // Every item sealed the ledger as it committed. The renders run once, under
    // a lock of their own, from the ledger as it is now — other writers may
    // have appended between items, and a render from this machine's older copy
    // would seal a view of a ledger that is no longer the one on disk.
    let lock = match lock_manifest(&data_dir) {
        Ok(l) => l,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };
    let mut manifest = match load_manifest(&lock) {
        Ok(m) => m,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };
    if let Err(e) = machine.reload_ledger() {
        eprintln!("error: batch '{}': {}", name, e);
        return EXIT_INTEGRITY_ERROR;
    }

    let render_count = super::transition::render_after_transitions(
        &config,
        &config_path,
        targeting,
        machine.ledger(),
        &mut manifest,
        &lock,
        &emitted,
    );
    drop(lock);

    if let Some(state) = last_state {
        write_status_cache(&data_dir, &config, &config_path, &state);
    }

    if render_count > 0 {
        println!(
            "batch {}: {} applied, {} refused ({} rendered)",
            name, applied_total, refused_total, render_count
        );
    } else {
        println!(
            "batch {}: {} applied, {} refused",
            name, applied_total, refused_total
        );
    }

    EXIT_SUCCESS
}
