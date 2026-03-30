# Plugin Phase 1: --json Output Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add `--json` flag to 6 CLI commands with versioned envelope output, refactor to data-first architecture, create HORIZONS-1 example protocol.

**Architecture:** Each command builds a typed data struct. A `CommandOutput` trait enables type-erased dispatch in `main.rs`. `CommandResult<T>` wraps data in a JSON envelope with `schema_version`, `ok`, `command`. Text output is produced by `Display` impls that match current behavior exactly.

**Tech Stack:** Rust, serde/serde_json for serialization, clap for `--json` flag, assert_cmd for CLI integration tests.

---

## File Map

| File | Action | Responsibility |
|------|--------|---------------|
| `src/cli/output.rs` | Create | CommandOutput trait, CommandResult<T>, ErrorData, all data structs, Display impls |
| `src/cli/mod.rs` | Modify | Add `pub mod output;` |
| `src/main.rs` | Modify | Add `--json` global flag, refactor dispatch to Box<dyn CommandOutput> |
| `src/cli/status.rs` | Modify | Return Box<dyn CommandOutput> instead of i32 |
| `src/cli/log.rs` | Modify | Return Box<dyn CommandOutput> instead of i32 |
| `src/cli/transition.rs` | Modify | `cmd_gate_check` returns Box<dyn CommandOutput> |
| `src/cli/manifest_cmd.rs` | Modify | `cmd_manifest_verify` returns Box<dyn CommandOutput> |
| `examples/horizons1/protocol.toml` | Create | Mission control protocol config |
| `examples/horizons1/states.toml` | Create | 9 mission phase states + anomaly |
| `examples/horizons1/transitions.toml` | Create | Phase transitions with placeholder gates |
| `examples/horizons1/events.toml` | Create | Mission event type definitions |
| `examples/horizons1/renders.toml` | Create | Empty renders (Phase 5) |
| `tests/json_output_tests.rs` | Create | Envelope serialization + per-command JSON integration tests |
| `tests/horizons1_tests.rs` | Create | HORIZONS-1 protocol integration tests |

---

### Task 1: Core output types — CommandOutput trait and CommandResult

**Files:**
- Create: `src/cli/output.rs`
- Modify: `src/cli/mod.rs`

- [ ] **Step 1: Write the test for envelope serialization**

Create `tests/json_output_tests.rs`:

```rust
// tests/json_output_tests.rs
//
// Tests for JSON envelope output.

use serde_json::Value;

/// Parse JSON output string and return the parsed value.
fn parse_envelope(json_str: &str) -> Value {
    serde_json::from_str(json_str).expect("valid JSON")
}

#[test]
fn test_ok_envelope_has_schema_version() {
    use sahjhan::cli::output::{CommandResult, CommandOutput};
    let result = CommandResult::ok("status", "test_data".to_string());
    let json_str = result.to_json();
    let v = parse_envelope(&json_str);
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["ok"], true);
    assert_eq!(v["command"], "status");
    assert_eq!(v["data"], "test_data");
    assert!(v.get("error").is_none() || v["error"].is_null());
}

#[test]
fn test_err_envelope_has_error_fields() {
    use sahjhan::cli::output::{CommandResult, CommandOutput};
    let result: CommandResult<String> = CommandResult::err("status", 2, "integrity_error", "chain invalid".to_string());
    let json_str = result.to_json();
    let v = parse_envelope(&json_str);
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["ok"], false);
    assert_eq!(v["command"], "status");
    assert!(v.get("data").is_none() || v["data"].is_null());
    assert_eq!(v["error"]["code"], "integrity_error");
    assert_eq!(v["error"]["message"], "chain invalid");
}

#[test]
fn test_err_with_details_envelope() {
    use sahjhan::cli::output::{CommandResult, CommandOutput};
    let details = serde_json::json!({"gate": "file_exists", "path": "/missing"});
    let result: CommandResult<String> = CommandResult::err_with_details(
        "transition", 1, "gate_blocked", "gate failed".to_string(), details.clone(),
    );
    let json_str = result.to_json();
    let v = parse_envelope(&json_str);
    assert_eq!(v["error"]["details"]["gate"], "file_exists");
}

#[test]
fn test_ok_text_output() {
    use sahjhan::cli::output::{CommandResult, CommandOutput};
    let result = CommandResult::ok("test", "hello world".to_string());
    assert_eq!(result.to_text(), "hello world");
}

#[test]
fn test_err_text_output() {
    use sahjhan::cli::output::{CommandResult, CommandOutput};
    let result: CommandResult<String> = CommandResult::err("test", 2, "integrity_error", "chain invalid".to_string());
    assert_eq!(result.to_text(), "error: chain invalid\n");
}

#[test]
fn test_exit_codes() {
    use sahjhan::cli::output::{CommandResult, CommandOutput};
    let ok: CommandResult<String> = CommandResult::ok("test", "data".to_string());
    assert_eq!(ok.exit_code(), 0);
    let err: CommandResult<String> = CommandResult::err("test", 2, "integrity_error", "bad".to_string());
    assert_eq!(err.exit_code(), 2);
}

#[test]
fn test_legacy_result_json() {
    use sahjhan::cli::output::{LegacyResult, CommandOutput};
    let legacy = LegacyResult::new("init", 0);
    let v = parse_envelope(&legacy.to_json());
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["ok"], true);
    assert_eq!(v["command"], "init");
}

#[test]
fn test_legacy_result_error_json() {
    use sahjhan::cli::output::{LegacyResult, CommandOutput};
    let legacy = LegacyResult::with_error("init", 3, "config_error", "missing file");
    let v = parse_envelope(&legacy.to_json());
    assert_eq!(v["ok"], false);
    assert_eq!(v["error"]["code"], "config_error");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --test json_output_tests`
Expected: Compile errors — `sahjhan::cli::output` doesn't exist yet.

- [ ] **Step 3: Create output.rs with core types**

Create `src/cli/output.rs`:

