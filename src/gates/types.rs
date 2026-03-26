// src/gates/types.rs
//
// Individual evaluation functions for each of the 11 gate types.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use regex::Regex;

use crate::config::GateConfig;
use crate::ledger::entry::LedgerEntry;

use super::evaluator::{GateContext, GateResult};
use super::template::{resolve_template, resolve_template_plain};

// ---------------------------------------------------------------------------
// Public dispatch
// ---------------------------------------------------------------------------

/// Evaluate a single gate by dispatching on `gate.gate_type`.
pub fn eval(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    match gate.gate_type.as_str() {
        "file_exists" => eval_file_exists(gate, ctx),
        "files_exist" => eval_files_exist(gate, ctx),
        "command_succeeds" => eval_command_succeeds(gate, ctx),
        "command_output" => eval_command_output(gate, ctx),
        "ledger_has_event" => eval_ledger_has_event(gate, ctx),
        "ledger_has_event_since" => eval_ledger_has_event_since(gate, ctx),
        "set_covered" => eval_set_covered(gate, ctx),
        "min_elapsed" => eval_min_elapsed(gate, ctx),
        "no_violations" => eval_no_violations(gate, ctx),
        "field_not_empty" => eval_field_not_empty(gate, ctx),
        "snapshot_compare" => eval_snapshot_compare(gate, ctx),
        other => GateResult {
            passed: false,
            gate_type: other.to_string(),
            description: format!("unknown gate type '{}'", other),
            reason: Some(format!("gate type '{}' is not implemented", other)),
        },
    }
}

// ---------------------------------------------------------------------------
// file_exists
// ---------------------------------------------------------------------------

fn eval_file_exists(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let raw_path = gate
        .params
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let vars = build_template_vars(ctx);
    let resolved = resolve_template_plain(raw_path, &vars);
    let exists = Path::new(&resolved).exists();

    GateResult {
        passed: exists,
        gate_type: "file_exists".to_string(),
        description: format!("file '{}' exists", resolved),
        reason: if exists {
            None
        } else {
            Some(format!("file '{}' does not exist", resolved))
        },
    }
}

// ---------------------------------------------------------------------------
// files_exist
// ---------------------------------------------------------------------------

fn eval_files_exist(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let vars = build_template_vars(ctx);
    let paths: Vec<String> = gate
        .params
        .get("paths")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str())
                .map(|s| resolve_template_plain(s, &vars))
                .collect()
        })
        .unwrap_or_default();

    let missing: Vec<&str> = paths
        .iter()
        .filter(|p| !Path::new(p.as_str()).exists())
        .map(|p| p.as_str())
        .collect();

    let passed = missing.is_empty();

    GateResult {
        passed,
        gate_type: "files_exist".to_string(),
        description: format!("{} file(s) must exist", paths.len()),
        reason: if passed {
            None
        } else {
            Some(format!("missing files: {}", missing.join(", ")))
        },
    }
}

// ---------------------------------------------------------------------------
// command_succeeds
// ---------------------------------------------------------------------------

fn eval_command_succeeds(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let raw_cmd = gate
        .params
        .get("cmd")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let timeout_secs = gate
        .params
        .get("timeout")
        .and_then(|v| v.as_integer())
        .map(|t| t as u64)
        .unwrap_or(60);

    // Validate fields referenced in the template before interpolation.
    if let Err(reason) = validate_template_fields(raw_cmd, ctx) {
        return GateResult {
            passed: false,
            gate_type: "command_succeeds".to_string(),
            description: format!("command succeeds: {}", raw_cmd),
            reason: Some(reason),
        };
    }

    let vars = build_template_vars(ctx);
    let cmd = resolve_template(raw_cmd, &vars);

    match run_shell_with_timeout(&cmd, &ctx.working_dir, timeout_secs) {
        Ok(CommandOutcome::Completed(status)) => {
            let passed = status.success();
            GateResult {
                passed,
                gate_type: "command_succeeds".to_string(),
                description: format!("command succeeds: {}", cmd),
                reason: if passed {
                    None
                } else {
                    Some(format!(
                        "command '{}' exited with status {}",
                        cmd,
                        status.code().unwrap_or(-1)
                    ))
                },
            }
        }
        Ok(CommandOutcome::TimedOut) => GateResult {
            passed: false,
            gate_type: "command_succeeds".to_string(),
            description: format!("command succeeds: {}", cmd),
            reason: Some(format!(
                "command '{}' timed out after {}s",
                cmd, timeout_secs
            )),
        },
        Err(e) => GateResult {
            passed: false,
            gate_type: "command_succeeds".to_string(),
            description: format!("command succeeds: {}", cmd),
            reason: Some(format!("failed to run command '{}': {}", cmd, e)),
        },
    }
}

