// src/gates/command.rs
//
// ## Index
// - [eval-command-succeeds]        eval_command_succeeds()         — run a shell command; pass if exit code is 0; captures stdout for attestation
// - [eval-command-output]          eval_command_output()           — run a shell command; pass if stdout matches expected string; captures stdout for attestation
// - [run-shell-output-with-timeout] run_shell_output_with_timeout() — run a command with polling timeout, captures stdout + stderr + ExitStatus
// - output_tail()      — bounded tail (lines+bytes) of captured output, or None if blank
// - annotate_failure() — append a stderr (or stdout) tail to a gate-failure reason so *why* it failed is visible

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use chrono::Utc;
use sha2::{Digest, Sha256};

use crate::config::GateConfig;

use super::evaluator::{GateAttestation, GateContext, GateResult};
use super::template::{find_unresolved_vars, resolve_template};
use super::types::{build_template_vars, validate_template_fields};

// ---------------------------------------------------------------------------
// Outcome types
// ---------------------------------------------------------------------------

/// Outcome of running a shell command with output capture and timeout.
pub(super) enum CommandOutputOutcome {
    /// Command completed within the timeout, producing this stdout, stderr,
    /// and exit status. stderr is captured so gates can report *why* a command
    /// failed (e.g. `python: command not found`) instead of only its
    /// downstream symptom.
    Completed(String, String, std::process::ExitStatus),
    /// Command exceeded the timeout and was killed.
    TimedOut,
}

// ---------------------------------------------------------------------------
// Failure diagnostics
// ---------------------------------------------------------------------------

/// Max stderr/stdout tail (lines and bytes) surfaced in a gate-failure reason.
/// Bounded so a chatty command can't flood the block message.
const FAILURE_TAIL_LINES: usize = 20;
const FAILURE_TAIL_BYTES: usize = 2000;

/// Return a bounded tail of `s` (last `FAILURE_TAIL_LINES` lines, capped at
/// `FAILURE_TAIL_BYTES` — keeping the *end*, where errors usually are), or
/// `None` if `s` is blank.
fn output_tail(s: &str) -> Option<String> {
    let trimmed = s.trim_end();
    if trimmed.trim().is_empty() {
        return None;
    }
    let lines: Vec<&str> = trimmed.lines().collect();
    let start = lines.len().saturating_sub(FAILURE_TAIL_LINES);
    let mut tail = lines[start..].join("\n");
    if tail.len() > FAILURE_TAIL_BYTES {
        let mut cut = tail.len() - FAILURE_TAIL_BYTES;
        while cut < tail.len() && !tail.is_char_boundary(cut) {
            cut += 1;
        }
        tail = format!("\u{2026}{}", &tail[cut..]);
    }
    Some(tail)
}

/// Append a bounded stderr (or, when stderr is blank and `stdout_fallback` is
/// set, stdout) tail to a gate-failure headline, so the *reason* a command
/// failed is visible. A missing interpreter (`python3: No module named
/// pytest`) reads very differently from a genuine test failure, yet a bare exit
/// code hides which one it was — turning a 30-minute diagnosis into seconds.
fn annotate_failure(headline: String, stdout: &str, stderr: &str, stdout_fallback: bool) -> String {
    if let Some(tail) = output_tail(stderr) {
        return format!(
            "{}\n\u{2500}\u{2500} stderr (tail) \u{2500}\u{2500}\n{}",
            headline, tail
        );
    }
    if stdout_fallback {
        if let Some(tail) = output_tail(stdout) {
            return format!(
                "{}\n\u{2500}\u{2500} stdout (tail) \u{2500}\u{2500}\n{}",
                headline, tail
            );
        }
    }
    headline
}