```rust
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

use std::fmt::Display;

use serde::Serialize;

/// Current output schema version. Additive changes don't bump;
/// removals or renames of existing fields do.
pub const SCHEMA_VERSION: u64 = 1;

/// Trait for type-erased command dispatch.
///
/// Implemented by `CommandResult<T>` (for converted commands) and
/// `LegacyResult` (for commands still returning i32).
pub trait CommandOutput {
    /// Serialize to JSON envelope string.
    fn to_json(&self) -> String;
    /// Format as human-readable text (success to stdout, errors to stderr style).
    fn to_text(&self) -> String;
    /// Process exit code.
    fn exit_code(&self) -> i32;
}

/// Typed command result carrying either data or an error.
pub struct CommandResult<T: Serialize + Display> {
    ok: bool,
    command: String,
    data: Option<T>,
    error: Option<ErrorData>,
    exit_code: i32,
}

/// Structured error information.
#[derive(Serialize, Clone)]
pub struct ErrorData {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl<T: Serialize + Display> CommandResult<T> {
    /// Create a success result.
    pub fn ok(command: &str, data: T) -> Self {
        Self {
            ok: true,
            command: command.to_string(),
            data: Some(data),
            error: None,
            exit_code: 0,
        }
    }

    /// Create an error result.
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

    /// Create an error result with structured details.
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
        serde_json::to_string(&map).unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string())
    }

    fn to_text(&self) -> String {
        if self.ok {
            if let Some(ref data) = self.data {
                data.to_string()
            } else {
                String::new()
            }
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

/// Shim for commands not yet converted to data-first.
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
        serde_json::to_string(&map).unwrap_or_else(|_| r#"{"error":"serialization failed"}"#.to_string())
    }

    fn to_text(&self) -> String {
        // Legacy commands handle their own printing; return empty.
        String::new()
    }

    fn exit_code(&self) -> i32 {
        self.exit_code
    }
}
```

- [ ] **Step 4: Add module declaration**

In `src/cli/mod.rs`, add after the existing modules:

```rust
pub mod output;
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --test json_output_tests`
Expected: All 9 tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/cli/output.rs src/cli/mod.rs tests/json_output_tests.rs
git commit -m "feat: add CommandResult envelope and CommandOutput trait for --json support"
```

---

### Task 2: Data structs for all 6 commands

**Files:**
- Modify: `src/cli/output.rs`
- Modify: `tests/json_output_tests.rs`

- [ ] **Step 1: Write tests for data struct serialization**

Append to `tests/json_output_tests.rs`:

```rust
#[test]
fn test_status_data_json_fields() {
    use sahjhan::cli::output::*;
    let data = StatusData {
        state: "idle".to_string(),
        event_count: 1,
        chain_valid: true,
        chain_error: None,
        sets: vec![SetSummaryData {
            name: "check".to_string(),
            completed: 1,
            total: 2,
            members: vec![
                MemberData { name: "tests".to_string(), done: true },
                MemberData { name: "lint".to_string(), done: false },
            ],
        }],
        transitions: vec![TransitionSummaryData {
            command: "begin".to_string(),
            from: "idle".to_string(),
            to: "working".to_string(),
            ready: true,
            gates: vec![],
        }],
    };
    let result = CommandResult::ok("status", data);
    let v = parse_envelope(&result.to_json());
    assert_eq!(v["data"]["state"], "idle");
    assert_eq!(v["data"]["event_count"], 1);
    assert_eq!(v["data"]["chain_valid"], true);
    assert_eq!(v["data"]["sets"][0]["name"], "check");
    assert_eq!(v["data"]["sets"][0]["completed"], 1);
    assert_eq!(v["data"]["sets"][0]["members"][0]["done"], true);
    assert_eq!(v["data"]["transitions"][0]["command"], "begin");
    assert_eq!(v["data"]["transitions"][0]["ready"], true);
}

#[test]
fn test_status_data_text_matches_current_format() {
    use sahjhan::cli::output::*;
    let data = StatusData {
        state: "idle".to_string(),
        event_count: 1,
        chain_valid: true,
        chain_error: None,
        sets: vec![SetSummaryData {
            name: "check".to_string(),
            completed: 1,
            total: 2,
            members: vec![
                MemberData { name: "tests".to_string(), done: true },
                MemberData { name: "lint".to_string(), done: false },
            ],
        }],
        transitions: vec![TransitionSummaryData {
            command: "begin".to_string(),
            from: "idle".to_string(),
            to: "working".to_string(),
            ready: true,
            gates: vec![],
        }],
    };
    let text = data.to_string();
    assert!(text.contains("state: idle (1 events, chain valid)"));
    assert!(text.contains("check: 1/2"));
    assert!(text.contains("\u{2713} tests"));
    assert!(text.contains("\u{00B7} lint"));
    assert!(text.contains("begin: ready"));
}

#[test]
fn test_log_data_json_has_full_hashes() {
    use std::collections::BTreeMap;
    use sahjhan::cli::output::*;
    let mut fields = BTreeMap::new();
    fields.insert("from".to_string(), "idle".to_string());
    let data = LogData {
        entries: vec![EntryData {
            seq: 0,
            timestamp: "2026-03-30T00:00:00.000Z".to_string(),
            event_type: "genesis".to_string(),
            hash: "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string(),
            fields,
        }],
    };
    let result = CommandResult::ok("log_dump", data);
    let v = parse_envelope(&result.to_json());
    // JSON gets full 64-char hash
    assert_eq!(v["data"]["entries"][0]["hash"].as_str().unwrap().len(), 64);
}

#[test]
fn test_log_data_text_truncates_hashes() {
    use std::collections::BTreeMap;
    use sahjhan::cli::output::*;
    let data = LogData {
        entries: vec![EntryData {
            seq: 0,
            timestamp: "2026-03-30T00:00:00.000Z".to_string(),
            event_type: "genesis".to_string(),
            hash: "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string(),
            fields: BTreeMap::new(),
        }],
    };
    let text = data.to_string();
    // Text gets 12-char truncated hash
    assert!(text.contains("hash=abcdef123456"));
    assert!(!text.contains("abcdef1234567890abcdef1234567890"));
}

#[test]
fn test_gate_check_data_json() {
    use sahjhan::cli::output::*;
    let data = GateCheckData {
        transition: "begin".to_string(),
        current_state: "idle".to_string(),
        candidates: vec![CandidateData {
            from: "idle".to_string(),
            to: "working".to_string(),
            gates: vec![],
            all_passed: true,
        }],
        result: "ready".to_string(),
        would_take: Some("working".to_string()),
    };
    let result = CommandResult::ok("gate_check", data);
    let v = parse_envelope(&result.to_json());
    assert_eq!(v["data"]["transition"], "begin");
    assert_eq!(v["data"]["candidates"][0]["all_passed"], true);
    assert_eq!(v["data"]["would_take"], "working");
}

