// src/mermaid.rs
//
// Protocol visualization: Mermaid stateDiagram-v2 and ASCII tree-walk output.
//
// ## Index
// - [generate-mermaid]   generate_mermaid()   — raw Mermaid stateDiagram-v2 text
// - [generate-ascii]     generate_ascii()     — ASCII tree-walk diagram

use crate::config::{GateConfig, ProtocolConfig};
use std::collections::{HashMap, HashSet};

/// Sanitize a state name for use in Mermaid: replace hyphens with underscores.
fn sanitize_id(name: &str) -> String {
    name.replace('-', "_")
}

/// Produce a short human-readable label for a gate (leaf or composite).
fn gate_short_label(gate: &GateConfig) -> String {
    match gate.gate_type.as_str() {
        "any_of" => format!("any_of({})", gate.gates.len()),
        "all_of" => format!("all_of({})", gate.gates.len()),
        "not" => {
            if let Some(child) = gate.gates.first() {
                format!("not({})", child.gate_type)
            } else {
                "not(?)".to_string()
            }
        }
        "k_of_n" => {
            let k = gate
                .params
                .get("k")
                .and_then(|v| v.as_integer())
                .unwrap_or(0);
            let n = gate.gates.len();
            format!("{}-of-{}", k, n)
        }
        leaf => leaf.to_string(),
    }
}

/// Generate a Mermaid `stateDiagram-v2` diagram from a protocol config.
///
/// State IDs are sanitized (hyphens → underscores). States whose names contain
/// hyphens get a `state "original-name" as sanitized_name` label line.
// [generate-mermaid]
pub fn generate_mermaid(config: &ProtocolConfig) -> String {
    let mut out = String::new();
    out.push_str("stateDiagram-v2\n");

    // Emit label aliases for states with hyphens.
    let mut state_names: Vec<&str> = config.states.keys().map(|s| s.as_str()).collect();
    state_names.sort();
    for name in &state_names {
        if name.contains('-') {
            out.push_str(&format!(
                "    state \"{}\" as {}\n",
                name,
                sanitize_id(name)
            ));
        }
    }

    // Initial state arrow.
    if let Some(initial) = config.initial_state() {
        out.push_str(&format!("    [*] --> {}\n", sanitize_id(initial)));
    }

    // Transitions.
    for t in &config.transitions {
        let from_id = sanitize_id(&t.from);
        let to_id = sanitize_id(&t.to);
        if t.gates.is_empty() {
            out.push_str(&format!("    {} --> {} : {}\n", from_id, to_id, t.command));
        } else {
            let gate_labels: Vec<String> = t.gates.iter().map(gate_short_label).collect();
            let gate_summary = gate_labels.join(", ");
            out.push_str(&format!(
                "    {} --> {} : {} [{}]\n",
                from_id, to_id, t.command, gate_summary
            ));
        }
    }

    // Terminal state arrows.
    for name in &state_names {
        if let Some(state) = config.states.get(*name) {
            if state.terminal.unwrap_or(false) {
                out.push_str(&format!("    {} --> [*]\n", sanitize_id(name)));
            }
        }
    }

    out
}

// --- ASCII tree-walk ---

/// Produce a gate summary line for ASCII output (same logic as Mermaid).
fn gate_ascii_label(gate: &GateConfig) -> String {
    gate_short_label(gate)
}

/// Shared read-only context for the recursive walk.
struct WalkCtx<'a> {
    config: &'a ProtocolConfig,
    /// Map from state name to outgoing transition indices into `config.transitions`.
    transitions_by_from: &'a HashMap<&'a str, Vec<usize>>,
}

/// Per-call parameters for a single `walk` invocation.
struct WalkCall<'a> {
    state_name: &'a str,
    /// Visual indent prefix for this level.
    prefix: &'a str,
    /// Whether this is the last child of its parent (controls ├─ vs └─).
    is_last: bool,
    /// True for the root call (prints without a leading connector).
    is_root: bool,
}

