// src/cli/output.rs
//
// Structured command output for JSON and text formatting.
//
// ## Index
// - SCHEMA_VERSION              — current output schema version
// - CommandOutput               — trait for type-erased command dispatch
// - CommandResult<T>            — typed command result with envelope
// - ErrorData                   — structured error info
// - LegacyResult                — shim for unconverted commands
// - StatusData                  — status command output (includes warnings for missing cache, etc.)
// - EventOnlyStatusData         — event-only ledger status
// - SetSummaryData              — set summary (status + set-status)
// - MemberData                  — individual set member
// - TransitionSummaryData       — transition summary for status
// - GateResultData              — gate evaluation result
// - LogData                     — log dump / log tail output
// - EntryData                   — single ledger entry
// - GateCheckData               — gate check dry-run output
// - CandidateData               — single transition candidate
// - ManifestVerifyData          — manifest verify output
// - MismatchData                — single manifest mismatch
// - HookEvalData                — hook evaluation result
// - HookEvalMessage             — single hook eval message
// - HookAutoRecord              — auto-recorded event
// - HookMonitorWarning          — monitor warning
// - LedgerActivateData          — ledger activate output
// - LedgerDeactivateData        — ledger deactivate output
// - LintData                    — lint findings + counts
// - [gate-failure-line]         gate_failure_line() — "<type>: <reason> — <intent>" for a blocked gate

use std::collections::BTreeMap;
use std::fmt::Display;

use serde::Serialize;

pub const SCHEMA_VERSION: u64 = 1;

/// Render a failed gate as `<gate_type>: <reason> — <intent>`.
///
/// A command or snapshot gate attaches its captured output to the reason,
/// below the headline (`── stderr (tail) ──`). The intent explains the
/// *headline*, so it belongs on that first line: appended to the whole reason
/// it lands under the tail and reads as one more line of the command's own
/// stderr, which is precisely the thing the tail exists to show verbatim.
// [gate-failure-line]
pub(crate) fn gate_failure_line(gate_type: &str, reason: &str, intent: Option<&str>) -> String {
    let intent = intent.unwrap_or("gate condition must be met");
    match reason.split_once('\n') {
        Some((headline, detail)) => format!(
            "{}: {} \u{2014} {}\n{}",
            gate_type, headline, intent, detail
        ),
        None => format!("{}: {} \u{2014} {}", gate_type, reason, intent),
    }
}

pub trait CommandOutput {
    fn to_json(&self) -> String;
    fn to_text(&self) -> String;
    fn exit_code(&self) -> i32;
}

pub struct CommandResult<T: Serialize + Display> {
    ok: bool,
    command: String,
    data: Option<T>,
    error: Option<ErrorData>,
    exit_code: i32,
}

#[derive(Serialize, Clone)]
pub struct ErrorData {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl<T: Serialize + Display> CommandResult<T> {
    pub fn ok(command: &str, data: T) -> Self {
        Self {
            ok: true,
            command: command.to_string(),
            data: Some(data),
            error: None,
            exit_code: 0,
        }
    }

    pub fn ok_with_exit_code(command: &str, data: T, exit_code: i32) -> Self {
        Self {
            ok: exit_code == 0,
            command: command.to_string(),
            data: Some(data),
            error: None,
            exit_code,
        }
    }

    pub fn err(command: &str, exit_code: i32, code: &str, message: String) -> Self {
        Self {
            ok: false,
            command: command.to_string(),
            data: None,
            error: Some(ErrorData {
                code: code.to_string(),
                message,
                details: None,
            }),
            exit_code,
        }
    }

    pub fn set_exit_code(&mut self, code: i32) {
        self.exit_code = code;
    }