#[test]
fn test_manifest_verify_data_json() {
    use sahjhan::cli::output::*;
    let data = ManifestVerifyData {
        clean: false,
        tracked_count: 3,
        mismatches: vec![MismatchData {
            path: "output/STATUS.md".to_string(),
            expected: "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string(),
            actual: Some("1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string()),
        }],
    };
    let result = CommandResult::ok("manifest_verify", data);
    let v = parse_envelope(&result.to_json());
    assert_eq!(v["data"]["clean"], false);
    assert_eq!(v["data"]["tracked_count"], 3);
    assert_eq!(v["data"]["mismatches"][0]["path"], "output/STATUS.md");
    // JSON has full hashes
    assert_eq!(v["data"]["mismatches"][0]["expected"].as_str().unwrap().len(), 64);
}

#[test]
fn test_event_only_status_data_json() {
    use sahjhan::cli::output::*;
    let data = EventOnlyStatusData {
        event_count: 42,
        chain_valid: true,
        chain_error: None,
    };
    let result = CommandResult::ok("status", data);
    let v = parse_envelope(&result.to_json());
    assert_eq!(v["data"]["event_count"], 42);
    assert_eq!(v["data"]["chain_valid"], true);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --test json_output_tests`
Expected: Compile errors — data structs don't exist yet.

- [ ] **Step 3: Add all data structs to output.rs**

Append to `src/cli/output.rs` after the `LegacyResult` impl block:

```rust
// ---------------------------------------------------------------------------
// Per-command data structs
// ---------------------------------------------------------------------------

/// Status command output.
#[derive(Serialize)]
pub struct StatusData {
    pub state: String,
    pub event_count: u64,
    pub chain_valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_error: Option<String>,
    pub sets: Vec<SetSummaryData>,
    pub transitions: Vec<TransitionSummaryData>,
}

/// Event-only ledger status output.
#[derive(Serialize)]
pub struct EventOnlyStatusData {
    pub event_count: u64,
    pub chain_valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub chain_error: Option<String>,
}

/// Set summary (used in status and set-status).
#[derive(Serialize)]
pub struct SetSummaryData {
    pub name: String,
    pub completed: usize,
    pub total: usize,
    pub members: Vec<MemberData>,
}

/// Individual set member status.
#[derive(Serialize)]
pub struct MemberData {
    pub name: String,
    pub done: bool,
}

/// Transition summary for status output.
#[derive(Serialize)]
pub struct TransitionSummaryData {
    pub command: String,
    pub from: String,
    pub to: String,
    pub ready: bool,
    pub gates: Vec<GateResultData>,
}

/// Gate evaluation result.
#[derive(Serialize)]
pub struct GateResultData {
    pub gate_type: String,
    pub passed: bool,
    pub evaluable: bool,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub intent: Option<String>,
}

/// Log dump / log tail output.
#[derive(Serialize)]
pub struct LogData {
    pub entries: Vec<EntryData>,
}

/// Single ledger entry.
#[derive(Serialize)]
pub struct EntryData {
    pub seq: u64,
    pub timestamp: String,
    pub event_type: String,
    pub hash: String,
    pub fields: std::collections::BTreeMap<String, String>,
}

/// Gate check (dry-run) output.
#[derive(Serialize)]
pub struct GateCheckData {
    pub transition: String,
    pub current_state: String,
    pub candidates: Vec<CandidateData>,
    pub result: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub would_take: Option<String>,
}

/// Single transition candidate in gate check.
#[derive(Serialize)]
pub struct CandidateData {
    pub from: String,
    pub to: String,
    pub gates: Vec<GateResultData>,
    pub all_passed: bool,
}

/// Manifest verify output.
#[derive(Serialize)]
pub struct ManifestVerifyData {
    pub clean: bool,
    pub tracked_count: usize,
    pub mismatches: Vec<MismatchData>,
}

/// Single manifest mismatch.
#[derive(Serialize)]
pub struct MismatchData {
    pub path: String,
    pub expected: String,
    pub actual: Option<String>,
}

// ---------------------------------------------------------------------------
// Display impls — reproduce current text output exactly
// ---------------------------------------------------------------------------

impl Display for StatusData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let chain = if self.chain_valid {
            "chain valid".to_string()
        } else {
            format!("chain INVALID ({})", self.chain_error.as_deref().unwrap_or("unknown"))
        };
        writeln!(f, "state: {} ({} events, {})", self.state, self.event_count, chain)?;

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
                    set.name, set.completed, set.total, members_str.join(", ")
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
                        let intent = g.intent.as_deref().unwrap_or("gate condition must be met");
                        writeln!(
                            f,
                            "    \u{2717} {}: {} \u{2014} {}",
                            g.gate_type,
                            g.reason.as_deref().unwrap_or("failed"),
                            intent
                        )?;
                    }
                }
            }
        }

        Ok(())
    }
}

impl Display for EventOnlyStatusData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let chain = if self.chain_valid {
            "chain valid".to_string()
        } else {
            format!("chain INVALID ({})", self.chain_error.as_deref().unwrap_or("unknown"))
        };
        writeln!(f, "event-only: {} events, {}", self.event_count, chain)
    }
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
        writeln!(
            f,
            "{}: {}/{} [{}]",
            self.name, self.completed, self.total, members_str.join(", ")
        )
    }
}

impl Display for LogData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        for entry in &self.entries {
            write!(
                f,
                "[{}] seq={} type={} hash={}",
                entry.timestamp, entry.seq, entry.event_type, &entry.hash[..12],
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
                continue;
            }

            for g in &candidate.gates {
                if g.passed {
                    writeln!(f, "  \u{2713} {}", g.description)?;
                } else if !g.evaluable {
                    writeln!(
                        f,
                        "  ? {}: {}",
                        g.gate_type,
                        g.reason.as_deref().unwrap_or("unevaluable"),
                    )?;
                } else {
                    writeln!(
                        f,
                        "  \u{2717} {}: {} \u{2014} {}",
                        g.gate_type,
                        g.reason.as_deref().unwrap_or("failed"),
                        g.intent.as_deref().unwrap_or("gate condition must be met")
                    )?;
                }
            }
        }

        if multi {
            if let Some(ref target) = self.would_take {
                writeln!(f, "result: would take \u{2192} {}", target)?;
            } else {
                writeln!(f, "result: blocked")?;
            }
        } else {
            if self.would_take.is_some() {
                writeln!(f, "result: ready")?;
            } else {
                writeln!(f, "result: blocked")?;
            }
        }
        Ok(())
    }
}

