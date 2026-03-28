// src/gates/evaluator.rs
//
// GateContext, GateResult, and the top-level evaluate_gate / evaluate_gates functions.
//
// ## Index
// - GateContext              — all inputs needed to evaluate a gate (ledger, config, state_params, etc.)
// - GateResult               — outcome: passed, gate_type, description, reason
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
    /// The gate type string (e.g. `"file_exists"`).
    pub gate_type: String,
    /// Human-readable description of what the gate checks.
    pub description: String,
    /// If `passed` is `false`, a human-readable explanation of why.
    pub reason: Option<String>,
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