// ---------------------------------------------------------------------------
// command_output
// ---------------------------------------------------------------------------

fn eval_command_output(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let raw_cmd = gate
        .params
        .get("cmd")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let expect = gate
        .params
        .get("expect")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let timeout_secs = gate
        .params
        .get("timeout")
        .and_then(|v| v.as_integer())
        .map(|t| t as u64)
        .unwrap_or(60);

    // Validate fields referenced in the template before interpolation.
    if let Err(reason) = validate_template_fields(raw_cmd, ctx) {
        return GateResult {
            passed: false,
            gate_type: "command_output".to_string(),
            description: format!("command output matches '{}'", expect),
            reason: Some(reason),
        };
    }

    let vars = build_template_vars(ctx);
    let cmd = resolve_template(raw_cmd, &vars);

    match run_shell_output_with_timeout(&cmd, &ctx.working_dir, timeout_secs) {
        Ok(CommandOutputOutcome::Completed(stdout)) => {
            let trimmed = stdout.trim().to_string();
            let passed = trimmed == expect;
            GateResult {
                passed,
                gate_type: "command_output".to_string(),
                description: format!("command output matches '{}'", expect),
                reason: if passed {
                    None
                } else {
                    Some(format!("expected '{}', got '{}'", expect, trimmed))
                },
            }
        }
        Ok(CommandOutputOutcome::TimedOut) => GateResult {
            passed: false,
            gate_type: "command_output".to_string(),
            description: format!("command output matches '{}'", expect),
            reason: Some(format!(
                "command '{}' timed out after {}s",
                cmd, timeout_secs
            )),
        },
        Err(e) => GateResult {
            passed: false,
            gate_type: "command_output".to_string(),
            description: format!("command output matches '{}'", expect),
            reason: Some(format!("failed to run command '{}': {}", cmd, e)),
        },
    }
}

// ---------------------------------------------------------------------------
// ledger_has_event
// ---------------------------------------------------------------------------

fn eval_ledger_has_event(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let event = gate
        .params
        .get("event")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let min_count = gate
        .params
        .get("min_count")
        .and_then(|v| v.as_integer())
        .map(|n| n as u32)
        .unwrap_or(1);

    // Optional filter map: each key/value must match the deserialized payload.
    let filter: HashMap<String, String> = gate
        .params
        .get("filter")
        .and_then(|v| v.as_table())
        .map(|tbl| {
            tbl.iter()
                .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                .collect()
        })
        .unwrap_or_default();

    let matching = ctx
        .ledger
        .events_of_type(event)
        .into_iter()
        .filter(|e| entry_matches_filter(e, &filter))
        .count();

    let passed = matching >= min_count as usize;

    GateResult {
        passed,
        gate_type: "ledger_has_event".to_string(),
        description: format!("ledger has >= {} '{}' event(s)", min_count, event),
        reason: if passed {
            None
        } else {
            Some(format!(
                "found {} '{}' event(s), need >= {}",
                matching, event, min_count
            ))
        },
    }
}

// ---------------------------------------------------------------------------
// ledger_has_event_since
// ---------------------------------------------------------------------------

fn eval_ledger_has_event_since(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let event = gate
        .params
        .get("event")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    // Only "last_transition" is specified in the spec; treat anything else as
    // "last_transition" too (graceful fallback).
    let _since = gate
        .params
        .get("since")
        .and_then(|v| v.as_str())
        .unwrap_or("last_transition");

    // Find the most recent state_transition event.
    let last_transition_seq = ctx
        .ledger
        .events_of_type("state_transition")
        .last()
        .map(|e| e.seq);

    // If there has been no transition yet, we check all entries.
    let threshold_seq = last_transition_seq.unwrap_or(0);

    let found = ctx
        .ledger
        .entries()
        .iter()
        .any(|e| e.event_type == event && e.seq > threshold_seq);

    GateResult {
        passed: found,
        gate_type: "ledger_has_event_since".to_string(),
        description: format!("'{}' event exists since last transition", event),
        reason: if found {
            None
        } else {
            Some(format!(
                "no '{}' event found after the last state_transition",
                event
            ))
        },
    }
}