impl Display for ManifestVerifyData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.clean {
            writeln!(f, "manifest clean ({} tracked)", self.tracked_count)
        } else {
            writeln!(f, "manifest: {} modified", self.mismatches.len())?;
            for m in &self.mismatches {
                let actual_str = match &m.actual {
                    Some(h) => format!("got {}", &h[..12]),
                    None => "missing".to_string(),
                };
                writeln!(
                    f,
                    "  {} \u{2014} expected {}, {}",
                    m.path, &m.expected[..12], actual_str
                )?;
            }
            Ok(())
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --test json_output_tests`
Expected: All tests pass (original 9 + new 8 = 17).

- [ ] **Step 5: Commit**

```bash
git add src/cli/output.rs tests/json_output_tests.rs
git commit -m "feat: add per-command data structs with Serialize and Display impls"
```

---

### Task 3: Wire --json flag and dispatch in main.rs

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: Add --json global flag to Cli struct**

In `src/main.rs`, add to the `Cli` struct after the `ledger_path` field (line 51):

```rust
    /// Output JSON instead of text
    #[arg(long, global = true)]
    json: bool,
```

- [ ] **Step 2: Refactor main() dispatch to use CommandOutput**

Replace the `let exit_code = match cli.command { ... };` block and the `std::process::exit(exit_code);` at the end of main() with the new dispatch pattern. The 6 converted commands return `Box<dyn CommandOutput>`. All other commands stay as `i32` wrapped in `LegacyResult`.

The full replacement for lines 383-507 of main.rs:

```rust
    use sahjhan::cli::output::{CommandOutput, LegacyResult};

    let result: Box<dyn CommandOutput> = match cli.command {
        Commands::Status => status::cmd_status(&cli.config_dir, &targeting),
        Commands::Log { action } => match action {
            LogAction::Dump => log::cmd_log_dump(&cli.config_dir, &targeting),
            LogAction::Verify => {
                let code = log::cmd_log_verify(&cli.config_dir, &targeting);
                Box::new(LegacyResult::new("log_verify", code))
            }
            LogAction::Tail { n } => log::cmd_log_tail(&cli.config_dir, n, &targeting),
        },
        Commands::Manifest { action } => match action {
            ManifestAction::Verify => manifest_cmd::cmd_manifest_verify(&cli.config_dir),
            ManifestAction::List => {
                let code = manifest_cmd::cmd_manifest_list(&cli.config_dir);
                Box::new(LegacyResult::new("manifest_list", code))
            }
            ManifestAction::Restore { path } => {
                let code = manifest_cmd::cmd_manifest_restore(&cli.config_dir, &path);
                Box::new(LegacyResult::new("manifest_restore", code))
            }
        },
        Commands::Set { action } => match action {
            SetAction::Status { set } => status::cmd_set_status(&cli.config_dir, &set, &targeting),
            SetAction::Complete { set, member } => {
                let code =
                    status::cmd_set_complete(&cli.config_dir, &set, &member, &targeting);
                Box::new(LegacyResult::new("set_complete", code))
            }
        },
        Commands::Gate { action } => match action {
            GateAction::Check { transition, args } => {
                transition::cmd_gate_check(&cli.config_dir, &transition, &args, &targeting)
            }
        },
        Commands::Validate => {
            let code = init::cmd_validate(&cli.config_dir);
            Box::new(LegacyResult::new("validate", code))
        }
        Commands::Init => {
            let code = init::cmd_init(&cli.config_dir);
            Box::new(LegacyResult::new("init", code))
        }
        Commands::Render { dump_context } => {
            let code = if dump_context {
                render::cmd_render_dump_context(&cli.config_dir, &targeting)
            } else {
                render::cmd_render(&cli.config_dir, &targeting)
            };
            Box::new(LegacyResult::new("render", code))
        }
        Commands::Transition { name, args } => {
            let code =
                transition::cmd_transition(&cli.config_dir, &name, &args, &targeting);
            Box::new(LegacyResult::new("transition", code))
        }
        Commands::Event { event_type, fields } => {
            let code =
                transition::cmd_event(&cli.config_dir, &event_type, &fields, &targeting);
            Box::new(LegacyResult::new("event", code))
        }
        Commands::AuthedEvent {
            event_type,
            fields,
            proof,
        } => {
            let code = authed_event::cmd_authed_event(
                &cli.config_dir,
                &event_type,
                &fields,
                &proof,
                &targeting,
            );
            Box::new(LegacyResult::new("authed_event", code))
        }
        Commands::Reseal { proof } => {
            let code = authed_event::cmd_reseal(&cli.config_dir, &proof, &targeting);
            Box::new(LegacyResult::new("reseal", code))
        }
        Commands::Reset { confirm, token } => {
            let code = init::cmd_reset(&cli.config_dir, confirm, &token);
            Box::new(LegacyResult::new("reset", code))
        }
        Commands::Hook { action } => match action {
            HookAction::Generate {
                harness,
                output_dir,
            } => {
                let code =
                    hooks_cmd::cmd_hook_generate(&cli.config_dir, &harness, &output_dir);
                Box::new(LegacyResult::new("hook_generate", code))
            }
        },
        Commands::Ledger { action } => match action {
            LedgerAction::Create {
                name,
                path,
                from,
                instance_id,
                mode,
            } => {
                let code = ledger::cmd_ledger_create(
                    &cli.config_dir,
                    name.as_deref(),
                    path.as_deref(),
                    from.as_deref(),
                    instance_id.as_deref(),
                    &mode,
                );
                Box::new(LegacyResult::new("ledger_create", code))
            }
            LedgerAction::List => {
                let code = ledger::cmd_ledger_list(&cli.config_dir);
                Box::new(LegacyResult::new("ledger_list", code))
            }
            LedgerAction::Remove { name } => {
                let code = ledger::cmd_ledger_remove(&cli.config_dir, &name);
                Box::new(LegacyResult::new("ledger_remove", code))
            }
            LedgerAction::Verify { name, path } => {
                let code =
                    ledger::cmd_ledger_verify(&cli.config_dir, name.as_deref(), path.as_deref());
                Box::new(LegacyResult::new("ledger_verify", code))
            }
            LedgerAction::Checkpoint {
                name,
                scope,
                snapshot,
            } => {
                let code =
                    ledger::cmd_ledger_checkpoint(&cli.config_dir, &name, &scope, &snapshot);
                Box::new(LegacyResult::new("ledger_checkpoint", code))
            }
            LedgerAction::Import { name, path } => {
                let code = ledger::cmd_ledger_import(&cli.config_dir, &name, &path);
                Box::new(LegacyResult::new("ledger_import", code))
            }
        },
        Commands::Config { action } => match action {
            ConfigAction::SessionKeyPath => {
                let code = config_cmd::cmd_session_key_path(&cli.config_dir, &targeting);
                Box::new(LegacyResult::new("config_session_key_path", code))
            }
        },
        Commands::Guards => {
            let code = guards::cmd_guards(&cli.config_dir);
            Box::new(LegacyResult::new("guards", code))
        }
        Commands::Mermaid { rendered } => {
            let code = mermaid_cmd::cmd_mermaid(&cli.config_dir, rendered);
            Box::new(LegacyResult::new("mermaid", code))
        }
        Commands::Query {
            sql,
            query_path,
            glob,
            event_type,
            fields,
            count,
            format,
            json,
        } => {
            let effective_format = if json { "json".to_string() } else { format };
            let query_targeting = commands::LedgerTargeting {
                ledger_name: cli.ledger,
                ledger_path: query_path.or(cli.ledger_path),
            };
            let code = query::cmd_query(
                &cli.config_dir,
                sql.as_deref(),
                &query_targeting,
                glob.as_deref(),
                event_type.as_deref(),
                &fields,
                count,
                &effective_format,
            );
            Box::new(LegacyResult::new("query", code))
        }
    };

    if cli.json {
        println!("{}", result.to_json());
    } else {
        let text = result.to_text();
        if !text.is_empty() {
            if result.exit_code() == 0 {
                print!("{}", text);
            } else {
                eprint!("{}", text);
            }
        }
    }
    std::process::exit(result.exit_code());
```

Note: the Query command's local `json` field conflicts with `cli.json`. Since `cli.json` is consumed before this match arm, and this arm moves `cli.ledger`/`cli.ledger_path`, destructure carefully. The existing `json` field on Query is the `--json` shortcut for `--format json` — distinct from the global `--json` envelope flag. No conflict because `cli.json` is read before the match consumes `cli`.

- [ ] **Step 3: Verify compilation**

Run: `cargo build`
Expected: Compile errors because `cmd_status`, `cmd_log_dump`, etc. still return `i32` not `Box<dyn CommandOutput>`. This is expected — Tasks 4-7 convert them. For now, temporarily keep the old signatures and wrap with LegacyResult to get a compiling state.

Replace the Status, Log::Dump, Log::Tail, Manifest::Verify, Set::Status, and Gate::Check arms with LegacyResult wrappers temporarily:

```rust
        Commands::Status => {
            let code = status::cmd_status(&cli.config_dir, &targeting);
            Box::new(LegacyResult::new("status", code))
        }
```

(and similarly for the other 5)

- [ ] **Step 4: Run full test suite**

Run: `cargo test`
Expected: All existing tests pass (no behavior change yet). The `--json` flag exists but no command uses it yet.

- [ ] **Step 5: Commit**

```bash
git add src/main.rs
git commit -m "feat: wire --json global flag and CommandOutput dispatch in main.rs"
```

---

### Task 4: Convert status commands (cmd_status, cmd_set_status)

**Files:**
- Modify: `src/cli/status.rs`
- Modify: `src/main.rs` (unwrap LegacyResult for Status and Set::Status)

- [ ] **Step 1: Write CLI integration tests for status --json**

Append to `tests/json_output_tests.rs`:

```rust
use assert_cmd::Command;
use tempfile::tempdir;

/// Create a temp directory with the minimal example config and run `init`.
fn setup_minimal() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();
    for file in &[
        "protocol.toml",
        "states.toml",
        "transitions.toml",
        "events.toml",
        "renders.toml",
    ] {
        std::fs::copy(format!("examples/minimal/{}", file), config_dir.join(file)).unwrap();
    }
    let templates_dir = config_dir.join("templates");
    std::fs::create_dir_all(&templates_dir).unwrap();
    for file in &["status.md.tera", "history.md.tera"] {
        std::fs::copy(
            format!("examples/minimal/templates/{}", file),
            templates_dir.join(file),
        )
        .unwrap();
    }
    std::fs::create_dir_all(dir.path().join("output")).unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "init"])
        .current_dir(dir.path())
        .assert()
        .success();

    dir
}