    pub fn err_with_details(
        command: &str,
        exit_code: i32,
        code: &str,
        message: String,
        details: serde_json::Value,
    ) -> Self {
        Self {
            ok: false,
            command: command.to_string(),
            data: None,
            error: Some(ErrorData {
                code: code.to_string(),
                message,
                details: Some(details),
            }),
            exit_code,
        }
    }
}

impl<T: Serialize + Display> CommandOutput for CommandResult<T> {
    fn to_json(&self) -> String {
        let mut map = serde_json::Map::new();
        map.insert(
            "schema_version".to_string(),
            serde_json::Value::Number(SCHEMA_VERSION.into()),
        );
        map.insert("ok".to_string(), serde_json::Value::Bool(self.ok));
        map.insert(
            "command".to_string(),
            serde_json::Value::String(self.command.clone()),
        );
        if let Some(ref data) = self.data {
            map.insert(
                "data".to_string(),
                serde_json::to_value(data).unwrap_or(serde_json::Value::Null),
            );
        }
        if let Some(ref error) = self.error {
            map.insert(
                "error".to_string(),
                serde_json::to_value(error).unwrap_or(serde_json::Value::Null),
            );
        }
        serde_json::to_string(&map)
            .unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string())
    }

    fn to_text(&self) -> String {
        // Data wins over ok-ness: a command can fail *with* a report (lint
        // findings, a blocked gate check, a dirty manifest), and that report is
        // the whole point of running it.
        if let Some(ref data) = self.data {
            data.to_string()
        } else if self.ok {
            String::new()
        } else if let Some(ref error) = self.error {
            format!("error: {}\n", error.message)
        } else {
            "error: unknown\n".to_string()
        }
    }

    fn exit_code(&self) -> i32 {
        self.exit_code
    }
}

pub struct LegacyResult {
    command: String,
    exit_code: i32,
    error: Option<ErrorData>,
}

impl LegacyResult {
    pub fn new(command: &str, exit_code: i32) -> Self {
        Self {
            command: command.to_string(),
            exit_code,
            error: None,
        }
    }

    pub fn with_error(command: &str, exit_code: i32, code: &str, message: &str) -> Self {
        Self {
            command: command.to_string(),
            exit_code,
            error: Some(ErrorData {
                code: code.to_string(),
                message: message.to_string(),
                details: None,
            }),
        }
    }
}

impl CommandOutput for LegacyResult {
    fn to_json(&self) -> String {
        let mut map = serde_json::Map::new();
        map.insert(
            "schema_version".to_string(),
            serde_json::Value::Number(SCHEMA_VERSION.into()),
        );
        map.insert(
            "ok".to_string(),
            serde_json::Value::Bool(self.exit_code == 0),
        );
        map.insert(
            "command".to_string(),
            serde_json::Value::String(self.command.clone()),
        );
        if let Some(ref error) = self.error {
            map.insert(
                "error".to_string(),
                serde_json::to_value(error).unwrap_or(serde_json::Value::Null),
            );
        }
        serde_json::to_string(&map)
            .unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string())
    }

    fn to_text(&self) -> String {
        String::new()
    }

    fn exit_code(&self) -> i32 {
        self.exit_code
    }
}

// ---------------------------------------------------------------------------
// StatusData
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct StatusData {
    pub state: String,
    pub ledger_name: String,
    pub ledger_source: String,
    pub event_count: usize,
    pub chain_valid: bool,
    pub chain_error: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
    pub sets: Vec<SetSummaryData>,
    pub transitions: Vec<TransitionSummaryData>,
}

