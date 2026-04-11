// src/hooks/eval.rs
//
// Hook evaluation engine — runtime enforcement of hooks, write-gated guards,
// managed path checks, and monitors against the current ledger state.
//
// ## Index
// - HookEvalRequest           — incoming evaluation request
// - HookEvalResult            — aggregate evaluation result
// - HookMessage               — a single enforcement message
// - AutoRecordResult          — an event to auto-record
// - MonitorWarning            — a monitor that fired
// - evaluate_hooks            — main evaluation entry point
// - derive_current_state      — find current state from ledger
// - hook_matches              — check if hook matches request + state
// - matches_filter            — check path filter against request
// - glob_match                — simple glob matching
// - eval_hook_condition       — evaluate gate/check condition
// - eval_managed_paths        — check paths.managed
// - eval_write_gated          — check write_gated guards
// - eval_monitors             — evaluate monitor triggers
// - interpolate_message       — template substitution in messages
// - resolve_tool_template     — replace {tool.file_path} in templates
// - compare_threshold         — numeric comparison helpers

use std::collections::HashMap;
use std::path::Path;

use serde::Serialize;

use crate::config::hooks::{HookConfig, HookEvent};
use crate::config::ProtocolConfig;
use crate::gates::evaluator::{evaluate_gate, GateContext};
use crate::ledger::chain::Ledger;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Request to evaluate hooks against the current state.
#[derive(Debug)]
pub struct HookEvalRequest {
    /// The hook event type being evaluated.
    pub event: HookEvent,
    /// Tool name (e.g. "Edit", "Write", "Bash").
    pub tool: Option<String>,
    /// File path being operated on.
    pub file: Option<String>,
    /// Agent output text (for Stop hooks).
    pub output_text: Option<String>,
}

/// Aggregate result of hook evaluation.
#[derive(Debug, Serialize)]
pub struct HookEvalResult {
    /// Overall decision: "block", "warn", or "allow".
    pub decision: String,
    /// Messages from hooks, write_gated, and managed path checks.
    pub messages: Vec<HookMessage>,
    /// Events to auto-record in the ledger.
    pub auto_records: Vec<AutoRecordResult>,
    /// Monitor warnings.
    pub monitor_warnings: Vec<MonitorWarning>,
}

/// A single enforcement message.
#[derive(Debug, Serialize)]
pub struct HookMessage {
    /// Source of the message: "hook", "write_gated", or "managed_path".
    pub source: String,
    /// Index of the rule that produced this message.
    pub rule_index: usize,
    /// Action: "block" or "warn".
    pub action: String,
    /// Human-readable message.
    pub message: String,
}

/// An event to auto-record in the ledger.
#[derive(Debug, Serialize)]
pub struct AutoRecordResult {
    /// Event type to record.
    pub event_type: String,
    /// Fields for the event.
    pub fields: HashMap<String, String>,
}

/// A monitor warning.
#[derive(Debug, Serialize)]
pub struct MonitorWarning {
    /// Monitor name.
    pub name: String,
    /// Warning message.
    pub message: String,
}

// ---------------------------------------------------------------------------
// Main evaluation entry point
// ---------------------------------------------------------------------------