#[test]
fn test_cli_status_json_envelope() {
    let dir = setup_minimal();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["schema_version"], 1);
    assert_eq!(v["ok"], true);
    assert_eq!(v["command"], "status");
    assert_eq!(v["data"]["state"], "idle");
    assert!(v["data"]["event_count"].as_u64().unwrap() >= 1);
    assert_eq!(v["data"]["chain_valid"], true);
}

#[test]
fn test_cli_status_text_unchanged() {
    let dir = setup_minimal();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("state: idle"));
    assert!(stdout.contains("chain valid"));
}

#[test]
fn test_cli_set_status_json() {
    let dir = setup_minimal();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "set", "status", "check"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["data"]["name"], "check");
    assert_eq!(v["data"]["total"], 2);
    assert_eq!(v["data"]["completed"], 0);
}
```

- [ ] **Step 2: Convert cmd_status to return Box<dyn CommandOutput>**

Rewrite `src/cli/status.rs` `cmd_status` to build `StatusData` or `EventOnlyStatusData` and return `CommandResult::ok(...)` or `CommandResult::err(...)`. The logic stays the same; replace `eprintln!` + `return EXIT_CODE` with `return Box::new(CommandResult::err(...))` and replace `println!` with building the data struct.

New signature:
```rust
pub fn cmd_status(config_dir: &str, targeting: &LedgerTargeting) -> Box<dyn CommandOutput>
```

Add at the top of `src/cli/status.rs`:
```rust
use super::output::{
    CommandOutput, CommandResult, EventOnlyStatusData, GateResultData, MemberData,
    SetSummaryData, StatusData, TransitionSummaryData,
};
```

The function body builds the data structs and returns them. For event-only ledgers, return `CommandResult::ok("status", EventOnlyStatusData { ... })`. For full ledgers, build `StatusData` with sets and transitions.

- [ ] **Step 3: Convert cmd_set_status to return Box<dyn CommandOutput>**

New signature:
```rust
pub fn cmd_set_status(config_dir: &str, set_name: &str, targeting: &LedgerTargeting) -> Box<dyn CommandOutput>
```

Builds `SetSummaryData` and returns `CommandResult::ok("set_status", data)`.

- [ ] **Step 4: Update main.rs dispatch**

Replace the LegacyResult wrappers for Status and Set::Status with direct calls:

```rust
        Commands::Status => status::cmd_status(&cli.config_dir, &targeting),
        // ...
        SetAction::Status { set } => status::cmd_set_status(&cli.config_dir, &set, &targeting),
