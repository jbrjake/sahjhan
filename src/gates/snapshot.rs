// src/gates/snapshot.rs
//
// ## Index
// - [eval-snapshot-compare]       eval_snapshot_compare()       — run command, extract JSON field, compare to reference
// - [resolve-snapshot-reference]  resolve_snapshot_reference()  — look up a "snapshot:key" value in the ledger

use chrono::Utc;
use sha2::{Digest, Sha256};
use std::time::Instant;

use crate::config::GateConfig;

use super::command::{run_shell_output_with_timeout, CommandOutputOutcome};
use super::evaluator::{GateAttestation, GateContext, GateResult};
use super::template::{find_unresolved_vars, resolve_template};
use super::types::{build_template_vars, validate_template_fields};

// [eval-snapshot-compare]
pub(super) fn eval_snapshot_compare(gate: &GateConfig, ctx: &GateContext) -> GateResult {
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

    // Validate fields referenced in the template before interpolation.
    if let Err(reason) = validate_template_fields(raw_cmd, ctx) {
        return GateResult {
            passed: false,
            evaluable: true,
            gate_type: "snapshot_compare".to_string(),
            description: format!(
                "snapshot_compare: {} {} {}",
                extract, compare, reference_raw
            ),
            reason: Some(reason),
            intent: None,
            attestation: None,
        };
    }

    let vars = build_template_vars(ctx);
    let cmd = resolve_template(raw_cmd, &vars);
    let description = format!(
        "snapshot_compare: {} {} {}",
        extract, compare, reference_raw
    );

    let unresolved = find_unresolved_vars(&cmd);
    if !unresolved.is_empty() {
        return GateResult {
            passed: false,
            evaluable: false,
            gate_type: "snapshot_compare".to_string(),
            description,
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

    // Resolve reference — if it starts with "snapshot:", look up in ledger.
    let reference = if let Some(snapshot_key) = reference_raw.strip_prefix("snapshot:") {
        match resolve_snapshot_reference(ctx, snapshot_key) {
            Ok(value) => value,
            Err(reason) => {
                return GateResult {
                    passed: false,
                    evaluable: true,
                    gate_type: "snapshot_compare".to_string(),
                    description,
                    reason: Some(reason),
                    intent: None,
                    attestation: None,
                }
            }
        }
    } else {
        reference_raw.to_string()
    };

    // Run command and get stdout with timeout enforcement.
    let (stdout, status) = match run_shell_output_with_timeout(&cmd, &ctx.working_dir, timeout_secs)
    {
        Ok(CommandOutputOutcome::Completed(s, st)) => (s, st),
        Ok(CommandOutputOutcome::TimedOut) => {
            return GateResult {
                passed: false,
                evaluable: true,
                gate_type: "snapshot_compare".to_string(),
                description,
                reason: Some(format!(
                    "command '{}' timed out after {}s",
                    cmd, timeout_secs
                )),
                intent: None,
                attestation: None,
            }
        }
        Err(e) => {
            return GateResult {
                passed: false,
                evaluable: true,
                gate_type: "snapshot_compare".to_string(),
                description,
                reason: Some(format!("command failed: {}", e)),
                intent: None,
                attestation: None,
            }
        }
    };
    let wall_time_ms = start.elapsed().as_millis() as u64;

    // Parse stdout as JSON and extract the named field.
    let json_value: serde_json::Value = match serde_json::from_str(&stdout) {
        Ok(v) => v,
        Err(e) => {
            return GateResult {
                passed: false,
                evaluable: true,
                gate_type: "snapshot_compare".to_string(),
                description,
                reason: Some(format!("stdout is not valid JSON: {}", e)),
                intent: None,
                attestation: None,
            }
        }
    };

    let extracted = match json_value.get(extract) {
        Some(v) => v.clone(),
        None => {
            return GateResult {
                passed: false,
                evaluable: true,
                gate_type: "snapshot_compare".to_string(),
                description,
                reason: Some(format!("JSON field '{}' not found in output", extract)),
                intent: None,
                attestation: None,
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
                "gt" | "gte" | "lt" | "lte" => {
                    // Numeric comparisons require numeric values; string fallback
                    // only supports equality.
                    return GateResult {
                        passed: false,
                        evaluable: true,
                        gate_type: "snapshot_compare".to_string(),
                        description,
                        reason: Some(format!(
                            "compare '{}' requires numeric values, but extracted '{}' is not a number",
                            compare, extracted_str
                        )),
                        intent: None,
                        attestation: None,
                    };
                }
                other => {
                    return GateResult {
                        passed: false,
                        evaluable: true,
                        gate_type: "snapshot_compare".to_string(),
                        description,
                        reason: Some(format!("unknown compare operator '{}'", other)),
                        intent: None,
                        attestation: None,
                    };
                }
            };
            let attestation = if passed && should_attest {
                let stdout_hash = format!("{:x}", Sha256::digest(stdout.as_bytes()));
                Some(GateAttestation {
                    gate_type: "snapshot_compare".to_string(),
                    command: cmd.clone(),
                    exit_code: status.code().unwrap_or(-1),
                    stdout_hash,
                    wall_time_ms,
                    executed_at: started_at,
                })
            } else {
                None
            };
            return GateResult {
                passed,
                evaluable: true,
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
                intent: None,
                attestation,
            };
        }
    };

    let reference_num: f64 = match reference.parse() {
        Ok(n) => n,
        Err(e) => {
            return GateResult {
                passed: false,
                evaluable: true,
                gate_type: "snapshot_compare".to_string(),
                description,
                reason: Some(format!("reference '{}' is not a number: {}", reference, e)),
                intent: None,
                attestation: None,
            }
        }
    };

    let passed = match compare {
        "gt" => extracted_num > reference_num,
        "gte" => extracted_num >= reference_num,
        "lt" => extracted_num < reference_num,
        "lte" => extracted_num <= reference_num,
        "eq" => (extracted_num - reference_num).abs() < f64::EPSILON,
        other => {
            return GateResult {
                passed: false,
                evaluable: true,
                gate_type: "snapshot_compare".to_string(),
                description,
                reason: Some(format!("unknown compare operator '{}'", other)),
                intent: None,
                attestation: None,
            }
        }
    };

    let attestation = if passed && should_attest {
        let stdout_hash = format!("{:x}", Sha256::digest(stdout.as_bytes()));
        Some(GateAttestation {
            gate_type: "snapshot_compare".to_string(),
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
        intent: None,
        attestation,
    }
}

// [resolve-snapshot-reference]
/// Resolve a `"snapshot:key"` reference from the ledger.
///
/// Searches `ctx.ledger.events_of_type("snapshot")` for the most recent entry
/// whose payload contains a matching `key` field, then returns the `value`
/// field from that entry's payload.
pub(super) fn resolve_snapshot_reference(
    ctx: &GateContext,
    snapshot_key: &str,
) -> Result<String, String> {
    let snapshots = ctx.ledger.events_of_type("snapshot");

    // Walk backwards to find the most recent snapshot with matching key.
    for entry in snapshots.iter().rev() {
        if entry.fields.get("key").map(|k| k.as_str()) == Some(snapshot_key) {
            return entry.fields.get("value").cloned().ok_or_else(|| {
                format!("snapshot with key '{}' has no 'value' field", snapshot_key)
            });
        }
    }

    Err(format!(
        "no snapshot found with key '{}' in ledger",
        snapshot_key
    ))
}
