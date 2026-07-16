// src/state/emit.rs
//
// Resolution of transition-emitted events (see config::transitions::EmitConfig).
//
// When a transition's gates all pass, each declared emit is resolved into a
// concrete set of event fields and appended to the ledger — letting a
// transition record the domain-state event it implies (e.g. fix_commit ->
// finding_resolved) without the agent issuing a second, redundant command.
//
// ## Index
// - [resolve-emit] resolve_emit() — run commands, interpolate templates, produce event fields

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use crate::config::transitions::EmitConfig;
use crate::gates::command::{run_shell_output_with_timeout, CommandOutputOutcome};
use crate::gates::template::{find_unresolved_vars, resolve_template_plain};
use crate::ledger::chain::Ledger;

/// Timeout for a single emit command. Emit commands are expected to be quick
/// value derivations (e.g. `git rev-parse HEAD`), not build/test steps.
const EMIT_COMMAND_TIMEOUT_SECS: u64 = 30;

// [resolve-emit]
/// Resolve one emitted event's fields into concrete values.
///
/// Builds a template variable map from three sources, in increasing precedence:
/// 1. the most recent value of each field across `ledger` (run-context
///    inheritance — `project`, `run`, `auditor`, …),
/// 2. `state_params` (positional args such as `item_id`, plus `key=value` args),
/// 3. the trimmed stdout of each `emit.commands` entry.
///
/// Then resolves each `emit.fields` template with `{{var}}` substitution.
///
/// Returns `Err` (blocking the transition, before anything is appended) if a
/// command exits non-zero / times out / fails to spawn, or if a resolved field
/// still contains an unresolved `{{var}}` placeholder.
pub fn resolve_emit(
    emit: &EmitConfig,
    state_params: &HashMap<String, String>,
    ledger: &Ledger,
    working_dir: &Path,
) -> Result<BTreeMap<String, String>, String> {
    let mut vars: HashMap<String, String> = HashMap::new();

    // 1. Inherit the most recent value of each field seen in the ledger.
    //    entries() is in append order, so later entries overwrite earlier ones.
    for entry in ledger.entries() {
        for (key, value) in &entry.fields {
            vars.insert(key.clone(), value.clone());
        }
    }

    // 2. Overlay transition state_params (args like item_id take precedence).
    for (key, value) in state_params {
        vars.insert(key.clone(), value.clone());
    }

    // 3. Run derivation commands; bind trimmed stdout to the var name.
    for (name, cmd) in &emit.commands {
        match run_shell_output_with_timeout(cmd, working_dir, EMIT_COMMAND_TIMEOUT_SECS) {
            Ok(CommandOutputOutcome::Completed(stdout, stderr, status)) => {
                if !status.success() {
                    return Err(format!(
                        "emit '{}' command '{}' exited with status {}: {}",
                        emit.event,
                        cmd,
                        status.code().unwrap_or(-1),
                        stderr.trim()
                    ));
                }
                vars.insert(name.clone(), stdout.trim().to_string());
            }
            Ok(CommandOutputOutcome::TimedOut) => {
                return Err(format!(
                    "emit '{}' command '{}' timed out after {}s",
                    emit.event, cmd, EMIT_COMMAND_TIMEOUT_SECS
                ));
            }
            Err(e) => {
                return Err(format!(
                    "emit '{}' command '{}' failed to run: {}",
                    emit.event, cmd, e
                ));
            }
        }
    }

    // 4. Resolve each field template.
    let mut resolved: BTreeMap<String, String> = BTreeMap::new();
    for (field, template) in &emit.fields {
        let value = resolve_template_plain(template, &vars);
        let missing = find_unresolved_vars(&value);
        if !missing.is_empty() {
            return Err(format!(
                "emit '{}' field '{}' has unresolved template var(s): {}",
                emit.event,
                field,
                missing.join(", ")
            ));
        }
        resolved.insert(field.clone(), value);
    }

    Ok(resolved)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn ledger_with(fields: &[(&str, &str)]) -> Ledger {
        let dir = tempdir().unwrap();
        let path = dir.path().join("ledger.jsonl");
        let mut ledger = Ledger::init(&path, "test", "1.0.0").unwrap();
        let map: BTreeMap<String, String> = fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        ledger.append("finding", map).unwrap();
        // Keep the tempdir alive for the ledger's lifetime by leaking it.
        std::mem::forget(dir);
        ledger
    }

    #[test]
    fn resolves_from_args_and_ledger() {
        let emit = EmitConfig {
            event: "finding_resolved".to_string(),
            commands: HashMap::new(),
            fields: HashMap::from([
                ("id".to_string(), "{{item_id}}".to_string()),
                ("project".to_string(), "{{project}}".to_string()),
                ("phase".to_string(), "fix_loop".to_string()),
            ]),
        };
        let ledger = ledger_with(&[("project", "holtz"), ("id", "BH-009")]);
        let params = HashMap::from([("item_id".to_string(), "BH-001".to_string())]);
        let out = resolve_emit(&emit, &params, &ledger, Path::new(".")).unwrap();
        // args win over ledger for id; project inherited from ledger; literal passes through.
        assert_eq!(out.get("id").unwrap(), "BH-001");
        assert_eq!(out.get("project").unwrap(), "holtz");
        assert_eq!(out.get("phase").unwrap(), "fix_loop");
    }

    #[test]
    fn binds_command_output() {
        let emit = EmitConfig {
            event: "finding_resolved".to_string(),
            commands: HashMap::from([("commit_hash".to_string(), "printf abc1234".to_string())]),
            fields: HashMap::from([("commit_hash".to_string(), "{{commit_hash}}".to_string())]),
        };
        let ledger = ledger_with(&[("project", "holtz")]);
        let out = resolve_emit(&emit, &HashMap::new(), &ledger, Path::new(".")).unwrap();
        assert_eq!(out.get("commit_hash").unwrap(), "abc1234");
    }

    #[test]
    fn unresolved_var_is_error() {
        let emit = EmitConfig {
            event: "finding_resolved".to_string(),
            commands: HashMap::new(),
            fields: HashMap::from([("id".to_string(), "{{item_id}}".to_string())]),
        };
        let ledger = ledger_with(&[("project", "holtz")]);
        let err = resolve_emit(&emit, &HashMap::new(), &ledger, Path::new(".")).unwrap_err();
        assert!(err.contains("unresolved"), "got: {err}");
    }

    #[test]
    fn failing_command_is_error() {
        let emit = EmitConfig {
            event: "finding_resolved".to_string(),
            commands: HashMap::from([("x".to_string(), "exit 3".to_string())]),
            fields: HashMap::new(),
        };
        let ledger = ledger_with(&[("project", "holtz")]);
        let err = resolve_emit(&emit, &HashMap::new(), &ledger, Path::new(".")).unwrap_err();
        assert!(err.contains("exited with status 3"), "got: {err}");
    }
}
