// src/gates/types.rs
//
// Dispatch function and shared helpers used by gate category modules.
//
// ## Index
// - [eval]                      eval()                      — dispatch a gate by gate_type
// - [build-template-vars]       build_template_vars()       — build template variable map from GateContext
// - [validate-template-fields]  validate_template_fields()  — validate {{var}} values against event field patterns
// - [entry-matches-filter]      entry_matches_filter()      — check if a ledger entry matches all filter k/v pairs
// - [gate-filter]               gate_filter()               — build a gate's filter map, resolving {{var}} in values
// - [filter-spec]               filter_spec()               — a gate's filter table as written, before resolution
// - [resolve-filter-spec]       resolve_filter_spec()       — resolve {{var}} in a filter spec's values
// - [candidate-refs]            candidate_refs()            — the {{event.<field>}} names a filter spec correlates on
// - [candidate-vars]            candidate_vars()            — event.<field> bindings contributed by one ledger entry
// - GateAnchor                  — the directory a command gate is evaluated at: Project (default) or Caller
// - AnchorError                 — a non-string `anchor`, or a string naming neither anchor
// - ANCHORED_GATE_TYPES         — the gate types that run a command, and so have a working directory
// - [resolve-gate-anchor]       resolve_gate_anchor()       — a gate's `anchor` param as written → GateAnchor
// - [gate-working-dir]          gate_working_dir()          — the directory this gate's command runs in

use std::collections::HashMap;
use std::path::Path;

use regex::Regex;

use crate::config::GateConfig;
use crate::ledger::entry::LedgerEntry;

use super::evaluator::{GateContext, GateResult};
use super::template::resolve_template_plain;

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
                evaluable: true,
                gate_type: "any_of".to_string(),
                description: format!("{} of {} alternatives passed", passed_count, total),
                reason,
                intent: None,
                attestation: None,
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
                evaluable: true,
                gate_type: "all_of".to_string(),
                description: format!("{} of {} conditions passed", passed_count, total),
                reason,
                intent: None,
                attestation: None,
            }
        }

        "not" => {
            if gate.gates.len() != 1 {
                return GateResult {
                    passed: false,
                    evaluable: true,
                    gate_type: "not".to_string(),
                    description: "not gate requires exactly one child gate".to_string(),
                    reason: Some(format!("expected 1 child gate, found {}", gate.gates.len())),
                    intent: None,
                    attestation: None,
                };
            }
            let child = eval(&gate.gates[0], ctx);
            GateResult {
                passed: !child.passed,
                evaluable: true,
                gate_type: "not".to_string(),
                description: format!("not({})", child.gate_type),
                reason: if child.passed {
                    Some(format!(
                        "child gate '{}' passed (not inverts to fail)",
                        child.gate_type
                    ))
                } else {
                    None
                },
                intent: None,
                attestation: None,
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
                    passed_count,
                    k,
                    failed.join("; ")
                ))
            } else {
                None
            };
            GateResult {
                passed,
                evaluable: true,
                gate_type: "k_of_n".to_string(),
                description: format!("{} of {} passed ({} required)", passed_count, total, k),
                reason,
                intent: None,
                attestation: None,
            }
        }

        other => GateResult {
            passed: false,
            evaluable: true,
            gate_type: other.to_string(),
            description: format!("unknown gate type '{}'", other),
            reason: Some(format!("gate type '{}' is not implemented", other)),
            intent: None,
            attestation: None,
        },
    };
    // Intent precedence: the gate's own `intent`, then the `intent` of the
    // named query it references (a named predicate carries its "why" once,
    // for every gate that uses it), then the per-type default.
    result.intent = Some(
        gate.intent
            .clone()
            .or_else(|| named_query_intent(gate, ctx))
            .unwrap_or_else(|| super::evaluator::default_intent(&gate.gate_type).to_string()),
    );
    result
}