impl Display for StatusData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Ledger: {} ({})", self.ledger_name, self.ledger_source)?;
        let chain_str = if self.chain_valid {
            "chain valid".to_string()
        } else {
            format!(
                "chain INVALID ({})",
                self.chain_error.as_deref().unwrap_or("unknown")
            )
        };
        writeln!(
            f,
            "state: {} ({} events, {})",
            self.state, self.event_count, chain_str
        )?;

        for warning in &self.warnings {
            writeln!(f, "\u{26A0} {}", warning)?;
        }

        if !self.sets.is_empty() {
            writeln!(f, "sets:")?;
            for set in &self.sets {
                let members_str: Vec<String> = set
                    .members
                    .iter()
                    .map(|m| {
                        if m.done {
                            format!("\u{2713} {}", m.name)
                        } else {
                            format!("\u{00B7} {}", m.name)
                        }
                    })
                    .collect();
                writeln!(
                    f,
                    "  {}: {}/{} [{}]",
                    set.name,
                    set.completed,
                    set.total,
                    members_str.join(", ")
                )?;
            }
        }

        if !self.transitions.is_empty() {
            writeln!(f, "next:")?;
            for t in &self.transitions {
                let readiness = if t.ready { "ready" } else { "blocked" };
                writeln!(f, "  {}: {}", t.command, readiness)?;
                for g in &t.gates {
                    if g.passed {
                        writeln!(f, "    \u{2713} {}", g.description)?;
                    } else {
                        writeln!(
                            f,
                            "    \u{2717} {}",
                            gate_failure_line(
                                &g.gate_type,
                                g.reason.as_deref().unwrap_or("failed"),
                                g.intent.as_deref()
                            )
                        )?;
                    }
                }
            }
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// EventOnlyStatusData
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct EventOnlyStatusData {
    pub event_count: usize,
    pub chain_valid: bool,
    pub chain_error: Option<String>,
}

impl Display for EventOnlyStatusData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let chain_str = if self.chain_valid {
            "chain valid".to_string()
        } else {
            format!(
                "chain INVALID ({})",
                self.chain_error.as_deref().unwrap_or("unknown")
            )
        };
        write!(f, "event-only: {} events, {}", self.event_count, chain_str)
    }
}

// ---------------------------------------------------------------------------
// SetSummaryData
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct SetSummaryData {
    pub name: String,
    pub completed: usize,
    pub total: usize,
    pub members: Vec<MemberData>,
}

impl Display for SetSummaryData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let members_str: Vec<String> = self
            .members
            .iter()
            .map(|m| {
                if m.done {
                    format!("\u{2713} {}", m.name)
                } else {
                    format!("\u{00B7} {}", m.name)
                }
            })
            .collect();
        write!(
            f,
            "{}: {}/{} [{}]",
            self.name,
            self.completed,
            self.total,
            members_str.join(", ")
        )
    }
}

// ---------------------------------------------------------------------------
// MemberData
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct MemberData {
    pub name: String,
    pub done: bool,
}

impl Display for MemberData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.done {
            write!(f, "\u{2713} {}", self.name)
        } else {
            write!(f, "\u{00B7} {}", self.name)
        }
    }
}

// ---------------------------------------------------------------------------
// TransitionSummaryData
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct TransitionSummaryData {
    pub command: String,
    pub from: String,
    pub to: String,
    pub ready: bool,
    pub gates: Vec<GateResultData>,
}

impl Display for TransitionSummaryData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let readiness = if self.ready { "ready" } else { "blocked" };
        write!(f, "{}: {}", self.command, readiness)
    }
}

// ---------------------------------------------------------------------------
// GateResultData
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct GateResultData {
    pub passed: bool,
    pub evaluable: bool,
    pub gate_type: String,
    pub description: String,
    pub reason: Option<String>,
    pub intent: Option<String>,
}

impl Display for GateResultData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.passed {
            write!(f, "\u{2713} {}", self.description)
        } else if !self.evaluable {
            write!(
                f,
                "? {}: {}",
                self.gate_type,
                self.reason.as_deref().unwrap_or("unevaluable")
            )
        } else {
            write!(
                f,
                "\u{2717} {}",
                gate_failure_line(
                    &self.gate_type,
                    self.reason.as_deref().unwrap_or("failed"),
                    self.intent.as_deref()
                )
            )
        }
    }
}

// ---------------------------------------------------------------------------
// LogData
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct LogData {
    pub entries: Vec<EntryData>,
}

