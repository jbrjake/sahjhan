// src/gates/types.rs
//
// Dispatch function and shared helpers used by gate category modules.
//
// ## Index
// - [eval]                      eval()                      — dispatch a gate by gate_type
// - [build-template-vars]       build_template_vars()       — build template variable map from GateContext
// - [validate-template-fields]  validate_template_fields()  — validate {{var}} values against event field patterns
// - [entry-matches-filter]      entry_matches_filter()      — check if a ledger entry matches all filter k/v pairs

use std::collections::HashMap;

use regex::Regex;

use crate::config::GateConfig;
use crate::ledger::entry::LedgerEntry;

use super::evaluator::{GateContext, GateResult};

// ---------------------------------------------------------------------------
// Public dispatch
// ---------------------------------------------------------------------------

// [eval]
/// Evaluate a single gate by dispatching on `gate.gate_type`.
///
/// After the gate module returns a result, the dispatch wrapper fills in
/// `result.intent` from `gate.intent` (if set) or the default for the gate type.
pub fn eval(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let mut result = match gate.gate_type.as_str() {
        "file_exists" => super::file::eval_file_exists(gate, ctx),
        "files_exist" => super::file::eval_files_exist(gate, ctx),
        "command_succeeds" => super::command::eval_command_succeeds(gate, ctx),
        "command_output" => super::command::eval_command_output(gate, ctx),
        "ledger_has_event" => super::ledger::eval_ledger_has_event(gate, ctx),
        "ledger_has_event_since" => super::ledger::eval_ledger_has_event_since(gate, ctx),
        "ledger_lacks_event" => super::ledger::eval_ledger_lacks_event(gate, ctx),
        "set_covered" => super::ledger::eval_set_covered(gate, ctx),
        "min_elapsed" => super::ledger::eval_min_elapsed(gate, ctx),
        "no_violations" => super::ledger::eval_no_violations(gate, ctx),
        "field_not_empty" => super::ledger::eval_field_not_empty(gate, ctx),
        "snapshot_compare" => super::snapshot::eval_snapshot_compare(gate, ctx),
        "query" => super::query::eval_query_gate(gate, ctx),

        // -- Composite gates --------------------------------------------------

        "any_of" => {
            let results: Vec<GateResult> = gate.gates.iter().map(|g| eval(g, ctx)).collect();
            let total = results.len();
            let passed_count = results.iter().filter(|r| r.passed).count();
            let passed = passed_count > 0;
            let reason = if !passed {
                let failed: Vec<String> = results
                    .iter()
                    .map(|r| format!("{}: {}", r.gate_type, r.description))
                    .collect();
                Some(format!("no alternatives passed: [{}]", failed.join("; ")))
            } else {
                None
            };
            GateResult {
                passed,
                gate_type: "any_of".to_string(),
                description: format!("{} of {} alternatives passed", passed_count, total),
                reason,
                intent: None,
            }
        }

        "all_of" => {
            let results: Vec<GateResult> = gate.gates.iter().map(|g| eval(g, ctx)).collect();
            let total = results.len();
            let passed_count = results.iter().filter(|r| r.passed).count();
            let passed = passed_count == total;
            let reason = if !passed {
                let failed: Vec<String> = results
                    .iter()
                    .filter(|r| !r.passed)
                    .map(|r| format!("{}: {}", r.gate_type, r.description))
                    .collect();
                Some(format!("failed conditions: [{}]", failed.join("; ")))
            } else {
                None
            };
            GateResult {
                passed,
                gate_type: "all_of".to_string(),
                description: format!("{} of {} conditions passed", passed_count, total),
                reason,
                intent: None,
            }
        }

        "not" => {
            if gate.gates.len() != 1 {
                return GateResult {
                    passed: false,
                    gate_type: "not".to_string(),
                    description: "not gate requires exactly one child gate".to_string(),
                    reason: Some(format!(
                        "expected 1 child gate, found {}",
                        gate.gates.len()
                    )),
                    intent: None,
                };
            }
            let child = eval(&gate.gates[0], ctx);
            GateResult {
                passed: !child.passed,
                gate_type: "not".to_string(),
                description: format!("not({})", child.gate_type),
                reason: if child.passed {
                    Some(format!("child gate '{}' passed (not inverts to fail)", child.gate_type))
                } else {
                    None
                },
                intent: None,
            }
        }

        "k_of_n" => {
            let k = gate
                .params
                .get("k")
                .and_then(|v| v.as_integer())
                .unwrap_or(0) as usize;
            let results: Vec<GateResult> = gate.gates.iter().map(|g| eval(g, ctx)).collect();
            let total = results.len();
            let passed_count = results.iter().filter(|r| r.passed).count();
            let passed = passed_count >= k;
            let reason = if !passed {
                let failed: Vec<String> = results
                    .iter()
                    .filter(|r| !r.passed)
                    .map(|r| format!("{}: {}", r.gate_type, r.description))
                    .collect();
                Some(format!(
                    "only {} of {} required passed; failed: [{}]",
                    passed_count, k, failed.join("; ")
                ))
            } else {
                None
            };
            GateResult {
                passed,
                gate_type: "k_of_n".to_string(),
                description: format!("{} of {} passed ({} required)", passed_count, total, k),
                reason,
                intent: None,
            }
        }

        other => GateResult {
            passed: false,
            gate_type: other.to_string(),
            description: format!("unknown gate type '{}'", other),
            reason: Some(format!("gate type '{}' is not implemented", other)),
            intent: None,
        },
    };
    result.intent = Some(
        gate.intent
            .clone()
            .unwrap_or_else(|| super::evaluator::default_intent(&gate.gate_type).to_string()),
    );
    result
}

