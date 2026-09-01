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
// - [emit-working-dir] emit_working_dir() — the directory this emit's commands run in
// - [resolve-emit]     resolve_emit()     — run commands, interpolate templates, produce event fields

use std::collections::{BTreeMap, HashMap};
use std::path::Path;

use crate::config::transitions::EmitConfig;
use crate::gates::command::{run_shell_output_with_timeout, CommandOutputOutcome};
use crate::gates::template::{find_unresolved_vars, resolve_template_plain};
use crate::gates::types::{resolve_anchor, Anchor};
use crate::ledger::chain::Ledger;

/// Timeout for a single emit command. Emit commands are expected to be quick
/// value derivations (e.g. `git rev-parse HEAD`), not build/test steps.
const EMIT_COMMAND_TIMEOUT_SECS: u64 = 30;

// [emit-working-dir]
/// The directory this emit's derivation commands run in.
///
/// The same two anchors a gate has, read by the same function, for the same
/// reason: a transition can run a caller-anchored gate *and* an emit, and
/// before #48 only the gate could say which tree it meant. The emit then
/// derived its value at the project root — so `fix_commit` could attest that
/// the caller's HEAD carries the fix and record, two lines later, the project's
/// unrelated HEAD as the commit that resolved it.
///
/// An anchor the engine cannot read fails the emit rather than falling back to
/// the project root, which blocks the whole transition before anything is
/// appended. The fallback is indistinguishable from an emit that chose the
/// project deliberately, and a value derived somewhere its author did not mean
/// is a false record — worse than the block it replaces.
fn emit_working_dir<'a>(
    emit: &EmitConfig,
    working_dir: &'a Path,
    caller_dir: &'a Path,
) -> Result<&'a Path, String> {
    match resolve_anchor(emit.anchor.as_ref()) {
        Ok(Anchor::Project) => Ok(working_dir),
        Ok(Anchor::Caller) => Ok(caller_dir),
        Err(e) => Err(format!("emit '{}' {}", emit.event, e)),
    }
}