/// Evaluate all hooks, guards, and monitors against the current state.
///
/// Evaluation order:
/// 1. Managed path check (PreToolUse Edit/Write only)
/// 2. Write-gated guards (PreToolUse Edit/Write only)
/// 3. Hooks matching event/tool/state/filter
/// 4. Monitors matching current state
///
/// Decision: block > warn > allow (most restrictive wins).
pub fn evaluate_hooks(
    config: &ProtocolConfig,
    ledger: &Ledger,
    request: &HookEvalRequest,
    working_dir: &Path,
) -> HookEvalResult {
    let mut messages: Vec<HookMessage> = Vec::new();
    let mut auto_records: Vec<AutoRecordResult> = Vec::new();

    let current_state = derive_current_state(config, ledger);

    // 1. Managed path check (PreToolUse Edit/Write only)
    eval_managed_paths(config, request, &mut messages);

    // 2. Write-gated guards (PreToolUse Edit/Write only)
    eval_write_gated(config, &current_state, request, &mut messages);

    // 3. Hooks
    for (idx, hook) in config.hooks.iter().enumerate() {
        if !hook_matches(hook, request, &current_state) {
            continue;
        }

        // Auto-record hooks
        if let Some(ref auto) = hook.auto_record {
            let mut fields = HashMap::new();
            for (k, v) in &auto.fields {
                fields.insert(k.clone(), resolve_tool_template(v, request));
            }
            auto_records.push(AutoRecordResult {
                event_type: auto.event_type.clone(),
                fields,
            });
            continue;
        }

        // Gate/check hooks
        let should_fire =
            eval_hook_condition(hook, config, ledger, &current_state, request, working_dir);
        if should_fire {
            let action = hook.action.as_deref().unwrap_or("warn").to_string();
            let count = count_events_since_last_transition(ledger);
            let msg = hook
                .message
                .as_deref()
                .map(|m| interpolate_message(m, &current_state, count))
                .unwrap_or_default();
            messages.push(HookMessage {
                source: "hook".to_string(),
                rule_index: idx,
                action,
                message: msg,
            });
        }
    }

    // 4. Monitors
    let monitor_warnings = eval_monitors(config, ledger, &current_state);

    // Determine overall decision (block > warn > allow)
    let decision = if messages.iter().any(|m| m.action == "block") {
        "block".to_string()
    } else if messages.iter().any(|m| m.action == "warn") || !monitor_warnings.is_empty() {
        "warn".to_string()
    } else {
        "allow".to_string()
    };

    HookEvalResult {
        decision,
        messages,
        auto_records,
        monitor_warnings,
    }
}

// ---------------------------------------------------------------------------
// Helper: derive current state
// ---------------------------------------------------------------------------

/// Find the current state from the last state_transition in the ledger,
/// or return the initial state from config.
fn derive_current_state(config: &ProtocolConfig, ledger: &Ledger) -> String {
    // Scan backwards for the last state_transition
    for entry in ledger.entries().iter().rev() {
        if entry.event_type == "state_transition" {
            if let Some(to) = entry.fields.get("to") {
                return to.clone();
            }
        }
    }

    // Fall back to initial state
    config.initial_state().unwrap_or("unknown").to_string()
}

// ---------------------------------------------------------------------------
// Helper: hook matching
// ---------------------------------------------------------------------------

/// Check if a hook matches the current request and state.
fn hook_matches(hook: &HookConfig, request: &HookEvalRequest, current_state: &str) -> bool {
    // Event must match
    if hook.event != request.event {
        return false;
    }

    // Tool filter
    if let Some(ref tools) = hook.tools {
        match &request.tool {
            Some(tool) => {
                if !tools.iter().any(|t| t == tool) {
                    return false;
                }
            }
            None => return false,
        }
    }

    // State filter (positive)
    if let Some(ref states) = hook.states {
        if !states.iter().any(|s| s == current_state) {
            return false;
        }
    }

    // State filter (negative)
    if let Some(ref states_not) = hook.states_not {
        if states_not.iter().any(|s| s == current_state) {
            return false;
        }
    }

    // Path filter
    if let Some(ref filter) = hook.filter {
        if !matches_filter(filter, request) {
            return false;
        }
    }

    true
}

/// Check path filter against request.
fn matches_filter(filter: &crate::config::hooks::HookFilter, request: &HookEvalRequest) -> bool {
    let file_path = match &request.file {
        Some(f) => f.as_str(),
        None => return true, // No file to filter on; pass through
    };

    // path_matches: must match if specified
    if let Some(ref pattern) = filter.path_matches {
        if !glob_match(pattern, file_path) {
            return false;
        }
    }

    // path_not_matches: must NOT match if specified
    if let Some(ref pattern) = filter.path_not_matches {
        if glob_match(pattern, file_path) {
            return false;
        }
    }

    true
}