/// The `intent` of the `[queries.<name>]` entry a query gate references, if any.
fn named_query_intent(gate: &GateConfig, ctx: &GateContext) -> Option<String> {
    if gate.gate_type != "query" {
        return None;
    }
    let name = gate.params.get("query").and_then(|v| v.as_str())?;
    ctx.config.queries.get(name)?.intent.clone()
}

// ---------------------------------------------------------------------------
// Anchoring: which directory a command gate is evaluated at
// ---------------------------------------------------------------------------

/// The directory a gate that runs a command is evaluated at.
///
/// The project root is the default and stays the default: a gate `cmd` is
/// written relative to the project, so running it from whichever subdirectory
/// the caller happened to be in is the same defect as keying a manifest entry
/// there (holtz #85).
///
/// `Caller` is the per-gate opt-out, for the one question the project anchor
/// cannot express: *does this actor's own tree satisfy the condition*. With
/// several actors working concurrently in separate git worktrees, a
/// tree-reading gate anchored at the project is true for at most one of them,
/// which turns the block it guards into a dead end for the rest (sahjhan #46).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GateAnchor {
    /// The project root — the anchor derived from the config's `data_dir`.
    Project,
    /// The directory the caller invoked sahjhan from.
    Caller,
}

/// Why a gate's `anchor` names no directory the engine can evaluate it at.
///
/// Both variants used to be spelled the same way as a correct config: an
/// unreadable `anchor` would have fallen through to the project root, which is
/// also what a gate that never mentions one gets. A typo would then read as a
/// deliberate choice — #34's failure mode on a new surface.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum AnchorError {
    /// Not a string at all: `anchor = 1` rather than `anchor = "caller"`.
    NotAString(String),
    /// A string naming neither anchor.
    Unrecognized(String),
}

impl std::fmt::Display for AnchorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AnchorError::NotAString(found) => write!(
                f,
                "has a non-string anchor (found {}; expected the string \"project\" or \"caller\")",
                found
            ),
            AnchorError::Unrecognized(value) => write!(
                f,
                "has unrecognized anchor '{}' (expected \"project\" or \"caller\")",
                value
            ),
        }
    }
}

impl std::error::Error for AnchorError {}

/// The gate types that execute a command, and so have a working directory to
/// anchor. `anchor` on any other type would be read by nothing, so config
/// validation rejects it rather than let it sit there looking effective.
pub(crate) const ANCHORED_GATE_TYPES: [&str; 3] =
    ["command_succeeds", "command_output", "snapshot_compare"];

// [resolve-gate-anchor]
/// Resolve a gate's `anchor` **parameter as written** — the one reader of the
/// parameter, so config validation and gate evaluation cannot disagree about
/// where a gate runs.
///
/// - absent → [`GateAnchor::Project`], the documented default;
/// - `"project"` / `"caller"` → the named anchor;
/// - any other string, or a non-string → an error. TOML is typed, so
///   `anchor = 1` is not the string `"1"`; it misses `as_str()`, and silently
///   taking the default is precisely how a mis-anchored gate stays invisible.
pub(crate) fn resolve_gate_anchor(gate: &GateConfig) -> Result<GateAnchor, AnchorError> {
    match gate.params.get("anchor") {
        None => Ok(GateAnchor::Project),
        Some(value) => match value.as_str() {
            Some("project") => Ok(GateAnchor::Project),
            Some("caller") => Ok(GateAnchor::Caller),
            Some(other) => Err(AnchorError::Unrecognized(other.to_string())),
            None => Err(AnchorError::NotAString(value.type_str().to_string())),
        },
    }
}