// ---------------------------------------------------------------------------
// set_covered (Issue #8: Use HashSet instead of Vec)
// ---------------------------------------------------------------------------

fn eval_set_covered(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let set_name = match gate.params.get("set").and_then(|v| v.as_str()) {
        Some(s) => s,
        None => {
            return GateResult {
                passed: false,
                gate_type: "set_covered".to_string(),
                description: "set is fully covered".to_string(),
                reason: Some("gate missing 'set' param".to_string()),
            }
        }
    };

    let event_name = gate
        .params
        .get("event")
        .and_then(|v| v.as_str())
        .unwrap_or("set_member_complete");

    let field_name = gate
        .params
        .get("field")
        .and_then(|v| v.as_str())
        .unwrap_or("member");

    let set_config = match ctx.config.sets.get(set_name) {
        Some(s) => s,
        None => {
            return GateResult {
                passed: false,
                gate_type: "set_covered".to_string(),
                description: format!("set '{}' is fully covered", set_name),
                reason: Some(format!("unknown set '{}'", set_name)),
            }
        }
    };

    // Collect the unique values of `field_name` from entries where
    // `"set" == set_name`.  Use HashSet for O(1) membership checks.
    let mut covered: HashSet<String> = HashSet::new();
    for entry in ctx.ledger.events_of_type(event_name) {
        let set_matches = entry
            .fields
            .get("set")
            .map(|v| v.as_str() == set_name)
            .unwrap_or(false);
        if set_matches {
            if let Some(member) = entry.fields.get(field_name) {
                covered.insert(member.clone());
            }
        }
    }

    let missing: Vec<&str> = set_config
        .values
        .iter()
        .filter(|v| !covered.contains(v.as_str()))
        .map(|v| v.as_str())
        .collect();

    let passed = missing.is_empty();

    GateResult {
        passed,
        gate_type: "set_covered".to_string(),
        description: format!("set '{}' is fully covered", set_name),
        reason: if passed {
            None
        } else {
            Some(format!(
                "set '{}' not fully covered; missing: {}",
                set_name,
                missing.join(", ")
            ))
        },
    }
}

// ---------------------------------------------------------------------------
// min_elapsed
// ---------------------------------------------------------------------------

fn eval_min_elapsed(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let event = gate
        .params
        .get("event")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let seconds = gate
        .params
        .get("seconds")
        .and_then(|v| v.as_integer())
        .map(|s| s as u64)
        .unwrap_or(0);

    // Find the most recent matching event and parse its ISO 8601 timestamp.
    let last_ts_ms = ctx
        .ledger
        .events_of_type(event)
        .last()
        .and_then(|e| {
            chrono::DateTime::parse_from_rfc3339(&e.ts)
                .ok()
                .map(|dt| dt.timestamp_millis())
        });

    let description = format!(
        "at least {} second(s) since last '{}' event",
        seconds, event
    );

    match last_ts_ms {
        None => {
            // No event found — consider the elapsed time infinite.
            GateResult {
                passed: true,
                gate_type: "min_elapsed".to_string(),
                description,
                reason: None,
            }
        }
        Some(ts_ms) => {
            let now_ms = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or(Duration::ZERO)
                .as_millis() as i64;

            let elapsed_ms = now_ms.saturating_sub(ts_ms);
            let required_ms = (seconds as i64) * 1000;
            let passed = elapsed_ms >= required_ms;

            GateResult {
                passed,
                gate_type: "min_elapsed".to_string(),
                description,
                reason: if passed {
                    None
                } else {
                    Some(format!(
                        "only {}ms elapsed since last '{}' event, need {}ms",
                        elapsed_ms, event, required_ms
                    ))
                },
            }
        }
    }
}

// ---------------------------------------------------------------------------
// no_violations (Issue #3: Account for resolved violations)
// ---------------------------------------------------------------------------

fn eval_no_violations(_gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let violations = ctx.ledger.events_of_type("protocol_violation").len();
    let resolved = ctx.ledger.events_of_type("violation_resolved").len();
    let unresolved = violations.saturating_sub(resolved);
    let passed = unresolved == 0;

    GateResult {
        passed,
        gate_type: "no_violations".to_string(),
        description: "no unresolved protocol_violation events".to_string(),
        reason: if passed {
            None
        } else {
            Some(format!(
                "found {} unresolved protocol_violation event(s) ({} total, {} resolved)",
                unresolved, violations, resolved
            ))
        },
    }
}