```

- [ ] **Step 5: Run tests**

Run: `cargo test --test json_output_tests && cargo test --test integration_tests`
Expected: All pass. JSON tests show correct envelope. Text output unchanged.

- [ ] **Step 6: Commit**

```bash
git add src/cli/status.rs src/main.rs tests/json_output_tests.rs
git commit -m "feat: convert status and set-status commands to data-first JSON output"
```

---

### Task 5: Convert log commands (cmd_log_dump, cmd_log_tail)

**Files:**
- Modify: `src/cli/log.rs`
- Modify: `src/main.rs`
- Modify: `tests/json_output_tests.rs`

- [ ] **Step 1: Write CLI integration tests**

Append to `tests/json_output_tests.rs`:

```rust
#[test]
fn test_cli_log_dump_json() {
    let dir = setup_minimal();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "log", "dump"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["command"], "log_dump");
    let entries = v["data"]["entries"].as_array().unwrap();
    assert!(!entries.is_empty());
    // Genesis entry
    assert_eq!(entries[0]["seq"], 0);
    assert!(entries[0]["hash"].as_str().unwrap().len() == 64);
}

#[test]
fn test_cli_log_tail_json() {
    let dir = setup_minimal();
    // Transition to create a second event
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin"])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "log", "tail", "1"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["command"], "log_tail");
    let entries = v["data"]["entries"].as_array().unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0]["event_type"], "state_transition");
}
```

- [ ] **Step 2: Convert cmd_log_dump and cmd_log_tail**

New signatures:
```rust
pub fn cmd_log_dump(config_dir: &str, targeting: &LedgerTargeting) -> Box<dyn CommandOutput>
pub fn cmd_log_tail(config_dir: &str, n: usize, targeting: &LedgerTargeting) -> Box<dyn CommandOutput>
```

Both build `LogData` from ledger entries using a shared helper:

```rust
fn entries_to_log_data(entries: &[crate::ledger::entry::LedgerEntry]) -> LogData {
    LogData {
        entries: entries
            .iter()
            .map(|e| EntryData {
                seq: e.seq,
                timestamp: e.ts.clone(),
                event_type: e.event_type.clone(),
                hash: e.hash.clone(),
                fields: e.fields.clone(),
            })
            .collect(),
    }
}
```

Add imports at the top of `src/cli/log.rs`:
```rust
use super::output::{CommandOutput, CommandResult, EntryData, LogData};
```

- [ ] **Step 3: Update main.rs dispatch**

Remove LegacyResult wrappers for Log::Dump and Log::Tail.

- [ ] **Step 4: Run tests**

Run: `cargo test --test json_output_tests && cargo test --test integration_tests`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/cli/log.rs src/main.rs tests/json_output_tests.rs
git commit -m "feat: convert log dump and log tail commands to data-first JSON output"
```

---

### Task 6: Convert gate check (cmd_gate_check)

**Files:**
- Modify: `src/cli/transition.rs`
- Modify: `src/main.rs`
- Modify: `tests/json_output_tests.rs`

- [ ] **Step 1: Write CLI integration tests**

Append to `tests/json_output_tests.rs`:

```rust
#[test]
fn test_cli_gate_check_json_ready() {
    let dir = setup_minimal();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "gate", "check", "begin"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["command"], "gate_check");
    assert_eq!(v["data"]["transition"], "begin");
    assert_eq!(v["data"]["current_state"], "idle");
    assert_eq!(v["data"]["result"], "ready (no gates)");
}

#[test]
fn test_cli_gate_check_json_blocked() {
    let dir = setup_minimal();
    // Transition to working state first
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin"])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "gate", "check", "complete"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["data"]["result"], "blocked");
    let candidates = v["data"]["candidates"].as_array().unwrap();
    assert_eq!(candidates[0]["all_passed"], false);
}
```

- [ ] **Step 2: Convert cmd_gate_check**

New signature:
```rust
pub fn cmd_gate_check(
    config_dir: &str,
    transition_name: &str,
    args: &[String],
    targeting: &LedgerTargeting,
) -> Box<dyn CommandOutput>
```

Build `GateCheckData` with `CandidateData` and `GateResultData` for each gate result. The logic is the same as current code but accumulates into structs instead of printing.

Add imports at the top of `src/cli/transition.rs`:
```rust
use super::output::{
    CandidateData, CommandOutput, CommandResult, GateCheckData, GateResultData,
};
```

- [ ] **Step 3: Update main.rs dispatch**

Remove LegacyResult wrapper for Gate::Check.

- [ ] **Step 4: Run tests**

Run: `cargo test --test json_output_tests && cargo test --test integration_tests`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/cli/transition.rs src/main.rs tests/json_output_tests.rs
git commit -m "feat: convert gate check command to data-first JSON output"
```

---

### Task 7: Convert manifest verify (cmd_manifest_verify)

**Files:**
- Modify: `src/cli/manifest_cmd.rs`
- Modify: `src/main.rs`
- Modify: `tests/json_output_tests.rs`

- [ ] **Step 1: Write CLI integration test**

Append to `tests/json_output_tests.rs`:

```rust
#[test]
fn test_cli_manifest_verify_json_clean() {
    let dir = setup_minimal();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "manifest", "verify"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["command"], "manifest_verify");
    assert_eq!(v["data"]["clean"], true);
}
```

- [ ] **Step 2: Convert cmd_manifest_verify**

New signature:
```rust
pub fn cmd_manifest_verify(config_dir: &str) -> Box<dyn CommandOutput>
```

Build `ManifestVerifyData` from verification result.

Add imports:
```rust
use super::output::{CommandOutput, CommandResult, ManifestVerifyData, MismatchData};
```

For the error case (mismatches), still return `CommandResult::ok(...)` with `clean: false` — the data carries the mismatch info. The exit code should be non-zero when mismatches exist. Use a custom helper: `CommandResult` with `exit_code` overridden. Add a method to `CommandResult`:

```rust
    /// Create a success result with a non-zero exit code (for "succeeded but found problems" cases).
    pub fn ok_with_exit_code(command: &str, data: T, exit_code: i32) -> Self {
        Self {
            ok: exit_code == 0,
            command: command.to_string(),
            data: Some(data),
            error: None,
            exit_code,
        }
    }