// [gate-working-dir]
/// The directory this gate's command runs in.
///
/// An anchor the engine cannot read fails the gate rather than falling back to
/// the project root: the fallback is indistinguishable from a gate that chose
/// the project deliberately, and a gate evaluated somewhere its author did not
/// mean is evidence of nothing. Config validation rejects such an anchor before
/// it can be sealed; this is the second line, for a config loaded past it.
pub(super) fn gate_working_dir<'a>(
    gate: &GateConfig,
    ctx: &'a GateContext,
) -> Result<&'a Path, String> {
    match resolve_gate_anchor(gate) {
        Ok(GateAnchor::Project) => Ok(ctx.working_dir.as_path()),
        Ok(GateAnchor::Caller) => Ok(ctx.caller_dir.as_path()),
        Err(e) => Err(format!("gate '{}' {}", gate.gate_type, e)),
    }
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

// [gate-filter]
/// Build a gate's optional field filter, resolving `{{var}}` in each value.
///
/// The three ledger gates each carried an identical copy of this extraction,
/// and all three compared filter values *literally* — so a filter could only
/// ever name a constant known when the config was written. A hook gate that
/// must be scoped to the actor which triggered it needs the value to come
/// from the request instead, which is what every other gate type already gets
/// from `build_template_vars`.
///
/// A placeholder with no binding is left literal by `resolve_template_plain`,
/// so it matches no entry and the gate fails closed rather than silently
/// widening to every actor.
pub(super) fn gate_filter(gate: &GateConfig, ctx: &GateContext) -> HashMap<String, String> {
    resolve_filter_spec(&filter_spec(gate, "filter"), &build_template_vars(ctx))
}

// [filter-spec]
/// A gate's key/value filter table as written, before any resolution.
///
/// Kept unresolved because the anchor-side filter (`since_filter`) may
/// correlate on a field of the candidate event, and so has to be resolved once
/// per candidate rather than once per gate. Non-string values are dropped:
/// a ledger field is a string, so nothing else could ever match.
pub(crate) fn filter_spec(gate: &GateConfig, key: &str) -> Vec<(String, String)> {
    gate.params
        .get(key)
        .and_then(|v| v.as_table())
        .map(|tbl| {
            tbl.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

// [resolve-filter-spec]
/// Resolve `{{var}}` in each value of a filter spec.
///
/// An unbound placeholder is left literal by `resolve_template_plain`, so it
/// matches no entry — which fails a candidate-side filter closed, and widens an
/// anchor-side one. That asymmetry is why `since_filter` is checked where the
/// config is loaded rather than trusted here.
pub(super) fn resolve_filter_spec(
    spec: &[(String, String)],
    vars: &HashMap<String, String>,
) -> HashMap<String, String> {
    spec.iter()
        .map(|(k, v)| (k.clone(), resolve_template_plain(v, vars)))
        .collect()
}

/// The prefix that makes a placeholder refer to the candidate event's own
/// fields rather than to a state param: `{{event.finding_id}}`.
pub(crate) const CANDIDATE_PREFIX: &str = "event.";

// [candidate-refs]
/// The `event.<field>` names a filter spec correlates on, deduplicated.
///
/// Empty means the spec resolves to one filter for the whole gate; non-empty
/// means the window is per candidate.
pub(crate) fn candidate_refs(spec: &[(String, String)]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for (_, value) in spec {
        for placeholder in extract_placeholders(value) {
            if let Some(field) = placeholder.strip_prefix(CANDIDATE_PREFIX) {
                if !field.is_empty() && !out.iter().any(|f| f == field) {
                    out.push(field.to_string());
                }
            }
        }
    }
    out
}

// [candidate-vars]
/// Template bindings one ledger entry contributes as the candidate event:
/// every field of the entry, under the `event.` prefix.
pub(super) fn candidate_vars(
    entry: &LedgerEntry,
    base: &HashMap<String, String>,
) -> HashMap<String, String> {
    let mut vars = base.clone();
    for (k, v) in &entry.fields {
        vars.insert(format!("{}{}", CANDIDATE_PREFIX, k), v.clone());
    }
    vars
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
///
/// The scan itself lives in `template.rs`; there is one copy so a placeholder
/// this module validates is the same one that module resolves.
fn extract_placeholders(template: &str) -> Vec<String> {
    super::template::find_unresolved_vars(template)
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
