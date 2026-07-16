// src/config/transitions.rs
//
// Deserialization structs for transitions.toml.
//
// ## Index
// - TransitionsFile         — top-level wrapper
// - TransitionConfig        — from, to, command, args (positional params), gates
// - GateConfig              — gate_type + optional intent + nested gates (composite) + flattened params

use serde::Deserialize;
use std::collections::HashMap;

/// Wrapper for the full transitions.toml file.
#[derive(Debug, Deserialize)]
pub struct TransitionsFile {
    pub transitions: Vec<TransitionConfig>,
}

/// A single transition definition.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct TransitionConfig {
    pub from: String,
    pub to: String,
    pub command: String,
    /// Named positional arguments for template variable resolution.
    ///
    /// When a transition declares `args = ["item_id"]`, the first positional
    /// CLI argument (one without `=`) is mapped to `item_id` in state_params
    /// before gate evaluation.
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub gates: Vec<GateConfig>,
    /// Events appended automatically when this transition's gates all pass.
    ///
    /// Lets a transition record the domain-state event it implies — e.g.
    /// `fix_commit` emitting `finding_resolved` — in the same atomic step,
    /// instead of forcing the agent to issue a second, redundant command that
    /// restates the same fact. See [`EmitConfig`].
    #[serde(default)]
    pub emits: Vec<EmitConfig>,
}

/// An event emitted automatically on a successful transition.
///
/// After the transition's `state_transition` event is appended, each declared
/// emit resolves its `fields` templates and appends `event` to the ledger.
///
/// Field templates use `{{name}}` placeholders resolved (raw, unescaped) from,
/// in increasing precedence:
/// 1. the most recent value of each field across the ledger (so an emit inherits
///    run context like `project`/`run`/`auditor` without restating it),
/// 2. the transition's `state_params` (positional `args` such as `item_id`, plus
///    any `key=value` CLI args), and
/// 3. the trimmed stdout of each `commands` entry (for values derived from the
///    environment at emit time, e.g. `git rev-parse --short=7 HEAD`).
///
/// Literals (templates with no `{{…}}`) pass through unchanged. If any field
/// template still contains an unresolved `{{var}}`, or a command fails, the
/// whole transition is blocked and nothing is appended (atomic).
///
/// The target `event` must be defined in `events.toml` and must NOT be
/// `restricted` — emits may not bypass the HMAC proof that `authed-event`
/// requires (enforced by config validation and again at emit time).
#[derive(Debug, Deserialize, Clone, Default)]
pub struct EmitConfig {
    /// Event type to append. Must be a defined, non-restricted event.
    pub event: String,
    /// `var_name -> shell command`. Each command runs in the transition's
    /// working directory; its trimmed stdout is bound to `var_name` for use in
    /// `fields` templates. A non-zero exit or timeout blocks the transition.
    #[serde(default)]
    pub commands: HashMap<String, String>,
    /// `field_name -> template`. Templates are resolved with `{{var}}`
    /// substitution (see struct docs) to produce the emitted event's fields.
    #[serde(default)]
    pub fields: HashMap<String, String>,
}

/// A gate condition attached to a transition.
///
/// The `type` field identifies the gate kind; all remaining fields are
/// captured in `params` via `#[serde(flatten)]` so that we can handle
/// different gate shapes without needing an exhaustive enum.
#[derive(Debug, Deserialize, Clone)]
pub struct GateConfig {
    #[serde(rename = "type")]
    pub gate_type: String,
    /// Optional human-readable explanation of why this gate exists.
    /// If absent, a default intent is derived from the gate type at evaluation time.
    #[serde(default)]
    pub intent: Option<String>,
    /// Nested gates for composite types (any_of, all_of, not, k_of_n).
    /// Empty for leaf gates.
    #[serde(default)]
    pub gates: Vec<GateConfig>,
    #[serde(flatten)]
    pub params: HashMap<String, toml::Value>,
}
