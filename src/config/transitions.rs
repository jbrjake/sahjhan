// src/config/transitions.rs
//
// Deserialization structs for transitions.toml.
//
// ## Index
// - TransitionsFile         — top-level wrapper
// - TransitionConfig        — from, to, command, args (positional params), gates, emits, boundary, integrity
// - IntegrityConfig         — per-transition evidence requirements (requires_attestation)
// - EmitConfig              — event + derivation commands + field templates + the anchor those commands run at
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
    /// Names the `[[boundaries]]` entry this edge satisfies.
    ///
    /// Tagging is what turns "the context reset happens on this edge" from a
    /// convention into a checkable graph property: lint check L3 removes every
    /// edge carrying the tag and asserts the boundary's target is then
    /// unreachable from its source.
    #[serde(default)]
    pub boundary: Option<String>,
    /// Integrity requirements this transition places on its own evidence.
    ///
    /// ```toml
    /// [[transitions]]
    /// command = "resume"
    ///   [transitions.integrity]
    ///   requires_attestation = "host"
    /// ```
    #[serde(default)]
    pub integrity: Option<IntegrityConfig>,
    /// Events appended automatically when this transition's gates all pass.
    ///
    /// Lets a transition record the domain-state event it implies — e.g.
    /// `fix_commit` emitting `finding_resolved` — in the same atomic step,
    /// instead of forcing the agent to issue a second, redundant command that
    /// restates the same fact. See [`EmitConfig`].
    #[serde(default)]
    pub emits: Vec<EmitConfig>,
}

/// Integrity requirements a transition places on the evidence its gates read.
///
/// `requires_attestation` names a level from `[attestation] levels`. Lint check
/// L7 then compares it against the declared attestation of every event the
/// transition's gates require, and reports evidence weaker than the transition
/// relies on — a gate demanding host-level proof, satisfied by an event the
/// agent can write itself, enforces nothing.
///
/// A gate may also carry `requires_attestation` directly, which overrides the
/// transition-level requirement for that gate.
#[derive(Debug, Deserialize, Clone, Default)]
pub struct IntegrityConfig {
    #[serde(default)]
    pub requires_attestation: Option<String>,
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
///
/// `deny_unknown_fields` is deliberate. Before sahjhan #48 this struct silently
/// swallowed any key it did not recognize, so `anchor = "caller"` — the spelling
/// a reader of #46 reaches for first — validated clean and did nothing. A key
/// this struct cannot act on is now a load error rather than a config that says
/// one thing and does another.
#[derive(Debug, Deserialize, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct EmitConfig {
    /// Event type to append. Must be a defined, non-restricted event.
    pub event: String,
    /// `var_name -> shell command`. Each command runs at this emit's `anchor`;
    /// its trimmed stdout is bound to `var_name` for use in `fields` templates.
    /// A non-zero exit or timeout blocks the transition.
    #[serde(default)]
    pub commands: HashMap<String, String>,
    /// `field_name -> template`. Templates are resolved with `{{var}}`
    /// substitution (see struct docs) to produce the emitted event's fields.
    #[serde(default)]
    pub fields: HashMap<String, String>,
    /// Where this emit's `commands` run: `"project"` (the default) or
    /// `"caller"`. Same key, same values and same validation as a gate's
    /// `anchor` — see [`crate::gates::types::resolve_anchor`].
    ///
    /// It is per emit rather than per command, and it is not inherited from any
    /// gate: a transition's gates can legitimately be anchored differently from
    /// each other (#46), so an emit following one of them would be guessing.
    ///
    /// Held as a `toml::Value` rather than a `String` so a non-string spelling
    /// is rejected by the same check, with the same message, as a gate's — not
    /// as a bare serde type error from a different layer.
    #[serde(default)]
    pub anchor: Option<toml::Value>,
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