```

For manifest verify with mismatches: `CommandResult::ok_with_exit_code("manifest_verify", data, EXIT_INTEGRITY_ERROR)`.

- [ ] **Step 3: Update main.rs dispatch**

Remove LegacyResult wrapper for Manifest::Verify.

- [ ] **Step 4: Run tests**

Run: `cargo test --test json_output_tests && cargo test --test integration_tests`
Expected: All pass.

- [ ] **Step 5: Commit**

```bash
git add src/cli/manifest_cmd.rs src/cli/output.rs src/main.rs tests/json_output_tests.rs
git commit -m "feat: convert manifest verify command to data-first JSON output"
```

---

### Task 8: HORIZONS-1 example protocol config

**Files:**
- Create: `examples/horizons1/protocol.toml`
- Create: `examples/horizons1/states.toml`
- Create: `examples/horizons1/transitions.toml`
- Create: `examples/horizons1/events.toml`
- Create: `examples/horizons1/renders.toml`

- [ ] **Step 1: Create protocol.toml**

```toml
[protocol]
name = "horizons1"
version = "1.0.0"
description = "HORIZONS-1 interplanetary probe mission control protocol"

[paths]
managed = ["output"]
data_dir = "output/.sahjhan"
render_dir = "output"

[sets.subsystems]
description = "Probe subsystem verification"
values = ["eps", "adcs", "telecom", "propulsion", "payload"]

[aliases]
"launch" = "transition launch"
"status-check" = "gate check clear_for_launch"
```

- [ ] **Step 2: Create states.toml**

```toml
[states.pre_launch]
label = "Pre-Launch"
initial = true

[states.assembly_complete]
label = "Assembly Complete"

[states.testing]
label = "Testing"
params = [
    { name = "current_subsystem", set = "subsystems", source = "current" },
]

[states.launch_ready]
label = "Launch Ready"

[states.launched]
label = "Launched"

[states.cruise]
label = "Cruise"

[states.encounter]
label = "Encounter"

[states.science_ops]
label = "Science Operations"

[states.mission_complete]
label = "Mission Complete"
terminal = true

[states.anomaly]
label = "Anomaly"
```

- [ ] **Step 3: Create transitions.toml**

```toml
[[transitions]]
from = "pre_launch"
to = "assembly_complete"
command = "complete_assembly"
gates = []

[[transitions]]
from = "assembly_complete"
to = "testing"
command = "begin_testing"
gates = []

[[transitions]]
from = "testing"
to = "launch_ready"
command = "clear_for_launch"
gates = [
    { type = "set_covered", set = "subsystems", event = "set_member_complete", field = "member" },
]

[[transitions]]
from = "launch_ready"
to = "launched"
command = "launch"
gates = []

[[transitions]]
from = "launched"
to = "cruise"
command = "begin_cruise"
gates = []

[[transitions]]
from = "cruise"
to = "encounter"
command = "begin_encounter"
gates = []

[[transitions]]
from = "encounter"
to = "science_ops"
command = "begin_science"
gates = []

[[transitions]]
from = "science_ops"
to = "mission_complete"
command = "complete_mission"
gates = []

# Anomaly transitions — reachable from every non-terminal state
[[transitions]]
from = "pre_launch"
to = "anomaly"
command = "declare_anomaly"
gates = []

[[transitions]]
from = "assembly_complete"
to = "anomaly"
command = "declare_anomaly"
gates = []

[[transitions]]
from = "testing"
to = "anomaly"
command = "declare_anomaly"
gates = []

[[transitions]]
from = "launch_ready"
to = "anomaly"
command = "declare_anomaly"
gates = []

[[transitions]]
from = "launched"
to = "anomaly"
command = "declare_anomaly"
gates = []

[[transitions]]
from = "cruise"
to = "anomaly"
command = "declare_anomaly"
gates = []

[[transitions]]
from = "encounter"
to = "anomaly"
command = "declare_anomaly"
gates = []

[[transitions]]
from = "science_ops"
to = "anomaly"
command = "declare_anomaly"
gates = []
```

- [ ] **Step 4: Create events.toml**

```toml
[events.telemetry_update]
description = "Telemetry reading from probe subsystems"
fields = [
    { name = "subsystem", type = "string", values = ["eps", "adcs", "telecom", "propulsion", "payload"] },
    { name = "status", type = "string", values = ["nominal", "degraded", "failed"] },
    { name = "power_draw_watts", type = "string", pattern = "^[0-9]+(\\.[0-9]+)?$" },
]

[events.trajectory_update]
description = "Navigation trajectory update"
fields = [
    { name = "delta_v_remaining_ms", type = "string", pattern = "^[0-9]+(\\.[0-9]+)?$" },
    { name = "periapsis_km", type = "string", pattern = "^[0-9]+(\\.[0-9]+)?$" },
    { name = "target", type = "string" },
]

[events.anomaly_report]
description = "Anomaly detection and reporting"
fields = [
    { name = "subsystem", type = "string" },
    { name = "severity", type = "string", values = ["warning", "critical", "emergency"] },
    { name = "description", type = "string" },
]

[events.science_data_downlink]
description = "Science data transmission to Earth"
fields = [
    { name = "instrument", type = "string" },
    { name = "data_volume_mb", type = "string", pattern = "^[0-9]+(\\.[0-9]+)?$" },
    { name = "snr_db", type = "string", pattern = "^[0-9]+(\\.[0-9]+)?$", optional = true },
]

[events.subsystem_checkout]
description = "Individual subsystem verification during testing phase"
fields = [
    { name = "subsystem", type = "string", values = ["eps", "adcs", "telecom", "propulsion", "payload"] },
    { name = "result", type = "string", values = ["pass", "fail"] },
    { name = "notes", type = "string", optional = true },
]

[events.set_member_complete]
description = "Subsystem verification complete"
fields = [
    { name = "set", type = "string" },
    { name = "member", type = "string" },
]
```

- [ ] **Step 5: Create renders.toml**

```toml
# Renderer plugins arrive in Phase 5.
# For now, no built-in renders are configured.
```

- [ ] **Step 6: Verify config loads**

Run: `cargo run -- --config-dir examples/horizons1 validate`
Expected: Success output (no errors).

- [ ] **Step 7: Commit**

```bash
git add examples/horizons1/
git commit -m "feat: add HORIZONS-1 example protocol for plugin system testing"
```

---

### Task 9: HORIZONS-1 integration tests

**Files:**
- Create: `tests/horizons1_tests.rs`

- [ ] **Step 1: Write integration tests**

```rust
// tests/horizons1_tests.rs
//
// Integration tests for the HORIZONS-1 mission control protocol.

