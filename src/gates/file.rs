// src/gates/file.rs
//
// ## Index
// - [eval-file-exists]  eval_file_exists()  — check if a single file exists
// - [eval-files-exist]  eval_files_exist()  — check if multiple files exist

use std::path::Path;

use crate::config::GateConfig;

use super::evaluator::{GateContext, GateResult};
use super::template::resolve_template_plain;
use super::types::build_template_vars;

// [eval-file-exists]
pub(super) fn eval_file_exists(gate: &GateConfig, ctx: &GateContext) -> GateResult {
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
        evaluable: true,
        gate_type: "file_exists".to_string(),
        description: format!("file '{}' exists", resolved),
        reason: if exists {
            None
        } else {
            Some(format!("file '{}' does not exist", resolved))
        },
        intent: None,
    }
}

// [eval-files-exist]
pub(super) fn eval_files_exist(gate: &GateConfig, ctx: &GateContext) -> GateResult {
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
        evaluable: true,
        gate_type: "files_exist".to_string(),
        description: format!("{} file(s) must exist", paths.len()),
        reason: if passed {
            None
        } else {
            Some(format!("missing files: {}", missing.join(", ")))
        },
        intent: None,
    }
}