impl Display for LogData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for entry in &self.entries {
            write!(
                f,
                "[{}] seq={} type={} hash={}",
                entry.timestamp,
                entry.seq,
                entry.event_type,
                &entry.hash[..12],
            )?;
            if !entry.fields.is_empty() {
                let pairs: Vec<String> = entry
                    .fields
                    .iter()
                    .map(|(k, v)| format!("{}={}", k, v))
                    .collect();
                write!(f, " {{{}}}", pairs.join(", "))?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// EntryData
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct EntryData {
    pub seq: u64,
    pub timestamp: String,
    pub event_type: String,
    pub hash: String,
    pub fields: BTreeMap<String, String>,
}

impl Display for EntryData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{}] seq={} type={} hash={}",
            self.timestamp,
            self.seq,
            self.event_type,
            &self.hash[..12],
        )?;
        if !self.fields.is_empty() {
            let pairs: Vec<String> = self
                .fields
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            write!(f, " {{{}}}", pairs.join(", "))?;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// GateCheckData
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct GateCheckData {
    pub transition: String,
    pub current_state: String,
    pub candidates: Vec<CandidateData>,
    pub result: String,
    pub would_take: Option<String>,
}

impl Display for GateCheckData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "gate-check: {}", self.transition)?;

        let multi = self.candidates.len() > 1;
        for (idx, candidate) in self.candidates.iter().enumerate() {
            if multi {
                writeln!(
                    f,
                    "candidate {}: {} \u{2192} {}",
                    idx + 1,
                    candidate.from,
                    candidate.to
                )?;
            }

            if candidate.gates.is_empty() {
                if multi {
                    writeln!(f, "  (no gates \u{2014} always passes)")?;
                } else {
                    writeln!(f, "result: ready (no gates)")?;
                    return Ok(());
                }
            }

            for gate in &candidate.gates {
                writeln!(f, "  {}", gate)?;
            }
        }

        if multi {
            if let Some(ref target) = self.would_take {
                writeln!(f, "result: would take \u{2192} {}", target)?;
            } else {
                writeln!(f, "result: blocked")?;
            }
        } else {
            writeln!(f, "result: {}", self.result)?;
        }

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CandidateData
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct CandidateData {
    pub from: String,
    pub to: String,
    pub gates: Vec<GateResultData>,
    pub all_passed: bool,
}

impl Display for CandidateData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} \u{2192} {} ({})",
            self.from,
            self.to,
            if self.all_passed { "ready" } else { "blocked" }
        )
    }
}

// ---------------------------------------------------------------------------
// ManifestVerifyData
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ManifestVerifyData {
    pub clean: bool,
    pub tracked_count: usize,
    pub mismatches: Vec<MismatchData>,
    /// Entries keyed outside `managed_paths`. Reported, never counted — see
    /// `manifest::verify::MismatchKind::Unmanaged`.
    #[serde(default)]
    pub unmanaged: Vec<MismatchData>,
}

impl Display for ManifestVerifyData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.clean {
            writeln!(f, "manifest clean ({} tracked)", self.tracked_count)?;
        } else {
            let modified = self
                .mismatches
                .iter()
                .filter(|m| m.kind == "modified")
                .count();
            let missing = self
                .mismatches
                .iter()
                .filter(|m| m.kind == "missing")
                .count();
            writeln!(f, "manifest: {} modified, {} missing", modified, missing)?;
            for m in &self.mismatches {
                writeln!(f, "  {}", m)?;
            }
        }

        if !self.unmanaged.is_empty() {
            writeln!(
                f,
                "manifest: {} entr{} outside managed paths \u{2014} not an integrity failure; \
                 a key computed against the wrong directory (holtz #85)",
                self.unmanaged.len(),
                if self.unmanaged.len() == 1 {
                    "y"
                } else {
                    "ies"
                }
            )?;
            for m in &self.unmanaged {
                writeln!(f, "  {}", m)?;
            }
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// MismatchData
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct MismatchData {
    pub path: String,
    pub expected: String,
    pub actual: Option<String>,
    /// `modified`, `missing`, or `unmanaged`. Consumers key their message off
    /// this rather than inferring "deleted" from a null hash, which cannot tell
    /// a removed file from one that never existed.
    pub kind: String,
}

impl Display for MismatchData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let detail = match (self.kind.as_str(), &self.actual) {
            ("unmanaged", _) => "not under any managed path".to_string(),
            ("missing", _) => "file not found".to_string(),
            (_, Some(h)) => format!("got {}", &h[..12]),
            (_, None) => "file not found".to_string(),
        };
        write!(
            f,
            "{} \u{2014} {}: expected {}, {}",
            self.path,
            self.kind,
            &self.expected[..12],
            detail
        )
    }
}

