// src/lint/mod.rs
//
// Static integrity analysis of the protocol graph (issue #32).
//
// `validate` answers "is this config well-formed". Lint answers the next
// question: "is this protocol coherent" — can every gate that blocks the agent
// ever be satisfied, can every state be left, does every declared event mean
// something, and can a boundary be routed around. All of it is decided at rest
// from config the engine already parses; nothing here opens a ledger or runs a
// command, so it is cheap enough for a pre-commit hook.
//
// The checks are deliberately weak and syntactic: sound (a finding is a real
// defect), incomplete (a clean run is not a proof), and cheap. There is no
// constraint solving here and there should not be.
//
// ## Index
// - CHECKS                  — id/description of every implemented check
// - Severity                — Error | Warning
// - LintFinding             — one defect: check id, severity, location, message, hint
// - LintOptions             — which checks to run
// - Analysis                — shared derived facts (graph, producers, consumers)
// - [lint-run]              run()  — run every enabled check, sorted findings

pub mod checks;
pub mod graph;
pub mod index;
pub mod similarity;

use std::collections::{HashMap, HashSet};

use serde::Serialize;

use crate::config::ProtocolConfig;

/// Every implemented check, in report order.
pub const CHECKS: &[(&str, &str)] = &[
    (
        "L1",
        "every event a gate requires has at least one producer",
    ),
    (
        "L2",
        "a required event's producers can run before the gate that needs them",
    ),
    (
        "L3",
        "no path reaches a boundary's target without crossing the boundary",
    ),
    (
        "L4",
        "every non-terminal state has at least one usable exit",
    ),
    ("L5", "every declared event is produced or consumed"),
    (
        "L6",
        "a predicate that decides a fact is declared once, not copied",
    ),
    (
        "L7",
        "evidence is at least as strong as the gate relying on it",
    ),
];

/// How serious a finding is.
///
/// `Error` means the protocol is provably broken given what the engine can
/// see; `Warning` means it is suspicious but a legitimate reading exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warning => "warning",
        }
    }
}

/// One defect found by static analysis.
#[derive(Debug, Clone, Serialize)]
pub struct LintFinding {
    /// Check id — `"L1"`, `"L4"`, …
    pub check: String,
    pub severity: Severity,
    /// Where the defect lives, e.g. `transitions.toml: transition 'submit' (implementing → verifying)`.
    pub location: String,
    /// What is wrong.
    pub message: String,
    /// What to do about it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hint: Option<String>,
}

impl LintFinding {
    pub fn error(check: &str, location: String, message: String) -> Self {
        LintFinding {
            check: check.to_string(),
            severity: Severity::Error,
            location,
            message,
            hint: None,
        }
    }

    pub fn warning(check: &str, location: String, message: String) -> Self {
        LintFinding {
            check: check.to_string(),
            severity: Severity::Warning,
            location,
            message,
            hint: None,
        }
    }

    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// Which checks to run.
#[derive(Debug, Clone, Default)]
pub struct LintOptions {
    /// Run only these check ids. Empty means "all enabled ones".
    /// An explicit selection overrides `[lint] disabled_checks`.
    pub only: Vec<String>,
}

/// Facts derived once and shared by the checks.
pub struct Analysis<'a> {
    pub config: &'a ProtocolConfig,
    pub graph: graph::Graph,
    pub producers: index::ProducerIndex,
    pub consumed: HashSet<String>,
    /// Transition index -> why it can never fire. Filled in by L1 and L2;
    /// read by L4, which needs to know whether a state's exits are real.
    pub unsatisfiable: HashMap<usize, String>,
}

impl<'a> Analysis<'a> {
    pub fn new(config: &'a ProtocolConfig) -> Self {
        Analysis {
            config,
            graph: graph::Graph::build(config),
            producers: index::ProducerIndex::build(config),
            consumed: index::consumed_events(config),
            unsatisfiable: HashMap::new(),
        }
    }

    /// Human-readable location for a transition, by index.
    pub fn transition_location(&self, index: usize) -> String {
        match self.config.transitions.get(index) {
            Some(t) => format!(
                "transitions.toml: transition '{}' ({} \u{2192} {})",
                t.command, t.from, t.to
            ),
            None => "transitions.toml".to_string(),
        }
    }
}

// [lint-run]
/// Run every enabled check and return the findings, ordered by check id then
/// location.
///
/// Checks run in dependency order regardless of the selection — L4 needs L1's
/// verdict on which transitions can never fire — and the selection is applied
/// to the *findings* at the end.
pub fn run(config: &ProtocolConfig, opts: &LintOptions) -> Vec<LintFinding> {
    let mut analysis = Analysis::new(config);
    let mut findings = Vec::new();

    // L1 populates `analysis.unsatisfiable`, which L4 reads.
    findings.extend(checks::l1_unsatisfiable_gates(&mut analysis));
    findings.extend(checks::l2_temporally_unsatisfiable(&mut analysis));
    findings.extend(checks::l3_boundary_route_around(&analysis));
    findings.extend(checks::l4_dead_end_states(&analysis));
    findings.extend(checks::l5_dead_vocabulary(&analysis));
    findings.extend(checks::l6_predicate_drift(&analysis));
    findings.extend(checks::l7_forgeable_evidence(&analysis));

    let selected: HashSet<&str> = if opts.only.is_empty() {
        CHECKS
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| {
                !config
                    .lint
                    .disabled_checks
                    .iter()
                    .any(|d| d.eq_ignore_ascii_case(id))
            })
            .collect()
    } else {
        CHECKS
            .iter()
            .map(|(id, _)| *id)
            .filter(|id| opts.only.iter().any(|o| o.eq_ignore_ascii_case(id)))
            .collect()
    };

    findings.retain(|f| selected.contains(f.check.as_str()));
    findings.sort_by(|a, b| {
        a.check
            .cmp(&b.check)
            .then_with(|| a.location.cmp(&b.location))
            .then_with(|| a.message.cmp(&b.message))
    });
    findings
}

/// Check ids in `only` that name no implemented check.
///
/// The CLI reports these rather than silently running nothing.
pub fn unknown_check_ids(only: &[String]) -> Vec<String> {
    only.iter()
        .filter(|o| !CHECKS.iter().any(|(id, _)| id.eq_ignore_ascii_case(o)))
        .cloned()
        .collect()
}