use assert_cmd::Command;
use serde_json::Value;
use tempfile::tempdir;

/// Set up a temp directory with horizons1 config and initialize.
fn setup_horizons1() -> tempfile::TempDir {
    let dir = tempdir().unwrap();
    let config_dir = dir.path().join("enforcement");
    std::fs::create_dir_all(&config_dir).unwrap();
    for file in &[
        "protocol.toml",
        "states.toml",
        "transitions.toml",
        "events.toml",
        "renders.toml",
    ] {
        std::fs::copy(format!("examples/horizons1/{}", file), config_dir.join(file)).unwrap();
    }
    std::fs::create_dir_all(dir.path().join("output")).unwrap();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "init"])
        .current_dir(dir.path())
        .assert()
        .success();

    dir
}

#[test]
fn test_horizons1_init_and_status() {
    let dir = setup_horizons1();
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["data"]["state"], "pre_launch");
    // Should have subsystems set
    let sets = v["data"]["sets"].as_array().unwrap();
    assert_eq!(sets.len(), 1);
    assert_eq!(sets[0]["name"], "subsystems");
    assert_eq!(sets[0]["total"], 5);
    assert_eq!(sets[0]["completed"], 0);
}

#[test]
fn test_horizons1_transition_through_phases() {
    let dir = setup_horizons1();

    // pre_launch → assembly_complete
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "complete_assembly"])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["data"]["state"], "assembly_complete");

    // assembly_complete → testing
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin_testing"])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["data"]["state"], "testing");
}

#[test]
fn test_horizons1_gate_blocks_launch_without_subsystems() {
    let dir = setup_horizons1();

    // Advance to testing
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "complete_assembly"])
        .current_dir(dir.path())
        .assert()
        .success();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin_testing"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Gate check should show blocked (subsystems not complete)
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "gate", "check", "clear_for_launch"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    assert!(output.status.success());
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["data"]["result"], "blocked");
    let candidates = v["data"]["candidates"].as_array().unwrap();
    assert_eq!(candidates[0]["all_passed"], false);
}

#[test]
fn test_horizons1_subsystem_completion_unblocks_launch() {
    let dir = setup_horizons1();

    // Advance to testing
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "complete_assembly"])
        .current_dir(dir.path())
        .assert()
        .success();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin_testing"])
        .current_dir(dir.path())
        .assert()
        .success();

    // Complete all subsystems
    for subsystem in &["eps", "adcs", "telecom", "propulsion", "payload"] {
        Command::cargo_bin("sahjhan")
            .unwrap()
            .args(["--config-dir", "enforcement", "set", "complete", "subsystems", subsystem])
            .current_dir(dir.path())
            .assert()
            .success();
    }

    // Gate check should now show ready
    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "gate", "check", "clear_for_launch"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["data"]["result"], "ready");
}

#[test]
fn test_horizons1_anomaly_from_any_state() {
    let dir = setup_horizons1();

    // Declare anomaly from pre_launch
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "declare_anomaly"])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "status"])
        .current_dir(dir.path())
        .output()
        .unwrap();
    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["data"]["state"], "anomaly");
}

#[test]
fn test_horizons1_log_json_after_transitions() {
    let dir = setup_horizons1();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "complete_assembly"])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "log", "dump"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    let entries = v["data"]["entries"].as_array().unwrap();
    // genesis + state_transition
    assert!(entries.len() >= 2);
    assert_eq!(entries.last().unwrap()["event_type"], "state_transition");
}

#[test]
fn test_horizons1_set_status_json() {
    let dir = setup_horizons1();

    // Advance to testing and complete two subsystems
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "complete_assembly"])
        .current_dir(dir.path())
        .assert()
        .success();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "transition", "begin_testing"])
        .current_dir(dir.path())
        .assert()
        .success();

    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "set", "complete", "subsystems", "eps"])
        .current_dir(dir.path())
        .assert()
        .success();
    Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "set", "complete", "subsystems", "adcs"])
        .current_dir(dir.path())
        .assert()
        .success();

    let output = Command::cargo_bin("sahjhan")
        .unwrap()
        .args(["--config-dir", "enforcement", "--json", "set", "status", "subsystems"])
        .current_dir(dir.path())
        .output()
        .unwrap();

    let v: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(v["data"]["name"], "subsystems");
    assert_eq!(v["data"]["completed"], 2);
    assert_eq!(v["data"]["total"], 5);
    let members = v["data"]["members"].as_array().unwrap();
    assert_eq!(members.len(), 5);
    // eps and adcs should be done
    assert_eq!(members[0]["name"], "eps");
    assert_eq!(members[0]["done"], true);
    assert_eq!(members[1]["name"], "adcs");
    assert_eq!(members[1]["done"], true);
    assert_eq!(members[2]["done"], false);
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --test horizons1_tests`
Expected: All 8 tests pass.

- [ ] **Step 3: Commit**

```bash
git add tests/horizons1_tests.rs
git commit -m "test: HORIZONS-1 integration tests for status, transitions, gates, sets with --json"
```

---

### Task 10: Update CLAUDE.md and final verification

**Files:**
- Modify: `CLAUDE.md`

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Count the tests. Expected: 347 + ~25 new ≈ 372+.

Run: `cargo clippy -- -D warnings`
Expected: No warnings.

Run: `cargo fmt --check`
Expected: No formatting issues (run `cargo fmt` first if needed).

- [ ] **Step 2: Update CLAUDE.md**

Add `output.rs` to the `cli/` module lookup table:

```
| JSON output types | `cli/output.rs` | `CommandOutput`, `CommandResult<T>`, data structs | Structured output with JSON envelope |
```

Update the test count to reflect new tests.

Add `tests/horizons1_tests.rs` and `tests/json_output_tests.rs` to the test files table:

```
| `tests/json_output_tests.rs` | JSON envelope serialization, per-command data struct tests, CLI --json integration |
| `tests/horizons1_tests.rs` | HORIZONS-1 protocol init, transitions, gates, sets, --json output |
```

- [ ] **Step 3: Run full test suite again**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add CLAUDE.md
git commit -m "docs: update CLAUDE.md with output.rs module, new test files, test count"
```
