// src/cli/hooks_cmd.rs
//
// Hook generation and evaluation commands.
//
// ## Index
// - [cmd-hook-generate] cmd_hook_generate() — generate hook scripts for a harness
// - [cmd-hook-eval]     cmd_hook_eval()     — evaluate hook rules against current state

use std::collections::BTreeMap;
use std::path::PathBuf;

use super::commands::{
    load_config, open_targeted_ledger, resolve_config_dir, LedgerTargeting, EXIT_CONFIG_ERROR,
    EXIT_GATE_FAILED, EXIT_SUCCESS,
};
use super::output::{
    CommandOutput, CommandResult, HookAutoRecord, HookEvalData, HookEvalMessage, HookMonitorWarning,
};
use crate::config::hooks::HookEvent;
use crate::hooks::eval::{evaluate_hooks, HookEvalRequest};

// ---------------------------------------------------------------------------
// hook generate
// ---------------------------------------------------------------------------

// [cmd-hook-generate]
pub fn cmd_hook_generate(
    config_dir: &str,
    harness: &Option<String>,
    output_dir: &Option<String>,
) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let harness_name = harness.as_deref().unwrap_or("cc");

    let generator = match crate::hooks::HookGenerator::new() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("Cannot initialize hook generator: {}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    let out_path = output_dir.as_ref().map(PathBuf::from);
    let hooks = match generator.generate(&config, harness_name, out_path.as_deref()) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("Hook generation failed: {}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    if output_dir.is_some() {
        let dir = output_dir.as_ref().unwrap();
        println!("Generated {} hook scripts in {}/", hooks.len(), dir);
        for hook in &hooks {
            println!("  {} ({})", hook.filename, hook.hook_type);
        }
    } else {
        // Print each hook to stdout with separators
        for hook in &hooks {
            println!("# === {} ({}) ===", hook.filename, hook.hook_type);
            println!("{}", hook.content);
        }
    }

    // Print suggested hooks.json configuration
    let hooks_dir = output_dir.as_deref().unwrap_or(".hooks");
    println!("\n# Suggested hooks.json configuration:");
    println!(
        "{}",
        crate::hooks::HookGenerator::suggested_hooks_json(&hooks, hooks_dir)
    );

    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------
// hook eval
// ---------------------------------------------------------------------------

// [cmd-hook-eval]
/// Evaluate hook rules against current protocol state.
///
/// This is a machine interface — output is always JSON.
/// Returns exit code 1 for "block", 0 for "allow" or "warn".
/// On config/ledger errors, returns "allow" (don't block agent on broken config).
pub fn cmd_hook_eval(
    config_dir: &str,
    event: &str,
    tool: &Option<String>,
    file: &Option<String>,
    output_text: &Option<String>,
    targeting: &LedgerTargeting,
) -> Box<dyn CommandOutput> {
    let config_path = resolve_config_dir(config_dir);

    // Load config — on failure, return allow
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err(_) => {
            return Box::new(CommandResult::ok_with_exit_code(
                "hook_eval",
                HookEvalData {
                    decision: "allow".to_string(),
                    messages: vec![],
                    auto_records: vec![],
                    monitor_warnings: vec![],
                },
                EXIT_SUCCESS,
            ));
        }
    };

    // Open ledger — on failure, return allow
    let mut ledger = match open_targeted_ledger(&config, targeting, &config_path) {
        Ok((l, _mode)) => l,
        Err(_) => {
            return Box::new(CommandResult::ok_with_exit_code(
                "hook_eval",
                HookEvalData {
                    decision: "allow".to_string(),
                    messages: vec![],
                    auto_records: vec![],
                    monitor_warnings: vec![],
                },
                EXIT_SUCCESS,
            ));
        }
    };

    // Parse event string
    let hook_event = match event {
        "PreToolUse" => HookEvent::PreToolUse,
        "PostToolUse" => HookEvent::PostToolUse,
        "Stop" => HookEvent::Stop,
        _ => {
            return Box::new(CommandResult::<HookEvalData>::err(
                "hook_eval",
                EXIT_CONFIG_ERROR,
                "invalid_event",
                format!(
                    "Unknown hook event '{}'. Valid: PreToolUse, PostToolUse, Stop",
                    event
                ),
            ));
        }
    };

    let request = HookEvalRequest {
        event: hook_event,
        tool: tool.clone(),
        file: file.clone(),
        output_text: output_text.clone(),
    };

    let working_dir = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let result = evaluate_hooks(&config, &ledger, &request, &working_dir);

    // Auto-record events
    for auto in &result.auto_records {
        let mut fields = BTreeMap::new();
        for (k, v) in &auto.fields {
            fields.insert(k.clone(), v.clone());
        }
        if let Err(e) = ledger.append(&auto.event_type, fields) {
            eprintln!("Warning: auto-record failed: {}", e);
        }
    }

    // Map to output types
    let exit_code = if result.decision == "block" {
        EXIT_GATE_FAILED
    } else {
        EXIT_SUCCESS
    };

    let data = HookEvalData {
        decision: result.decision,
        messages: result
            .messages
            .into_iter()
            .map(|m| HookEvalMessage {
                source: m.source,
                rule_index: m.rule_index,
                action: m.action,
                message: m.message,
            })
            .collect(),
        auto_records: result
            .auto_records
            .into_iter()
            .map(|a| HookAutoRecord {
                event_type: a.event_type,
                fields: a.fields,
            })
            .collect(),
        monitor_warnings: result
            .monitor_warnings
            .into_iter()
            .map(|w| HookMonitorWarning {
                name: w.name,
                message: w.message,
            })
            .collect(),
    };

    Box::new(CommandResult::ok_with_exit_code(
        "hook_eval",
        data,
        exit_code,
    ))
}
