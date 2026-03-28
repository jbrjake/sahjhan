// src/gates/command.rs
//
// ## Index
// - [eval-command-succeeds]        eval_command_succeeds()         — run a shell command; pass if exit code is 0
// - [eval-command-output]          eval_command_output()           — run a shell command; pass if stdout matches expected string
// - [run-shell-with-timeout]       run_shell_with_timeout()        — run a command with polling timeout, status only
// - [run-shell-output-with-timeout] run_shell_output_with_timeout() — run a command with polling timeout, captures stdout

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use crate::config::GateConfig;

use super::evaluator::{GateContext, GateResult};
use super::template::resolve_template;
use super::types::{build_template_vars, validate_template_fields};

// ---------------------------------------------------------------------------
// Outcome types
// ---------------------------------------------------------------------------

/// Outcome of running a shell command with timeout.
pub(super) enum CommandOutcome {
    /// Command completed within the timeout.
    Completed(std::process::ExitStatus),
    /// Command exceeded the timeout and was killed.
    TimedOut,
}

/// Outcome of running a shell command with output capture and timeout.
pub(super) enum CommandOutputOutcome {
    /// Command completed within the timeout, producing this stdout.
    Completed(String),
    /// Command exceeded the timeout and was killed.
    TimedOut,
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
            gate_type: "command_succeeds".to_string(),
            description: format!("command succeeds: {}", raw_cmd),
            reason: Some(reason),
            intent: None,
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
                intent: None,
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
            intent: None,
        },
        Err(e) => GateResult {
            passed: false,
            gate_type: "command_succeeds".to_string(),
            description: format!("command succeeds: {}", cmd),
            reason: Some(format!("failed to run command '{}': {}", cmd, e)),
            intent: None,
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
            gate_type: "command_output".to_string(),
            description: format!("command output matches '{}'", expect),
            reason: Some(reason),
            intent: None,
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
                intent: None,
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
            intent: None,
        },
        Err(e) => GateResult {
            passed: false,
            gate_type: "command_output".to_string(),
            description: format!("command output matches '{}'", expect),
            reason: Some(format!("failed to run command '{}': {}", cmd, e)),
            intent: None,
        },
    }
}

// ---------------------------------------------------------------------------
// Command execution with timeout enforcement
// (Issue #1, #6: Use try_wait() polling loop)
// ---------------------------------------------------------------------------

// [run-shell-with-timeout]
/// Run a shell command with timeout enforcement using `try_wait()` polling.
pub(super) fn run_shell_with_timeout(
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
