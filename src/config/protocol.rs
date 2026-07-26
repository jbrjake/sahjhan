// src/config/protocol.rs
//
// Deserialization structs for protocol.toml.
//
// ## Index
// - ProtocolFile            — top-level wrapper (protocol, paths, sets, aliases, checkpoints, ledgers, guards, queries)
// - ProtocolMeta            — name, version, description
// - PathsConfig             — managed, data_dir, render_dir
// - SetConfig               — description + ordered values
// - CheckpointConfig        — checkpoint interval
// - LedgerTemplateConfig     — ledger declaration (path or path_template)
// - GuardsConfig            — write_gated paths
// - WriteGatedConfig        — path whose writability is gated by protocol state
// - NamedQuery              — a reusable, named SQL predicate ([queries.<name>])
// - LintConfig              — [lint] section; static-analysis strictness knobs
// - BoundaryConfig          — [[boundaries]]; an edge that must not be routed around
// - BoundaryEdge            — the from/to pair a boundary protects

use serde::Deserialize;
use std::collections::HashMap;

/// Represents the full contents of protocol.toml as deserialized from disk.
#[derive(Debug, Deserialize)]
pub struct ProtocolFile {
    pub protocol: ProtocolMeta,
    pub paths: PathsConfig,
    #[serde(default)]
    pub sets: HashMap<String, SetConfig>,
    #[serde(default)]
    pub aliases: HashMap<String, String>,
    #[serde(default)]
    pub checkpoints: CheckpointConfig,
    #[serde(default)]
    pub ledgers: HashMap<String, LedgerTemplateConfig>,
    pub guards: Option<GuardsConfig>,
    #[serde(default)]
    pub queries: HashMap<String, NamedQuery>,
    #[serde(default)]
    pub boundaries: Vec<BoundaryConfig>,
    #[serde(default)]
    pub lint: LintConfig,
}

/// An edge in the transition graph that must not be bypassed.
///
/// ```toml
/// [[boundaries]]
/// name = "context-reset"
/// must_traverse = { from = "merge_done", to = "fix_loop" }
/// ```
///
/// with the participating edges tagged:
///
/// ```toml
/// [[transitions]]
/// from = "awaiting_clear"
/// to   = "fix_loop"
/// command = "resume"
/// boundary = "context-reset"
/// ```
///
/// Lint check L3 then asserts the graph property: *every* path from
/// `merge_done` to `fix_loop` crosses an edge tagged `context-reset`. That is
/// something a hand-written test can only spot-check — a second transition
/// sharing a command name, added later for an unrelated reason, becomes a
/// bypass the moment anything routes into it.
#[derive(Debug, Deserialize, Clone)]
pub struct BoundaryConfig {
    pub name: String,
    pub must_traverse: BoundaryEdge,
}

/// The `from` → `to` pair a boundary protects.
#[derive(Debug, Deserialize, Clone)]
pub struct BoundaryEdge {
    pub from: String,
    pub to: String,
}

/// The `[lint]` section — how strict `sahjhan lint` is about this protocol.
///
/// ```toml
/// [lint]
/// require_producers = true      # every required event must name a producer
/// disabled_checks   = ["L6"]
/// ```
#[derive(Debug, Deserialize, Clone, Default)]
pub struct LintConfig {
    /// Treat "no visible producer" as an error for *every* required event, not
    /// just restricted ones.
    ///
    /// Off by default because `sahjhan event` can record any declared,
    /// non-restricted event — so without this flag the engine cannot claim such
    /// a gate is unsatisfiable. Turn it on once producers are declared, and L1
    /// becomes a closure check over the whole vocabulary.
    #[serde(default)]
    pub require_producers: bool,
    /// Check ids (`"L1"`, `"L6"`, …) to skip entirely.
    #[serde(default)]
    pub disabled_checks: Vec<String>,
}

/// A named SQL predicate declared once and referenced by many gates.
///
/// ```toml
/// [queries.pattern_analysis_overdue]
/// sql    = "SELECT count(*) < 3 FROM events WHERE event_type = 'fix'"
/// intent = "3+ fixes since the last pattern analysis"
/// ```
///
/// A gate references it by name instead of carrying a copy of the SQL:
///
/// ```toml
/// { type = "query", query = "pattern_analysis_overdue" }
/// ```
///
/// Two gates that must agree on a fact become the same object rather than two
/// strings hoped to be equal — the drift that `lint` check L6 looks for cannot
/// happen once a predicate has a name. `intent` (if present) becomes the gate's
/// intent when the gate does not declare one of its own.
#[derive(Debug, Deserialize, Clone)]
pub struct NamedQuery {
    pub sql: String,
    #[serde(default)]
    pub intent: Option<String>,
}

/// Configuration for the `[guards]` section of protocol.toml.
///
/// Paths whose writability is gated by protocol state.
///
/// ```toml
/// [[guards.write_gated]]
/// path = "src/main.rs"
/// writable_in = ["coding", "review"]
/// message = "Source files are only writable during coding and review states"
/// ```
#[derive(Debug, Deserialize, Clone, Default)]
pub struct GuardsConfig {
    #[serde(default)]
    pub write_gated: Vec<WriteGatedConfig>,
}

/// A path whose writability is gated by protocol state.
#[derive(Debug, Deserialize, Clone)]
pub struct WriteGatedConfig {
    pub path: String,
    pub writable_in: Vec<String>,
    pub message: String,
}

/// Configuration for the `[checkpoints]` section of protocol.toml.
///
/// ```toml
/// [checkpoints]
/// interval = 100  # 0 = disabled
/// ```
#[derive(Debug, Deserialize, Default, Clone)]
pub struct CheckpointConfig {
    /// How often (in events) to auto-checkpoint. `0` means disabled.
    #[serde(default)]
    pub interval: u64,
}

/// The `[protocol]` section.
#[derive(Debug, Deserialize, Clone)]
pub struct ProtocolMeta {
    pub name: String,
    pub version: String,
    pub description: String,
}

/// The `[paths]` section.
#[derive(Debug, Deserialize, Clone)]
pub struct PathsConfig {
    pub managed: Vec<String>,
    pub data_dir: String,
    pub render_dir: String,
}

/// A named set of values (e.g. `[sets.check]`).
#[derive(Debug, Deserialize, Clone)]
pub struct SetConfig {
    pub description: String,
    pub values: Vec<String>,
}

/// A ledger declaration in protocol.toml.
///
/// Two forms:
/// - **Template** (`path_template`): pattern with `{template.instance_id}` / `{template.name}`
/// - **Fixed** (`path`): single known path, no instantiation
///
/// These are mutually exclusive.
#[derive(Debug, Deserialize, Clone)]
pub struct LedgerTemplateConfig {
    pub description: String,
    /// Fixed path (for singleton ledgers).
    pub path: Option<String>,
    /// Path template with `{template.instance_id}` and `{template.name}` variables.
    pub path_template: Option<String>,
}