// ---------------------------------------------------------------------------
// Shared helpers (pub(super) — used by sibling gate modules)
// ---------------------------------------------------------------------------

// [build-template-vars]
/// Build the template variable map from a `GateContext`.
pub(super) fn build_template_vars(ctx: &GateContext) -> HashMap<String, String> {
    let mut vars: HashMap<String, String> = ctx.state_params.clone();

    // Inject config.paths.* variables.
    vars.insert(
        "paths.data_dir".to_string(),
        ctx.config.paths.data_dir.clone(),
    );
    vars.insert(
        "paths.render_dir".to_string(),
        ctx.config.paths.render_dir.clone(),
    );
    // managed is a Vec<String>; join with colon as a reasonable default.
    vars.insert(
        "paths.managed".to_string(),
        ctx.config.paths.managed.join(":"),
    );

    // Inject set names as "sets.<name>" => comma-joined values.
    for (set_name, set_config) in &ctx.config.sets {
        vars.insert(format!("sets.{}", set_name), set_config.values.join(","));
    }

    vars
}

// [validate-template-fields]
/// Validate template variables against event field definitions.
///
/// For each `{{var}}` in the template that corresponds to a state_param, check
/// if there is an event field definition in `config.events` with a `pattern`
/// regex. If so, validate the value matches before allowing interpolation.
///
/// Issue #4: Field validation performed *before* template interpolation.
pub(super) fn validate_template_fields(template: &str, ctx: &GateContext) -> Result<(), String> {
    // Extract placeholder names from the template.
    let placeholders = extract_placeholders(template);

    for placeholder in &placeholders {
        // Only validate state_params values — config paths/sets are trusted.
        if let Some(value) = ctx.state_params.get(placeholder.as_str()) {
            // Search all event configs for a field with this name that has a pattern.
            if let Some(pattern) = find_field_pattern(ctx, placeholder) {
                match Regex::new(&pattern) {
                    Ok(re) => {
                        if !re.is_match(value) {
                            return Err(format!(
                                "field '{}' value '{}' does not match pattern '{}'",
                                placeholder, value, pattern
                            ));
                        }
                    }
                    Err(e) => {
                        return Err(format!(
                            "invalid regex pattern '{}' for field '{}': {}",
                            pattern, placeholder, e
                        ));
                    }
                }
            }
        }
    }

    Ok(())
}

// [entry-matches-filter]
/// Check whether a ledger entry's fields match all key/value pairs in `filter`.
pub(super) fn entry_matches_filter(entry: &LedgerEntry, filter: &HashMap<String, String>) -> bool {
    if filter.is_empty() {
        return true;
    }
    filter
        .iter()
        .all(|(k, v)| entry.fields.get(k).map(|fv| fv == v).unwrap_or(false))
}

// ---------------------------------------------------------------------------
// Private helpers
// ---------------------------------------------------------------------------

/// Extract `{{placeholder}}` names from a template string.
fn extract_placeholders(template: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = template;
    while let Some(start) = rest.find("{{") {
        let after_start = &rest[start + 2..];
        if let Some(end) = after_start.find("}}") {
            names.push(after_start[..end].to_string());
            rest = &after_start[end + 2..];
        } else {
            break;
        }
    }
    names
}

/// Look up a field pattern from config.events for the given field name.
///
/// Searches all event definitions; returns the first `pattern` found for a
/// field with the given name.
fn find_field_pattern(ctx: &GateContext, field_name: &str) -> Option<String> {
    for event_config in ctx.config.events.values() {
        for field in &event_config.fields {
            if field.name == field_name && field.pattern.is_some() {
                return field.pattern.clone();
            }
        }
    }
    None
}