/// Simple glob matching.
///
/// Supports:
/// - `*` matches any characters within a single path segment (no `/`)
/// - `**` matches any depth of path segments (including zero)
/// - `*.ext` matches files ending in `.ext` anywhere
fn glob_match(pattern: &str, path: &str) -> bool {
    // Normalize separators
    let pattern = pattern.replace('\\', "/");
    let path = path.replace('\\', "/");

    // Simple case: pattern is just `*.ext` (no slashes) — match basename
    if !pattern.contains('/') && pattern.starts_with('*') && !pattern.contains("**") {
        // e.g. "*.rs" matches "src/main.rs"
        let suffix = &pattern[1..]; // ".rs"
        return path.ends_with(suffix);
    }

    glob_match_recursive(&pattern, &path)
}

/// Recursive glob matching engine.
fn glob_match_recursive(pattern: &str, text: &str) -> bool {
    if pattern.is_empty() {
        return text.is_empty();
    }

    // Handle **/ (matches any depth)
    if let Some(rest) = pattern.strip_prefix("**/") {
        // Try matching rest against text, or skip one segment of text
        if glob_match_recursive(rest, text) {
            return true;
        }
        // Try removing one segment from text
        if let Some(slash_pos) = text.find('/') {
            return glob_match_recursive(pattern, &text[slash_pos + 1..]);
        }
        return false;
    }

    // Handle trailing **
    if pattern == "**" {
        return true;
    }

    // Handle * (matches within segment)
    if pattern.starts_with('*') && !pattern.starts_with("**") {
        let rest = &pattern[1..];
        // Try matching rest against progressively shorter text (within segment)
        for i in 0..=text.len() {
            // Don't let * cross a slash
            if i > 0 && text.as_bytes()[i - 1] == b'/' {
                break;
            }
            if glob_match_recursive(rest, &text[i..]) {
                return true;
            }
        }
        return false;
    }

    // Literal character match
    if let (Some(pc), Some(tc)) = (pattern.chars().next(), text.chars().next()) {
        if pc == tc {
            return glob_match_recursive(&pattern[pc.len_utf8()..], &text[tc.len_utf8()..]);
        }
    }

    false
}

// ---------------------------------------------------------------------------
// Helper: hook condition evaluation
// ---------------------------------------------------------------------------

/// Evaluate the condition for a hook. Returns true if the hook should fire.
///
/// For gate hooks: fires when the gate FAILS (condition not met).
/// For check hooks: fires when the check condition IS met.
fn eval_hook_condition(
    hook: &HookConfig,
    config: &ProtocolConfig,
    ledger: &Ledger,
    current_state: &str,
    request: &HookEvalRequest,
    working_dir: &Path,
) -> bool {
    // Gate-based hook: fire if gate fails
    if let Some(ref gate) = hook.gate {
        let ctx = GateContext {
            ledger,
            config,
            current_state,
            state_params: HashMap::new(),
            working_dir: working_dir.to_path_buf(),
            event_fields: None,
        };
        let result = evaluate_gate(gate, &ctx);
        return !result.passed;
    }

    // Check-based hook
    if let Some(ref check) = hook.check {
        match check.check_type.as_str() {
            "output_contains_any" => {
                if let Some(ref patterns) = check.patterns {
                    if let Some(ref output) = request.output_text {
                        let output_lower = output.to_lowercase();
                        return patterns
                            .iter()
                            .any(|p| output_lower.contains(&p.to_lowercase()));
                    }
                }
                return false;
            }
            "event_count_since_last_transition" => {
                let count = count_events_since_last_transition(ledger);
                let threshold = check.threshold.unwrap_or(0);
                let compare = check.compare.as_deref().unwrap_or("gte");
                return compare_threshold(count as i64, threshold, compare);
            }
            "query" => {
                // Stub: don't block
                return false;
            }
            _ => return false,
        }
    }

    false
}