// [eval-command-succeeds]
pub(super) fn eval_command_succeeds(gate: &GateConfig, ctx: &GateContext) -> GateResult {
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
            evaluable: true,
            gate_type: "command_succeeds".to_string(),
            description: format!("command succeeds: {}", raw_cmd),
            reason: Some(reason),
            intent: None,
            attestation: None,
        };
    }

    let vars = build_template_vars(ctx);
    let cmd = resolve_template(raw_cmd, &vars);

    let unresolved = find_unresolved_vars(&cmd);
    if !unresolved.is_empty() {
        return GateResult {
            passed: false,
            evaluable: false,
            gate_type: "command_succeeds".to_string(),
            description: format!("command succeeds: {}", raw_cmd),
            reason: Some(format!(
                "unevaluable (requires arg: {})",
                unresolved.join(", ")
            )),
            intent: None,
            attestation: None,
        };
    }

    let should_attest = gate
        .params
        .get("attest")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let started_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let start = Instant::now();

    match run_shell_output_with_timeout(&cmd, &ctx.working_dir, timeout_secs) {
        Ok(CommandOutputOutcome::Completed(stdout, stderr, status)) => {
            let wall_time_ms = start.elapsed().as_millis() as u64;
            let passed = status.success();
            let attestation = if passed && should_attest {
                let stdout_hash = format!("{:x}", Sha256::digest(stdout.as_bytes()));
                Some(GateAttestation {
                    gate_type: "command_succeeds".to_string(),
                    command: cmd.clone(),
                    exit_code: status.code().unwrap_or(-1),
                    stdout_hash,
                    wall_time_ms,
                    executed_at: started_at,
                })
            } else {
                None
            };
            GateResult {
                passed,
                evaluable: true,
                gate_type: "command_succeeds".to_string(),
                description: format!("command succeeds: {}", cmd),
                reason: if passed {
                    None
                } else {
                    Some(annotate_failure(
                        format!(
                            "command '{}' exited with status {}",
                            cmd,
                            status.code().unwrap_or(-1)
                        ),
                        &stdout,
                        &stderr,
                        true,
                    ))
                },
                intent: None,
                attestation,
            }
        }
        Ok(CommandOutputOutcome::TimedOut) => GateResult {
            passed: false,
            evaluable: true,
            gate_type: "command_succeeds".to_string(),
            description: format!("command succeeds: {}", cmd),
            reason: Some(format!(
                "command '{}' timed out after {}s",
                cmd, timeout_secs
            )),
            intent: None,
            attestation: None,
        },
        Err(e) => GateResult {
            passed: false,
            evaluable: true,
            gate_type: "command_succeeds".to_string(),
            description: format!("command succeeds: {}", cmd),
            reason: Some(format!("failed to run command '{}': {}", cmd, e)),
            intent: None,
            attestation: None,
        },
    }
}

// [eval-command-output]
pub(super) fn eval_command_output(gate: &GateConfig, ctx: &GateContext) -> GateResult {
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
            evaluable: true,
            gate_type: "command_output".to_string(),
            description: format!("command output matches '{}'", expect),
            reason: Some(reason),
            intent: None,
            attestation: None,
        };
    }

    let vars = build_template_vars(ctx);
    let cmd = resolve_template(raw_cmd, &vars);

    let unresolved = find_unresolved_vars(&cmd);
    if !unresolved.is_empty() {
        return GateResult {
            passed: false,
            evaluable: false,
            gate_type: "command_output".to_string(),
            description: format!("command output matches '{}'", expect),
            reason: Some(format!(
                "unevaluable (requires arg: {})",
                unresolved.join(", ")
            )),
            intent: None,
            attestation: None,
        };
    }

    let should_attest = gate
        .params
        .get("attest")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let started_at = Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
    let start = Instant::now();

    match run_shell_output_with_timeout(&cmd, &ctx.working_dir, timeout_secs) {
        Ok(CommandOutputOutcome::Completed(stdout, stderr, status)) => {
            let wall_time_ms = start.elapsed().as_millis() as u64;
            let trimmed = stdout.trim().to_string();
            let passed = trimmed == expect;
            let attestation = if passed && should_attest {
                let stdout_hash = format!("{:x}", Sha256::digest(stdout.as_bytes()));
                Some(GateAttestation {
                    gate_type: "command_output".to_string(),
                    command: cmd.clone(),
                    exit_code: status.code().unwrap_or(-1),
                    stdout_hash,
                    wall_time_ms,
                    executed_at: started_at,
                })
            } else {
                None
            };
            GateResult {
                passed,
                evaluable: true,
                gate_type: "command_output".to_string(),
                description: format!("command output matches '{}'", expect),
                reason: if passed {
                    None
                } else {
                    Some(annotate_failure(
                        format!("expected '{}', got '{}'", expect, trimmed),
                        &stdout,
                        &stderr,
                        false,
                    ))
                },
                intent: None,
                attestation,
            }
        }
        Ok(CommandOutputOutcome::TimedOut) => GateResult {
            passed: false,
            evaluable: true,
            gate_type: "command_output".to_string(),
            description: format!("command output matches '{}'", expect),
            reason: Some(format!(
                "command '{}' timed out after {}s",
                cmd, timeout_secs
            )),
            intent: None,
            attestation: None,
        },
        Err(e) => GateResult {
            passed: false,
            evaluable: true,
            gate_type: "command_output".to_string(),
            description: format!("command output matches '{}'", expect),
            reason: Some(format!("failed to run command '{}': {}", cmd, e)),
            intent: None,
            attestation: None,
        },
    }
}

// ---------------------------------------------------------------------------
// Command execution with timeout enforcement
// (Issue #1, #6: Use try_wait() polling loop)
// ---------------------------------------------------------------------------

// [run-shell-output-with-timeout]
/// Run a shell command capturing stdout, with timeout enforcement.
pub(super) fn run_shell_output_with_timeout(
    cmd: &str,
    working_dir: &Path,
    timeout_secs: u64,
) -> Result<CommandOutputOutcome, std::io::Error> {
    let mut child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(working_dir)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let poll_interval = Duration::from_millis(50);

    loop {
        match child.try_wait()? {
            Some(_status) => {
                // Process has exited — read stdout and stderr.
                let output = child.wait_with_output()?;
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                return Ok(CommandOutputOutcome::Completed(
                    stdout,
                    stderr,
                    output.status,
                ));
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