// [resolve-emit]
/// Resolve one emitted event's fields into concrete values.
///
/// Builds a template variable map from three sources, in increasing precedence:
/// 1. the most recent value of each field across `ledger` (run-context
///    inheritance — `project`, `run`, `auditor`, …),
/// 2. `state_params` (positional args such as `item_id`, plus `key=value` args),
/// 3. the trimmed stdout of each `emit.commands` entry, run at the emit's own
///    anchor — see [`emit_working_dir`].
///
/// Then resolves each `emit.fields` template with `{{var}}` substitution.
///
/// Returns `Err` (blocking the transition, before anything is appended) if the
/// anchor is unreadable, if a command exits non-zero / times out / fails to
/// spawn, or if a resolved field still contains an unresolved `{{var}}`
/// placeholder.
pub fn resolve_emit(
    emit: &EmitConfig,
    state_params: &HashMap<String, String>,
    ledger: &Ledger,
    working_dir: &Path,
    caller_dir: &Path,
) -> Result<BTreeMap<String, String>, String> {
    // Resolved before anything else, and whether or not this emit runs a
    // command: an anchor the engine cannot read is a defect in the config, not
    // a property of the commands that happen to be declared beside it.
    let command_dir = emit_working_dir(emit, working_dir, caller_dir)?;

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

    // 3. Run derivation commands at this emit's anchor; bind trimmed stdout to
    //    the var name.
    for (name, cmd) in &emit.commands {
        match run_shell_output_with_timeout(cmd, command_dir, EMIT_COMMAND_TIMEOUT_SECS) {
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

    /// An emit with no anchor — the shape every emit had before #48.
    fn emit_with(commands: HashMap<String, String>, fields: HashMap<String, String>) -> EmitConfig {
        EmitConfig {
            event: "finding_resolved".to_string(),
            commands,
            fields,
            anchor: None,
        }
    }

    /// Two directories that disagree about the same file, so a resolved value
    /// names which one its command ran in.
    fn two_trees() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let dir = tempdir().unwrap();
        let project = dir.path().join("project");
        let caller = dir.path().join("caller");
        std::fs::create_dir_all(&project).unwrap();
        std::fs::create_dir_all(&caller).unwrap();
        std::fs::write(project.join("head"), "a131e39\n").unwrap();
        std::fs::write(caller.join("head"), "afd0292\n").unwrap();
        (dir, project, caller)
    }

    #[test]
    fn resolves_from_args_and_ledger() {
        let emit = emit_with(
            HashMap::new(),
            HashMap::from([
                ("id".to_string(), "{{item_id}}".to_string()),
                ("project".to_string(), "{{project}}".to_string()),
                ("phase".to_string(), "fix_loop".to_string()),
            ]),
        );
        let ledger = ledger_with(&[("project", "holtz"), ("id", "BH-009")]);
        let params = HashMap::from([("item_id".to_string(), "BH-001".to_string())]);
        let out = resolve_emit(&emit, &params, &ledger, Path::new("."), Path::new(".")).unwrap();
        // args win over ledger for id; project inherited from ledger; literal passes through.
        assert_eq!(out.get("id").unwrap(), "BH-001");
        assert_eq!(out.get("project").unwrap(), "holtz");
        assert_eq!(out.get("phase").unwrap(), "fix_loop");
    }

    #[test]
    fn binds_command_output() {
        let emit = emit_with(
            HashMap::from([("commit_hash".to_string(), "printf abc1234".to_string())]),
            HashMap::from([("commit_hash".to_string(), "{{commit_hash}}".to_string())]),
        );
        let ledger = ledger_with(&[("project", "holtz")]);
        let out = resolve_emit(
            &emit,
            &HashMap::new(),
            &ledger,
            Path::new("."),
            Path::new("."),
        )
        .unwrap();
        assert_eq!(out.get("commit_hash").unwrap(), "abc1234");
    }

    #[test]
    fn unresolved_var_is_error() {
        let emit = emit_with(
            HashMap::new(),
            HashMap::from([("id".to_string(), "{{item_id}}".to_string())]),
        );
        let ledger = ledger_with(&[("project", "holtz")]);
        let err = resolve_emit(
            &emit,
            &HashMap::new(),
            &ledger,
            Path::new("."),
            Path::new("."),
        )
        .unwrap_err();
        assert!(err.contains("unresolved"), "got: {err}");
    }

    #[test]
    fn failing_command_is_error() {
        let emit = emit_with(
            HashMap::from([("x".to_string(), "exit 3".to_string())]),
            HashMap::new(),
        );
        let ledger = ledger_with(&[("project", "holtz")]);
        let err = resolve_emit(
            &emit,
            &HashMap::new(),
            &ledger,
            Path::new("."),
            Path::new("."),
        )
        .unwrap_err();
        assert!(err.contains("exited with status 3"), "got: {err}");
    }

    #[test]
    fn commands_run_at_the_project_by_default() {
        // The pre-#48 behaviour, kept: an emit that says nothing about
        // anchoring derives its value from the project's tree.
        let (_dir, project, caller) = two_trees();
        let emit = emit_with(
            HashMap::from([("head".to_string(), "cat head".to_string())]),
            HashMap::from([("commit_hash".to_string(), "{{head}}".to_string())]),
        );
        let ledger = ledger_with(&[("project", "holtz")]);
        let out = resolve_emit(&emit, &HashMap::new(), &ledger, &project, &caller).unwrap();
        assert_eq!(out.get("commit_hash").unwrap(), "a131e39");
    }

    #[test]
    fn caller_anchored_commands_run_in_the_callers_tree() {
        // The #48 reproduction: same config, same invocation, and the recorded
        // value is now the caller's HEAD rather than a commit that contains
        // none of the work.
        let (_dir, project, caller) = two_trees();
        let mut emit = emit_with(
            HashMap::from([("head".to_string(), "cat head".to_string())]),
            HashMap::from([("commit_hash".to_string(), "{{head}}".to_string())]),
        );
        emit.anchor = Some(toml::Value::String("caller".to_string()));
        let ledger = ledger_with(&[("project", "holtz")]);
        let out = resolve_emit(&emit, &HashMap::new(), &ledger, &project, &caller).unwrap();
        assert_eq!(out.get("commit_hash").unwrap(), "afd0292");
    }

    #[test]
    fn an_unreadable_anchor_fails_the_emit() {
        // Not a fallback to the project: that is spelled exactly like a
        // deliberate project anchor, and the record it produces is false.
        let (_dir, project, caller) = two_trees();
        for anchor in [
            toml::Value::String("callr".to_string()),
            toml::Value::Integer(1),
        ] {
            let mut emit = emit_with(
                HashMap::from([("head".to_string(), "cat head".to_string())]),
                HashMap::from([("commit_hash".to_string(), "{{head}}".to_string())]),
            );
            emit.anchor = Some(anchor.clone());
            let ledger = ledger_with(&[("project", "holtz")]);
            let err = resolve_emit(&emit, &HashMap::new(), &ledger, &project, &caller).unwrap_err();
            assert!(
                err.contains("emit 'finding_resolved'") && err.contains("anchor"),
                "anchor = {anchor:?} must fail the emit by name, got: {err}"
            );
        }
    }
}