// ---------------------------------------------------------------------------
// field_not_empty
// ---------------------------------------------------------------------------

fn eval_field_not_empty(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let field = gate
        .params
        .get("field")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let description = format!("field '{}' is non-empty", field);

    let value = ctx
        .event_fields
        .and_then(|fields| fields.get(field))
        .map(|s| s.as_str());

    match value {
        None => GateResult {
            passed: false,
            gate_type: "field_not_empty".to_string(),
            description,
            reason: Some(format!("field '{}' not present in event payload", field)),
        },
        Some("") => GateResult {
            passed: false,
            gate_type: "field_not_empty".to_string(),
            description,
            reason: Some(format!("field '{}' is empty", field)),
        },
        Some(_) => GateResult {
            passed: true,
            gate_type: "field_not_empty".to_string(),
            description,
            reason: None,
        },
    }
}

// ---------------------------------------------------------------------------
// snapshot_compare (Issue #2: Resolve "snapshot:key" references from ledger)
// ---------------------------------------------------------------------------

fn eval_snapshot_compare(gate: &GateConfig, ctx: &GateContext) -> GateResult {
    let raw_cmd = gate
        .params
        .get("cmd")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let extract = gate
        .params
        .get("extract")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let compare = gate
        .params
        .get("compare")
        .and_then(|v| v.as_str())
        .unwrap_or("eq");

    let reference_raw = gate
        .params
        .get("reference")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    let timeout_secs = gate
        .params
        .get("timeout")
        .and_then(|v| v.as_integer())
        .map(|t| t as u64)
        .unwrap_or(60);

    let vars = build_template_vars(ctx);
    let cmd = resolve_template(raw_cmd, &vars);
    let description = format!(
        "snapshot_compare: {} {} {}",
        extract, compare, reference_raw
    );

    // Resolve reference — if it starts with "snapshot:", look up in ledger.
    let reference = if let Some(snapshot_key) = reference_raw.strip_prefix("snapshot:") {
        match resolve_snapshot_reference(ctx, snapshot_key) {
            Ok(value) => value,
            Err(reason) => {
                return GateResult {
                    passed: false,
                    gate_type: "snapshot_compare".to_string(),
                    description,
                    reason: Some(reason),
                }
            }
        }
    } else {
        reference_raw.to_string()
    };

    // Run command and get stdout with timeout enforcement.
    let stdout = match run_shell_output_with_timeout(&cmd, &ctx.working_dir, timeout_secs) {
        Ok(CommandOutputOutcome::Completed(s)) => s,
        Ok(CommandOutputOutcome::TimedOut) => {
            return GateResult {
                passed: false,
                gate_type: "snapshot_compare".to_string(),
                description,
                reason: Some(format!(
                    "command '{}' timed out after {}s",
                    cmd, timeout_secs
                )),
            }
        }
        Err(e) => {
            return GateResult {
                passed: false,
                gate_type: "snapshot_compare".to_string(),
                description,
                reason: Some(format!("command failed: {}", e)),
            }
        }
    };

    // Parse stdout as JSON and extract the named field.
    let json_value: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(e) => {
            return GateResult {
                passed: false,
                gate_type: "snapshot_compare".to_string(),
                description,
                reason: Some(format!("stdout is not valid JSON: {}", e)),
            }
        }
    };

    let extracted = match json_value.get(extract) {
        Some(v) => v.clone(),
        None => {
            return GateResult {
                passed: false,
                gate_type: "snapshot_compare".to_string(),
                description,
                reason: Some(format!("JSON field '{}' not found in output", extract)),
            }
        }
    };

    // Compare extracted value (as f64) against reference.
    let extracted_num = match extracted.as_f64() {
        Some(n) => n,
        None => {
            // Try string comparison.
            let extracted_owned = extracted.to_string();
            let extracted_str = extracted.as_str().unwrap_or(&extracted_owned);
            let passed = match compare {
                "eq" => extracted_str == reference,
                _ => false,
            };
            return GateResult {
                passed,
                gate_type: "snapshot_compare".to_string(),
                description,
                reason: if passed {
                    None
                } else {
                    Some(format!(
                        "string compare: '{}' {} '{}' is false",
                        extracted_str, compare, reference
                    ))
                },
            };
        }
    };

    let reference_num: f64 = match reference.parse() {
        Ok(n) => n,
        Err(e) => {
            return GateResult {
                passed: false,
                gate_type: "snapshot_compare".to_string(),
                description,
                reason: Some(format!("reference '{}' is not a number: {}", reference, e)),
            }
        }
    };

    let passed = match compare {
        "gt" => extracted_num > reference_num,
        "gte" => extracted_num >= reference_num,
        "eq" => (extracted_num - reference_num).abs() < f64::EPSILON,
        other => {
            return GateResult {
                passed: false,
                gate_type: "snapshot_compare".to_string(),
                description,
                reason: Some(format!("unknown compare operator '{}'", other)),
            }
        }
    };

    GateResult {
        passed,
        gate_type: "snapshot_compare".to_string(),
        description,
        reason: if passed {
            None
        } else {
            Some(format!(
                "{} {} {} is false",
                extracted_num, compare, reference_num
            ))
        },
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

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

/// Resolve a `"snapshot:key"` reference from the ledger.
///
/// Searches `ctx.ledger.events_of_type("snapshot")` for the most recent entry
/// whose payload contains a matching `key` field, then returns the `value`
/// field from that entry's payload.
fn resolve_snapshot_reference(ctx: &GateContext, snapshot_key: &str) -> Result<String, String> {
    let snapshots = ctx.ledger.events_of_type("snapshot");

    // Walk backwards to find the most recent snapshot with matching key.
    for entry in snapshots.iter().rev() {
        if entry.fields.get("key").map(|k| k.as_str()) == Some(snapshot_key) {
            return entry
                .fields
                .get("value")
                .cloned()
                .ok_or_else(|| {
                    format!("snapshot with key '{}' has no 'value' field", snapshot_key)
                });
        }
    }

    Err(format!(
        "no snapshot found with key '{}' in ledger",
        snapshot_key
    ))
}

/// Validate template variables against event field definitions.
///
/// For each `{{var}}` in the template that corresponds to a state_param, check
/// if there is an event field definition in `config.events` with a `pattern`
/// regex. If so, validate the value matches before allowing interpolation.
///
/// Issue #4: Field validation performed *before* template interpolation.
fn validate_template_fields(template: &str, ctx: &GateContext) -> Result<(), String> {
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

// ---------------------------------------------------------------------------
// Command execution with timeout enforcement
// (Issue #1, #6: Use try_wait() polling loop)
// ---------------------------------------------------------------------------

/// Outcome of running a shell command with timeout.
enum CommandOutcome {
    /// Command completed within the timeout.
    Completed(std::process::ExitStatus),
    /// Command exceeded the timeout and was killed.
    TimedOut,
}

/// Outcome of running a shell command with output capture and timeout.
enum CommandOutputOutcome {
    /// Command completed within the timeout, producing this stdout.
    Completed(String),
    /// Command exceeded the timeout and was killed.
    TimedOut,
}

/// Run a shell command with timeout enforcement using `try_wait()` polling.
fn run_shell_with_timeout(
    cmd: &str,
    working_dir: &Path,
    timeout_secs: u64,
) -> Result<CommandOutcome, std::io::Error> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(working_dir)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let poll_interval = Duration::from_millis(50);

    loop {
        match child.try_wait()? {
            Some(status) => return Ok(CommandOutcome::Completed(status)),
            None => {
                if Instant::now() >= deadline {
                    // Kill the process and return timeout.
                    let _ = child.kill();
                    let _ = child.wait(); // Reap the zombie.
                    return Ok(CommandOutcome::TimedOut);
                }
                std::thread::sleep(poll_interval);
            }
        }
    }
}

/// Run a shell command capturing stdout, with timeout enforcement.
fn run_shell_output_with_timeout(
    cmd: &str,
    working_dir: &Path,
    timeout_secs: u64,
) -> Result<CommandOutputOutcome, std::io::Error> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(working_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let poll_interval = Duration::from_millis(50);

    loop {
        match child.try_wait()? {
            Some(_status) => {
                // Process has exited — read stdout.
                let output = child.wait_with_output()?;
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                return Ok(CommandOutputOutcome::Completed(stdout));
            }
            None => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Ok(CommandOutputOutcome::TimedOut);
                }
                std::thread::sleep(poll_interval);
            }
        }
    }
}

/// Check whether a ledger entry's fields match all key/value pairs in `filter`.
fn entry_matches_filter(entry: &LedgerEntry, filter: &HashMap<String, String>) -> bool {
    if filter.is_empty() {
        return true;
    }
    filter
        .iter()
        .all(|(k, v)| entry.fields.get(k).map(|fv| fv == v).unwrap_or(false))
}