/// Count events since the last state_transition in the ledger.
fn count_events_since_last_transition(ledger: &Ledger) -> usize {
    let entries = ledger.entries();
    let mut count = 0;
    for entry in entries.iter().rev() {
        if entry.event_type == "state_transition" {
            break;
        }
        // Skip genesis
        if entry.event_type == "genesis" {
            continue;
        }
        count += 1;
    }
    count
}

/// Compare count against threshold using the given comparison operator.
fn compare_threshold(count: i64, threshold: i64, compare: &str) -> bool {
    match compare {
        "gte" => count >= threshold,
        "gt" => count > threshold,
        "lte" => count <= threshold,
        "lt" => count < threshold,
        "eq" => count == threshold,
        _ => count >= threshold,
    }
}

// ---------------------------------------------------------------------------
// Helper: managed paths
// ---------------------------------------------------------------------------

/// Check if the request targets a managed path (PreToolUse Edit/Write only).
fn eval_managed_paths(
    config: &ProtocolConfig,
    request: &HookEvalRequest,
    messages: &mut Vec<HookMessage>,
) {
    if request.event != HookEvent::PreToolUse {
        return;
    }

    let tool = match &request.tool {
        Some(t) if t == "Edit" || t == "Write" => t,
        _ => return,
    };

    let file_path = match &request.file {
        Some(f) => f,
        None => return,
    };

    for (idx, managed) in config.paths.managed.iter().enumerate() {
        // Ensure directory-boundary matching: "output" must not match
        // "output-extra/foo" — only "output/foo" or "output" itself.
        let managed_prefix = if managed.ends_with('/') {
            managed.clone()
        } else {
            format!("{}/", managed)
        };
        if file_path == managed.as_str() || file_path.starts_with(&managed_prefix) {
            messages.push(HookMessage {
                source: "managed_path".to_string(),
                rule_index: idx,
                action: "block".to_string(),
                message: format!(
                    "WRITE BLOCKED: {} is managed by sahjhan. {} not permitted on managed paths.",
                    file_path, tool
                ),
            });
            return;
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: write-gated guards
// ---------------------------------------------------------------------------

/// Check write-gated guards (PreToolUse Edit/Write only).
fn eval_write_gated(
    config: &ProtocolConfig,
    current_state: &str,
    request: &HookEvalRequest,
    messages: &mut Vec<HookMessage>,
) {
    if request.event != HookEvent::PreToolUse {
        return;
    }

    match &request.tool {
        Some(t) if t == "Edit" || t == "Write" => {}
        _ => return,
    }

    let file_path = match &request.file {
        Some(f) => f,
        None => return,
    };

    let guards = match &config.guards {
        Some(g) => g,
        None => return,
    };

    for (idx, wg) in guards.write_gated.iter().enumerate() {
        if glob_match(&wg.path, file_path) && !wg.writable_in.iter().any(|s| s == current_state) {
            messages.push(HookMessage {
                source: "write_gated".to_string(),
                rule_index: idx,
                action: "block".to_string(),
                message: wg.message.clone(),
            });
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: monitors
// ---------------------------------------------------------------------------

/// Evaluate monitors against the current state.
fn eval_monitors(
    config: &ProtocolConfig,
    ledger: &Ledger,
    current_state: &str,
) -> Vec<MonitorWarning> {
    let mut warnings = Vec::new();

    for monitor in &config.monitors {
        // State filter
        if let Some(ref states) = monitor.states {
            if !states.iter().any(|s| s == current_state) {
                continue;
            }
        }

        // Evaluate trigger
        if monitor.trigger.trigger_type == "event_count_since_last_transition" {
            let count = count_events_since_last_transition(ledger);
            if count as u64 >= monitor.trigger.threshold {
                let msg = interpolate_message(&monitor.message, current_state, count);
                warnings.push(MonitorWarning {
                    name: monitor.name.clone(),
                    message: msg,
                });
            }
        }
        // Unknown trigger types are silently skipped
    }

    warnings
}

// ---------------------------------------------------------------------------
// Helper: message interpolation
// ---------------------------------------------------------------------------

/// Replace `{current_state}` and `{count}` in a message template.
fn interpolate_message(template: &str, current_state: &str, count: usize) -> String {
    template
        .replace("{current_state}", current_state)
        .replace("{count}", &count.to_string())
}

/// Replace `{tool.file_path}` in a template with the request's file path.
fn resolve_tool_template(template: &str, request: &HookEvalRequest) -> String {
    let file_path = request.file.as_deref().unwrap_or("");
    template.replace("{tool.file_path}", file_path)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_match_star_ext() {
        assert!(glob_match("*.rs", "src/main.rs"));
        assert!(glob_match("*.rs", "main.rs"));
        assert!(!glob_match("*.rs", "main.txt"));
    }

    #[test]
    fn test_glob_match_double_star() {
        assert!(glob_match("**/tests/*", "src/tests/foo.rs"));
        assert!(glob_match("tests/*", "tests/foo.rs"));
        assert!(!glob_match("tests/*", "src/tests/foo.rs"));
    }

    #[test]
    fn test_glob_match_exact() {
        assert!(glob_match("src/main.rs", "src/main.rs"));
        assert!(!glob_match("src/main.rs", "src/lib.rs"));
    }

    #[test]
    fn test_compare_threshold() {
        assert!(compare_threshold(5, 3, "gte"));
        assert!(compare_threshold(3, 3, "gte"));
        assert!(!compare_threshold(2, 3, "gte"));

        assert!(compare_threshold(5, 3, "gt"));
        assert!(!compare_threshold(3, 3, "gt"));

        assert!(compare_threshold(2, 3, "lt"));
        assert!(!compare_threshold(3, 3, "lt"));

        assert!(compare_threshold(3, 3, "eq"));
        assert!(!compare_threshold(4, 3, "eq"));
    }

    #[test]
    fn test_interpolate_message() {
        let msg = interpolate_message(
            "State is {current_state}, {count} events recorded",
            "working",
            42,
        );
        assert_eq!(msg, "State is working, 42 events recorded");
    }

    #[test]
    fn test_resolve_tool_template() {
        let request = HookEvalRequest {
            event: HookEvent::PostToolUse,
            tool: Some("Edit".to_string()),
            file: Some("src/main.rs".to_string()),
            output_text: None,
        };
        let result = resolve_tool_template("{tool.file_path}", &request);
        assert_eq!(result, "src/main.rs");
    }

    #[test]
    fn test_managed_path_prefix_boundary() {
        // Regression: "output" must not match "output-extra/foo"
        let config = ProtocolConfig {
            paths: crate::config::PathsConfig {
                managed: vec!["output".to_string()],
                data_dir: "output/.sahjhan".to_string(),
                render_dir: "output".to_string(),
            },
            protocol: crate::config::ProtocolMeta {
                name: "test".to_string(),
                version: "1.0".to_string(),
                description: "test".to_string(),
            },
            sets: HashMap::new(),
            aliases: HashMap::new(),
            states: HashMap::new(),
            transitions: vec![],
            events: HashMap::new(),
            renders: vec![],
            checkpoints: Default::default(),
            ledgers: HashMap::new(),
            guards: None,
            hooks: vec![],
            monitors: vec![],
        };

        // Should block: file is under managed path
        let mut msgs = Vec::new();
        let req_under = HookEvalRequest {
            event: HookEvent::PreToolUse,
            tool: Some("Edit".to_string()),
            file: Some("output/foo.md".to_string()),
            output_text: None,
        };
        eval_managed_paths(&config, &req_under, &mut msgs);
        assert_eq!(msgs.len(), 1, "should block file under managed path");

        // Should NOT block: file shares prefix but is in different directory
        let mut msgs2 = Vec::new();
        let req_sibling = HookEvalRequest {
            event: HookEvent::PreToolUse,
            tool: Some("Edit".to_string()),
            file: Some("output-extra/foo.md".to_string()),
            output_text: None,
        };
        eval_managed_paths(&config, &req_sibling, &mut msgs2);
        assert_eq!(
            msgs2.len(),
            0,
            "should NOT block file in sibling directory with shared prefix"
        );
    }
}