// ---------------------------------------------------------------------------
// HookEvalData
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Clone)]
pub struct HookEvalData {
    pub decision: String,
    pub messages: Vec<HookEvalMessage>,
    pub auto_records: Vec<HookAutoRecord>,
    pub monitor_warnings: Vec<HookMonitorWarning>,
}

impl Display for HookEvalData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_string_pretty(self).unwrap_or_default()
        )
    }
}

// ---------------------------------------------------------------------------
// HookEvalMessage
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Clone)]
pub struct HookEvalMessage {
    pub source: String,
    pub rule_index: usize,
    pub action: String,
    pub message: String,
}

// ---------------------------------------------------------------------------
// HookAutoRecord
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Clone)]
pub struct HookAutoRecord {
    pub event_type: String,
    pub fields: std::collections::HashMap<String, String>,
}

// ---------------------------------------------------------------------------
// HookMonitorWarning
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize, Clone)]
pub struct HookMonitorWarning {
    pub name: String,
    pub message: String,
}

// ---------------------------------------------------------------------------
// LedgerActivateData
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct LedgerActivateData {
    pub activated: String,
}

impl Display for LedgerActivateData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `main` prints command text with `print!`, so a top-level command's
        // output terminates its own line or the next thing written runs into it.
        writeln!(f, "Activated ledger: {}", self.activated)
    }
}

// ---------------------------------------------------------------------------
// LedgerDeactivateData
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct LedgerDeactivateData {
    pub deactivated: bool,
}

impl Display for LedgerDeactivateData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.deactivated {
            writeln!(f, "Deactivated active ledger")
        } else {
            writeln!(f, "No active ledger to deactivate")
        }
    }
}

// ---------------------------------------------------------------------------
// LintData
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct LintData {
    /// Findings, most structural first (ordered by check id then location).
    pub findings: Vec<crate::lint::LintFinding>,
    pub error_count: usize,
    pub warning_count: usize,
    /// Check ids that actually ran, so a caller can tell "clean" from "skipped".
    pub checks_run: Vec<String>,
}

impl Display for LintData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for finding in &self.findings {
            writeln!(
                f,
                "{} {}: {}",
                finding.check,
                finding.severity.as_str(),
                finding.location
            )?;
            writeln!(f, "    {}", finding.message)?;
            if let Some(ref hint) = finding.hint {
                writeln!(f, "    hint: {}", hint)?;
            }
        }

        if self.findings.is_empty() {
            writeln!(
                f,
                "clean. {} check(s) run: {}",
                self.checks_run.len(),
                self.checks_run.join(", ")
            )
        } else {
            writeln!(
                f,
                "{} error(s), {} warning(s) from {} check(s): {}",
                self.error_count,
                self.warning_count,
                self.checks_run.len(),
                self.checks_run.join(", ")
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::gate_failure_line;

    #[test]
    fn intent_follows_a_single_line_reason() {
        assert_eq!(
            gate_failure_line(
                "file_exists",
                "file 'repro.log' not found",
                Some("the repro has to be on disk")
            ),
            "file_exists: file 'repro.log' not found \u{2014} the repro has to be on disk"
        );
    }

    #[test]
    fn intent_stays_on_the_headline_above_captured_output() {
        let reason = "command 'cargo test' exited with status 101\n\u{2500}\u{2500} stderr (tail) \u{2500}\u{2500}\nerror: test failed";
        assert_eq!(
            gate_failure_line("command_succeeds", reason, Some("the suite has to be green")),
            "command_succeeds: command 'cargo test' exited with status 101 \u{2014} the suite has to be green\n\u{2500}\u{2500} stderr (tail) \u{2500}\u{2500}\nerror: test failed"
        );
    }

    #[test]
    fn missing_intent_falls_back() {
        assert_eq!(
            gate_failure_line("query", "expected true, got false", None),
            "query: expected true, got false \u{2014} gate condition must be met"
        );
    }
}