/// Walk from `state_name` depth-first, appending lines to `out`.
///
/// `visited` tracks states already expanded to detect cycles.
fn walk(ctx: &WalkCtx<'_>, call: WalkCall<'_>, visited: &mut HashSet<String>, out: &mut String) {
    let state = match ctx.config.states.get(call.state_name) {
        Some(s) => s,
        None => return,
    };

    // Build state annotation.
    let mut annotations = Vec::new();
    if state.initial.unwrap_or(false) {
        annotations.push("initial");
    }
    if state.terminal.unwrap_or(false) {
        annotations.push("terminal");
    }
    let annotation = if annotations.is_empty() {
        String::new()
    } else {
        format!(" ({})", annotations.join(", "))
    };

    // Emit this node.
    if call.is_root {
        out.push_str(&format!("[{}]{}\n", call.state_name, annotation));
    } else {
        let connector = if call.is_last { "└─" } else { "├─" };
        out.push_str(&format!(
            "{}{} [{}]{}\n",
            call.prefix, connector, call.state_name, annotation
        ));
    }

    // If already visited, it's a back-edge — caller already printed `(↑ cycle)` inline.
    if visited.contains(call.state_name) {
        return;
    }
    visited.insert(call.state_name.to_string());

    let outgoing = match ctx.transitions_by_from.get(call.state_name) {
        Some(v) => v,
        None => return,
    };

    // Determine the child prefix for sub-items.
    let child_prefix = if call.is_root {
        " ".to_string()
    } else if call.is_last {
        format!("{}  ", call.prefix)
    } else {
        format!("{}│ ", call.prefix)
    };

    // Track (from, command) pairs to detect fallback candidates.
    let mut seen_from_command: HashSet<String> = HashSet::new();

    for (i, &tidx) in outgoing.iter().enumerate() {
        let t = &ctx.config.transitions[tidx];
        let is_last_child = i == outgoing.len() - 1;
        let child_connector = if is_last_child { "└─" } else { "├─" };

        // Determine if this is a fallback candidate.
        let fc_key = format!("{}:{}", t.from, t.command);
        let is_fallback_candidate = seen_from_command.contains(&fc_key);
        seen_from_command.insert(fc_key);

        let fallback_note = if is_fallback_candidate {
            " (fallback)"
        } else {
            ""
        };

        // Transition label line.
        out.push_str(&format!(
            "{}{} {} ──▶",
            child_prefix, child_connector, t.command
        ));

        // Target state — check cycle.
        let is_cycle = visited.contains(t.to.as_str());
        if is_cycle {
            out.push_str(&format!(" [{}] (↑ cycle){}\n", t.to, fallback_note));
        } else {
            out.push_str(&format!(" [{}]{}\n", t.to, fallback_note));
        }

        // Gate summary sub-line (under the target, indented with │).
        if !t.gates.is_empty() {
            let gate_labels: Vec<String> = t.gates.iter().map(gate_ascii_label).collect();
            let gate_summary = gate_labels.join(", ");
            let gate_prefix = if is_last_child {
                format!("{}   ", child_prefix)
            } else {
                format!("{}│  ", child_prefix)
            };
            out.push_str(&format!("{}│ {}\n", gate_prefix, gate_summary));
        }

        // Recurse into target unless it's a cycle.
        if !is_cycle {
            let target_prefix = if is_last_child {
                format!("{}   ", child_prefix)
            } else {
                format!("{}│  ", child_prefix)
            };
            walk(
                ctx,
                WalkCall {
                    state_name: &t.to,
                    prefix: &target_prefix,
                    is_last: false,
                    is_root: false,
                },
                visited,
                out,
            );
        }
    }
}

/// Generate an ASCII tree-walk diagram from a protocol config.
///
/// DFS from the initial state; cycles are marked `(↑ cycle)` rather than
/// recursed into. Fallback candidates (duplicate from+command) are noted.
// [generate-ascii]
pub fn generate_ascii(config: &ProtocolConfig) -> String {
    let initial = match config.initial_state() {
        Some(s) => s,
        None => return "(no initial state)\n".to_string(),
    };

    // Build adjacency: state name → list of transition indices.
    let mut transitions_by_from: HashMap<&str, Vec<usize>> = HashMap::new();
    for (i, t) in config.transitions.iter().enumerate() {
        transitions_by_from
            .entry(t.from.as_str())
            .or_default()
            .push(i);
    }

    let ctx = WalkCtx {
        config,
        transitions_by_from: &transitions_by_from,
    };

    let mut out = String::new();
    let mut visited: HashSet<String> = HashSet::new();

    walk(
        &ctx,
        WalkCall {
            state_name: initial,
            prefix: "",
            is_last: true,
            is_root: true,
        },
        &mut visited,
        &mut out,
    );

    out
}
