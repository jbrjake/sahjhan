// src/gates/evaluator.rs
//
// GateContext, GateResult, and the top-level evaluate_gate / evaluate_gates functions.
//
// ## Index
// - GateContext              — all inputs needed to evaluate a gate (ledger, config, state_params, etc.)
// - GateResult               — outcome: passed, gate_type, description, reason, intent
// - default_intent           — returns default intent string for a given gate type
// - [evaluate-gate]          evaluate_gate()   — evaluate a single gate
// - [evaluate-gates]         evaluate_gates()  — evaluate all gates, return all results

use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::{GateConfig, ProtocolConfig};
use crate::ledger::chain::Ledger;

use super::types;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// All the context needed to evaluate a gate.
pub struct GateContext<'a> {
    /// The ledger to query for events.
    pub ledger: &'a Ledger,
    /// Full protocol configuration.
    pub config: &'a ProtocolConfig,
    /// Name of the state the machine is currently in.
    pub current_state: &'a str,
    /// Key/value parameters extracted from the current state (used for
    /// template variable resolution).
    pub state_params: HashMap<String, String>,
    /// The directory in which shell commands should be executed.
    pub working_dir: PathBuf,
    /// The payload fields of the triggering event, if any (used by
    /// `field_not_empty`).
    pub event_fields: Option<&'a HashMap<String, String>>,
}

/// The outcome of evaluating a single gate.
pub struct GateResult {
    /// Whether the gate condition was satisfied.
    pub passed: bool,
    /// Whether the gate could be evaluated. `false` when required template
    /// variables are missing — distinct from a gate that was evaluated and failed.
    pub evaluable: bool,
    /// The gate type string (e.g. `"file_exists"`).
    pub gate_type: String,
    /// Human-readable description of what the gate checks.
    pub description: String,
    /// If `passed` is `false`, a human-readable explanation of why.
    pub reason: Option<String>,
    /// Why this gate exists — taken from `GateConfig.intent` if set,
    /// otherwise filled in from `default_intent` by the dispatch wrapper.
    pub intent: Option<String>,
}

/// Return the default intent string for a given gate type.
///
/// Used when a GateConfig does not specify an explicit intent.
pub fn default_intent(gate_type: &str) -> &str {
    match gate_type {
        "file_exists" | "files_exist" => "required files must exist before proceeding",
        "command_succeeds" => "command must pass before proceeding",
        "command_output" => "command output must match expected value",
        "ledger_has_event" => "required events must be recorded first",
        "ledger_has_event_since" => "required events must occur since last transition",
        "ledger_lacks_event" => "prohibited events must not exist",
        "set_covered" => "all set members must be completed",
        "min_elapsed" => "minimum time must elapse before proceeding",
        "no_violations" => "all protocol violations must be resolved",
        "field_not_empty" => "required field must have a value",
        "snapshot_compare" => "snapshot must match expected state",
        "query" => "query condition must be satisfied",
        "any_of" => "at least one alternative must pass",
        "all_of" => "all conditions must pass",
        "not" => "condition must not be met",
        "k_of_n" => "minimum number of conditions must pass",
        _ => "gate condition must be met",
    }
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

// [evaluate-gate]
/// Evaluate a single gate against the provided context.
pub fn evaluate_gate(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    types::eval(gate, ctx)
}

// [evaluate-gates]
/// Evaluate every gate in `gates` and return all results.
///
/// All gates are evaluated even when earlier ones fail, so callers can
/// present the full picture.
pub fn evaluate_gates(gates: &[GateConfig], ctx: &GateContext) -> Vec<GateResult> {
    gates.iter().map(|g| evaluate_gate(g, ctx)).collect()
}
