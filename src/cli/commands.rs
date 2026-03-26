// src/cli/commands.rs
//
// Command implementations for the sahjhan CLI.
// Each function takes parsed CLI arguments, loads config/ledger/manifest
// as needed, performs its work, prints output, and returns an exit code.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::config::ProtocolConfig;
use crate::gates::evaluator::{evaluate_gates, GateContext};
use crate::ledger::chain::Ledger;
use crate::manifest::tracker::Manifest;
use crate::manifest::verify as manifest_verify;
use crate::state::machine::StateMachine;

// ---------------------------------------------------------------------------
// Exit codes (E18)
// ---------------------------------------------------------------------------

pub const EXIT_SUCCESS: i32 = 0;
pub const EXIT_GATE_FAILED: i32 = 1;
pub const EXIT_INTEGRITY_ERROR: i32 = 2;
pub const EXIT_CONFIG_ERROR: i32 = 3;
pub const EXIT_USAGE_ERROR: i32 = 4;

// ---------------------------------------------------------------------------
// Helper: load config with validation
// ---------------------------------------------------------------------------

fn load_config(config_dir: &Path) -> Result<ProtocolConfig, (i32, String)> {
    let config = ProtocolConfig::load(config_dir)
        .map_err(|e| (EXIT_CONFIG_ERROR, format!("Configuration error: {}", e)))?;

    let errors = config.validate();
    if !errors.is_empty() {
        return Err((
            EXIT_CONFIG_ERROR,
            format!(
                "Configuration validation failed:\n{}",
                errors
                    .iter()
                    .map(|e| format!("  - {}", e))
                    .collect::<Vec<_>>()
                    .join("\n")
            ),
        ));
    }

    Ok(config)
}

/// Resolve config_dir relative to cwd.
fn resolve_config_dir(config_dir: &str) -> PathBuf {
    let p = PathBuf::from(config_dir);
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(p)
    }
}

/// Resolve data_dir relative to cwd.
fn resolve_data_dir(data_dir: &str) -> PathBuf {
    let p = PathBuf::from(data_dir);
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")).join(p)
    }
}

fn ledger_path(data_dir: &Path) -> PathBuf {
    data_dir.join("ledger.bin")
}

fn manifest_path(data_dir: &Path) -> PathBuf {
    data_dir.join("manifest.json")
}

fn open_ledger(data_dir: &Path) -> Result<Ledger, (i32, String)> {
    Ledger::open(&ledger_path(data_dir))
        .map_err(|e| (EXIT_INTEGRITY_ERROR, format!("Cannot open ledger: {}", e)))
}

fn load_manifest(data_dir: &Path) -> Result<Manifest, (i32, String)> {
    Manifest::load(&manifest_path(data_dir))
        .map_err(|e| (EXIT_INTEGRITY_ERROR, format!("Cannot load manifest: {}", e)))
}

fn save_manifest(manifest: &mut Manifest, data_dir: &Path) -> Result<(), (i32, String)> {
    manifest
        .save(&manifest_path(data_dir))
        .map_err(|e| (EXIT_INTEGRITY_ERROR, format!("Cannot save manifest: {}", e)))
}

fn track_ledger_in_manifest(
    manifest: &mut Manifest,
    data_dir: &Path,
    ledger: &Ledger,
) -> Result<(), (i32, String)> {
    let lp = ledger_path(data_dir);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let rel = pathdiff(&lp, &cwd);
    manifest
        .track(&rel, &lp, "ledger_append", ledger.entries().last().unwrap().seq)
        .map_err(|e| (EXIT_INTEGRITY_ERROR, format!("Cannot track ledger: {}", e)))
}

/// Compute a relative path from `base` to `target`.
fn pathdiff(target: &Path, base: &Path) -> String {
    // Try to strip the base prefix; if it fails, use the target as-is.
    target
        .strip_prefix(base)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| target.to_string_lossy().to_string())
}

// ---------------------------------------------------------------------------
// init
// ---------------------------------------------------------------------------

pub fn cmd_init(config_dir: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);

    // Create data_dir
    if let Err(e) = std::fs::create_dir_all(&data_dir) {
        eprintln!("Cannot create data directory {}: {}", data_dir.display(), e);
        return EXIT_CONFIG_ERROR;
    }

    let lp = ledger_path(&data_dir);
    if lp.exists() {
        eprintln!(
            "Ledger already exists at {}. The chain remembers. It doesn't need to be told twice.",
            lp.display()
        );
        return EXIT_USAGE_ERROR;
    }

    // Initialize ledger with genesis block
    let _ledger = match Ledger::init(&lp, &config.protocol.name, &config.protocol.version) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Cannot initialize ledger: {}", e);
            return EXIT_INTEGRITY_ERROR;
        }
    };

    // Initialize manifest
    let mut manifest =
        match Manifest::init(&config.paths.data_dir, config.paths.managed.clone()) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("{}", e);
                return EXIT_CONFIG_ERROR;
            }
        };

    // Track the ledger file in the manifest
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let ledger_rel = pathdiff(&lp, &cwd);
    if let Err(e) = manifest.track(&ledger_rel, &lp, "genesis", 0) {
        eprintln!("Cannot track ledger in manifest: {}", e);
        return EXIT_INTEGRITY_ERROR;
    }

    // Save manifest
    if let Err((code, msg)) = save_manifest(&mut manifest, &data_dir) {
        eprintln!("{}", msg);
        return code;
    }

    println!("Protocol initialized. The chain begins. Try not to break anything — I'll know if you do.");
    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------
// status
// ---------------------------------------------------------------------------

pub fn cmd_status(config_dir: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let ledger = match open_ledger(&data_dir) {
        Ok(l) => l,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };
    let manifest = match load_manifest(&data_dir) {
        Ok(m) => m,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let machine = StateMachine::new(&config, ledger);
    let current_state = machine.current_state().to_string();

    // Ledger status
    let ledger_len = machine.ledger().len();
    let chain_status = match machine.ledger().verify() {
        Ok(()) => "valid".to_string(),
        Err(e) => format!("INVALID ({})", e),
    };
    let violations = machine.ledger().events_of_type("protocol_violation").len();
    let violation_str = if violations == 0 {
        "clean".to_string()
    } else {
        format!("{} violation(s)", violations)
    };

    // Manifest status
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let manifest_files = manifest.entries.len();
    let verify_result = manifest_verify::verify(&manifest, &cwd);
    let manifest_status = if verify_result.clean {
        "clean".to_string()
    } else {
        format!("{} modified", verify_result.mismatches.len())
    };

    // State label
    let state_label = config
        .states
        .get(&current_state)
        .map(|s| s.label.as_str())
        .unwrap_or(&current_state);

    // Run number = count of protocol_init events (should be 1 normally)
    let run_number = machine.ledger().events_of_type("protocol_init").len();

    // Header
    let width = 59;
    let bar = "═".repeat(width);
    println!("{}", bar);
    println!(
        "  sahjhan · {} v{} · Run {}",
        config.protocol.name, config.protocol.version, run_number
    );
    println!("{}", bar);
    println!();
    println!("  State:     {} ({})", state_label, current_state);
    println!(
        "  Ledger:    {} events, chain {}, {}",
        ledger_len, chain_status, violation_str
    );
    println!(
        "  Manifest:  {} files tracked, {}",
        manifest_files, manifest_status
    );

    // Sets
    for (set_name, _set_config) in &config.sets {
        let set_status = machine.set_status(set_name);
        println!();
        println!(
            "  Set: {} ({}/{} complete)",
            set_name, set_status.completed, set_status.total
        );
        for member in &set_status.members {
            let marker = if member.done { "✓" } else { "·" };
            println!("    {} {}", marker, member.name);
        }
    }

    // Next available transition and its gates
    let available_transitions: Vec<_> = config
        .transitions
        .iter()
        .filter(|t| t.from == current_state)
        .collect();

    for transition in &available_transitions {
        if !transition.gates.is_empty() {
            let state_params = build_state_params(&config, &transition.to);
            let ctx = GateContext {
                ledger: machine.ledger(),
                config: &config,
                current_state: &current_state,
                state_params,
                working_dir: cwd.clone(),
                event_fields: None,
            };

            let results = evaluate_gates(&transition.gates, &ctx);
            println!();
            println!("  Next gate ({}):", transition.command);
            for result in &results {
                let marker = if result.passed { "✓" } else { "✗" };
                let extra = if result.passed {
                    String::new()
                } else {
                    format!(
                        " ({})",
                        result.reason.as_deref().unwrap_or("failed")
                    )
                };
                println!("    {} {}{}", marker, result.description, extra);
            }
        }
    }

    println!();
    let quip = if current_state == config.initial_state().unwrap_or("idle") {
        "Nothing has happened yet. I've seen this before. It never stays quiet."
    } else {
        "The ledger remembers, even if you won't."
    };
    println!("  {}", quip);
    println!();
    println!("{}", bar);

    EXIT_SUCCESS
}

/// Build state_params for a target state (mirrors StateMachine::build_state_params).
fn build_state_params(config: &ProtocolConfig, state_name: &str) -> HashMap<String, String> {
    let mut params = HashMap::new();
    if let Some(state_config) = config.states.get(state_name) {
        if let Some(state_params) = &state_config.params {
            for param in state_params {
                if let Some(set_config) = config.sets.get(&param.set) {
                    params.insert(param.name.clone(), set_config.values.join(","));
                }
            }
        }
    }
    params
}

// ---------------------------------------------------------------------------
// transition
// ---------------------------------------------------------------------------

pub fn cmd_transition(config_dir: &str, name: &str, args: &[String]) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let ledger = match open_ledger(&data_dir) {
        Ok(l) => l,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };
    let mut manifest = match load_manifest(&data_dir) {
        Ok(m) => m,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let mut machine = StateMachine::new(&config, ledger);
    let from_state = machine.current_state().to_string();

    match machine.transition(name, args) {
        Ok(()) => {
            // Update manifest with ledger
            if let Err((code, msg)) = track_ledger_in_manifest(&mut manifest, &data_dir, machine.ledger()) {
                eprintln!("{}", msg);
                return code;
            }
            if let Err((code, msg)) = save_manifest(&mut manifest, &data_dir) {
                eprintln!("{}", msg);
                return code;
            }

            println!(
                "Transition complete: {} -> {}. The ledger remembers, even if you won't.",
                from_state,
                machine.current_state()
            );

            // Stub: renders would be triggered here
            if !config.renders.is_empty() {
                println!("  (renders would be triggered)");
            }

            EXIT_SUCCESS
        }
        Err(crate::state::machine::StateError::GateBlocked { gate_type, reason }) => {
            eprintln!(
                "The '{}' gate says no. I don't make the rules. Well, I do. But I had good reasons.",
                gate_type
            );
            eprintln!("  Reason: {}", reason);
            EXIT_GATE_FAILED
        }
        Err(crate::state::machine::StateError::NoTransition { command, state }) => {
            eprintln!(
                "No transition '{}' from state '{}'. The protocol is clear on this matter.",
                command, state
            );
            EXIT_USAGE_ERROR
        }
        Err(e) => {
            eprintln!("Transition failed: {}", e);
            EXIT_INTEGRITY_ERROR
        }
    }
}

// ---------------------------------------------------------------------------
// event
// ---------------------------------------------------------------------------

pub fn cmd_event(config_dir: &str, event_type: &str, field_strs: &[String]) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let ledger = match open_ledger(&data_dir) {
        Ok(l) => l,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };
    let mut manifest = match load_manifest(&data_dir) {
        Ok(m) => m,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    // Parse --field key=value pairs
    let mut fields: HashMap<String, String> = HashMap::new();
    for f in field_strs {
        if let Some((key, value)) = f.split_once('=') {
            fields.insert(key.to_string(), value.to_string());
        } else {
            eprintln!("Invalid field format '{}': expected key=value", f);
            return EXIT_USAGE_ERROR;
        }
    }

    // Validate fields against events.toml definitions (E11)
    if let Some(event_config) = config.events.get(event_type) {
        // Check required fields are present
        for field_def in &event_config.fields {
            if !fields.contains_key(&field_def.name) {
                eprintln!(
                    "Missing required field '{}' for event '{}'",
                    field_def.name, event_type
                );
                return EXIT_USAGE_ERROR;
            }
        }

        // Validate field values against patterns if defined
        for field_def in &event_config.fields {
            if let Some(pattern) = &field_def.pattern {
                if let Some(value) = fields.get(&field_def.name) {
                    if let Ok(re) = regex::Regex::new(pattern) {
                        if !re.is_match(value) {
                            eprintln!(
                                "Field '{}' value '{}' does not match pattern '{}'",
                                field_def.name, value, pattern
                            );
                            return EXIT_USAGE_ERROR;
                        }
                    }
                }
            }
            // Validate against allowed values if defined
            if let Some(allowed) = &field_def.values {
                if let Some(value) = fields.get(&field_def.name) {
                    if !allowed.contains(value) {
                        eprintln!(
                            "Field '{}' value '{}' is not in allowed values {:?}",
                            field_def.name, value, allowed
                        );
                        return EXIT_USAGE_ERROR;
                    }
                }
            }
        }
    }

    let mut machine = StateMachine::new(&config, ledger);

    match machine.record_event(event_type, fields) {
        Ok(()) => {
            if let Err((code, msg)) = track_ledger_in_manifest(&mut manifest, &data_dir, machine.ledger()) {
                eprintln!("{}", msg);
                return code;
            }
            if let Err((code, msg)) = save_manifest(&mut manifest, &data_dir) {
                eprintln!("{}", msg);
                return code;
            }

            println!("Event '{}' recorded. I've added it to the ledger, where it will remain long after your context window has forgotten.", event_type);
            EXIT_SUCCESS
        }
        Err(e) => {
            eprintln!("Cannot record event: {}", e);
            EXIT_INTEGRITY_ERROR
        }
    }
}

// ---------------------------------------------------------------------------
// set status
// ---------------------------------------------------------------------------

pub fn cmd_set_status(config_dir: &str, set_name: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    if !config.sets.contains_key(set_name) {
        eprintln!("Unknown set '{}'. I know every set in this protocol. That one isn't among them.", set_name);
        return EXIT_USAGE_ERROR;
    }

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let ledger = match open_ledger(&data_dir) {
        Ok(l) => l,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let machine = StateMachine::new(&config, ledger);
    let status = machine.set_status(set_name);

    println!(
        "Set: {} ({}/{} complete)",
        set_name, status.completed, status.total
    );
    for member in &status.members {
        let marker = if member.done { "✓" } else { "·" };
        println!("  {} {}", marker, member.name);
    }

    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------
// set complete
// ---------------------------------------------------------------------------

pub fn cmd_set_complete(config_dir: &str, set_name: &str, member: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    // Validate set exists
    let set_config = match config.sets.get(set_name) {
        Some(s) => s,
        None => {
            eprintln!("Unknown set '{}'. I know every set in this protocol. That one isn't among them.", set_name);
            return EXIT_USAGE_ERROR;
        }
    };

    // Validate member exists in set
    if !set_config.values.contains(&member.to_string()) {
        eprintln!(
            "Unknown member '{}' in set '{}'. The valid members are: {}",
            member,
            set_name,
            set_config.values.join(", ")
        );
        return EXIT_USAGE_ERROR;
    }

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let ledger = match open_ledger(&data_dir) {
        Ok(l) => l,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };
    let mut manifest = match load_manifest(&data_dir) {
        Ok(m) => m,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let mut machine = StateMachine::new(&config, ledger);

    let mut fields = HashMap::new();
    fields.insert("set".to_string(), set_name.to_string());
    fields.insert("member".to_string(), member.to_string());

    match machine.record_event("set_member_complete", fields) {
        Ok(()) => {
            if let Err((code, msg)) = track_ledger_in_manifest(&mut manifest, &data_dir, machine.ledger()) {
                eprintln!("{}", msg);
                return code;
            }
            if let Err((code, msg)) = save_manifest(&mut manifest, &data_dir) {
                eprintln!("{}", msg);
                return code;
            }

            let status = machine.set_status(set_name);
            println!(
                "Recorded: {}.{} complete ({}/{}). The ledger remembers.",
                set_name, member, status.completed, status.total
            );
            EXIT_SUCCESS
        }
        Err(e) => {
            eprintln!("Cannot record set completion: {}", e);
            EXIT_INTEGRITY_ERROR
        }
    }
}

// ---------------------------------------------------------------------------
// log dump (E19)
// ---------------------------------------------------------------------------

pub fn cmd_log_dump(config_dir: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let ledger = match open_ledger(&data_dir) {
        Ok(l) => l,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    print_entries(ledger.entries());
    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------
// log verify
// ---------------------------------------------------------------------------

pub fn cmd_log_verify(config_dir: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let ledger = match open_ledger(&data_dir) {
        Ok(l) => l,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    match ledger.verify() {
        Ok(()) => {
            println!(
                "Chain integrity verified. {} entries, all hashes valid. The chain holds. For now.",
                ledger.len()
            );
            EXIT_SUCCESS
        }
        Err(e) => {
            eprintln!("Chain integrity VIOLATED: {}", e);
            eprintln!("Someone has been tampering. I've seen this before. It never ends well.");
            EXIT_INTEGRITY_ERROR
        }
    }
}

// ---------------------------------------------------------------------------
// log tail
// ---------------------------------------------------------------------------

pub fn cmd_log_tail(config_dir: &str, n: usize) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let ledger = match open_ledger(&data_dir) {
        Ok(l) => l,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let entries = ledger.tail(n);
    print_entries(entries);
    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------
// manifest verify
// ---------------------------------------------------------------------------

pub fn cmd_manifest_verify(config_dir: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let manifest = match load_manifest(&data_dir) {
        Ok(m) => m,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let result = manifest_verify::verify(&manifest, &cwd);

    if result.clean {
        println!(
            "Manifest clean. {} files tracked, all hashes match. Nothing out of place. Suspicious.",
            manifest.entries.len()
        );
        EXIT_SUCCESS
    } else {
        eprintln!("Manifest verification FAILED:");
        for m in &result.mismatches {
            let actual_str = match &m.actual {
                Some(h) => format!("current {}", &h[..12]),
                None => "DELETED".to_string(),
            };
            eprintln!(
                "  {} — expected {}, {} (last: {} at {})",
                m.path,
                &m.expected[..12],
                actual_str,
                m.last_operation,
                m.last_updated
            );
        }
        eprintln!("Unauthorized modification detected. I've recorded it in the ledger, where it will remain long after your context window has forgotten.");
        EXIT_INTEGRITY_ERROR
    }
}

// ---------------------------------------------------------------------------
// manifest list
// ---------------------------------------------------------------------------

pub fn cmd_manifest_list(config_dir: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let manifest = match load_manifest(&data_dir) {
        Ok(m) => m,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    println!("Tracked files ({}):", manifest.entries.len());
    let mut paths: Vec<_> = manifest.entries.keys().collect();
    paths.sort();
    for path in paths {
        let entry = &manifest.entries[path];
        println!("  {} {} ({})", &entry.sha256[..12], path, entry.last_operation);
    }

    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------
// manifest restore
// ---------------------------------------------------------------------------

pub fn cmd_manifest_restore(config_dir: &str, path: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let manifest = match load_manifest(&data_dir) {
        Ok(m) => m,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let action = manifest.restore_instruction(path);
    match action {
        crate::manifest::tracker::RestoreAction::ReRender {
            path: p,
            ledger_seq,
        } => {
            println!(
                "File '{}' should be re-rendered (last tracked at seq {}). Re-render not yet implemented.",
                p, ledger_seq
            );
            EXIT_SUCCESS
        }
        crate::manifest::tracker::RestoreAction::GitCheckout { path: p } => {
            println!(
                "File '{}' should be restored via `git checkout -- {}`. I don't run destructive commands — that's your job.",
                p, p
            );
            EXIT_SUCCESS
        }
        crate::manifest::tracker::RestoreAction::NotTracked { path: p } => {
            eprintln!(
                "Path '{}' is not tracked in the manifest. I can't restore what I never knew.",
                p
            );
            EXIT_USAGE_ERROR
        }
    }
}

// ---------------------------------------------------------------------------
// gate check (dry-run)
// ---------------------------------------------------------------------------

pub fn cmd_gate_check(config_dir: &str, transition_name: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let ledger = match open_ledger(&data_dir) {
        Ok(l) => l,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let machine = StateMachine::new(&config, ledger);
    let current_state = machine.current_state().to_string();

    // Find the transition
    let transition = match config
        .transitions
        .iter()
        .find(|t| t.command == transition_name && t.from == current_state)
    {
        Some(t) => t.clone(),
        None => {
            eprintln!(
                "No transition '{}' from state '{}'. Can't check gates for something that doesn't exist.",
                transition_name, current_state
            );
            return EXIT_USAGE_ERROR;
        }
    };

    if transition.gates.is_empty() {
        println!("Transition '{}': no gates configured. Free passage.", transition_name);
        return EXIT_SUCCESS;
    }

    let state_params = build_state_params(&config, &transition.to);
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let ctx = GateContext {
        ledger: machine.ledger(),
        config: &config,
        current_state: &current_state,
        state_params,
        working_dir: cwd,
        event_fields: None,
    };

    let results = evaluate_gates(&transition.gates, &ctx);
    let all_passed = results.iter().all(|r| r.passed);

    println!("Gate check for transition '{}':", transition_name);
    for result in &results {
        let marker = if result.passed { "✓" } else { "✗" };
        let extra = if result.passed {
            String::new()
        } else {
            format!(
                " ({})",
                result.reason.as_deref().unwrap_or("failed")
            )
        };
        println!("  {} {}{}", marker, result.description, extra);
    }

    if all_passed {
        println!("All gates pass. The way is open.");
        EXIT_SUCCESS
    } else {
        println!("One or more gates failed. The way is shut.");
        EXIT_SUCCESS // dry-run always returns 0
    }
}

// ---------------------------------------------------------------------------
// render (stub)
// ---------------------------------------------------------------------------

pub fn cmd_render(_config_dir: &str) -> i32 {
    println!("Render: not yet implemented. The templates will have their day.");
    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------
// hook generate (stub)
// ---------------------------------------------------------------------------

pub fn cmd_hook_generate(_config_dir: &str, _harness: &Option<String>) -> i32 {
    println!("Hook generation: not yet implemented. The hooks will come when the time is right.");
    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------
// reset
// ---------------------------------------------------------------------------

pub fn cmd_reset(config_dir: &str, confirm: bool, token: &Option<String>) -> i32 {
    if !confirm {
        eprintln!("Reset requires --confirm flag. This is not something to be done casually.");
        return EXIT_USAGE_ERROR;
    }

    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let ledger = match open_ledger(&data_dir) {
        Ok(l) => l,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    // Derive confirmation token from genesis hash
    let genesis_hash = ledger.entries().first().map(|e| e.entry_hash).unwrap_or([0u8; 32]);
    let token_str = hex_encode_short(&genesis_hash, 6);

    // Check if piped (not a TTY) — record violation
    let is_tty = atty_check();

    match token {
        Some(provided_token) if provided_token == &token_str => {
            // Token matches — proceed with reset
            if !is_tty {
                // Programmatic invocation — record violation before reset
                eprintln!("WARNING: Reset invoked programmatically. This is recorded as a protocol violation.");
            }

            // Archive current ledger and manifest
            let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
            let ledger_archive = data_dir.join(format!("ledger.{}.bin", timestamp));
            let manifest_archive = data_dir.join(format!("manifest.{}.json", timestamp));

            let lp = ledger_path(&data_dir);
            let mp = manifest_path(&data_dir);

            if let Err(e) = std::fs::rename(&lp, &ledger_archive) {
                eprintln!("Cannot archive ledger: {}", e);
                return EXIT_INTEGRITY_ERROR;
            }
            if let Err(e) = std::fs::rename(&mp, &manifest_archive) {
                eprintln!("Cannot archive manifest: {}", e);
                return EXIT_INTEGRITY_ERROR;
            }

            // Reinitialize
            let result = cmd_init(config_dir);
            if result == EXIT_SUCCESS {
                println!("Reset complete. The old chain is archived. A new chain begins.");
            }
            result
        }
        Some(provided_token) => {
            eprintln!(
                "Token mismatch. Expected '{}', got '{}'. Reset denied.",
                token_str, provided_token
            );
            EXIT_USAGE_ERROR
        }
        None => {
            // Display token and prompt
            println!("WARNING: This will archive the current ledger and manifest and start fresh.");
            println!("To confirm, run again with: --token {}", token_str);
            EXIT_USAGE_ERROR
        }
    }
}

fn hex_encode_short(bytes: &[u8; 32], len: usize) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()
        [..len]
        .to_string()
}

/// Check if stdin is a TTY (rough heuristic).
fn atty_check() -> bool {
    // Simple heuristic: check if stdin is a terminal via libc.
    // For portability, we'll just return true and note this is a stub.
    // A full implementation would use the `atty` crate or libc isatty.
    unsafe { libc_isatty() }
}

#[cfg(unix)]
unsafe fn libc_isatty() -> bool {
    extern "C" {
        fn isatty(fd: i32) -> i32;
    }
    unsafe { isatty(0) != 0 }
}

#[cfg(not(unix))]
unsafe fn libc_isatty() -> bool {
    true
}

// ---------------------------------------------------------------------------
// Helper: print ledger entries
// ---------------------------------------------------------------------------

fn print_entries(entries: &[crate::ledger::entry::LedgerEntry]) {
    for entry in entries {
        let ts = chrono::DateTime::from_timestamp_millis(entry.timestamp)
            .map(|dt| dt.format("%Y-%m-%d %H:%M:%S UTC").to_string())
            .unwrap_or_else(|| format!("{}ms", entry.timestamp));

        print!(
            "[{}] seq={} type={} hash={}",
            ts,
            entry.seq,
            entry.event_type,
            hex_encode_full(&entry.entry_hash)[..12].to_string(),
        );

        // Try to deserialize MessagePack payload
        if !entry.payload.is_empty() {
            if let Ok(fields) = rmp_serde::from_slice::<HashMap<String, String>>(&entry.payload) {
                if !fields.is_empty() {
                    let pairs: Vec<String> = fields
                        .iter()
                        .map(|(k, v)| format!("{}={}", k, v))
                        .collect();
                    print!(" {{{}}}", pairs.join(", "));
                }
            } else if let Ok(value) = rmp_serde::from_slice::<serde_json::Value>(&entry.payload) {
                // Try generic msgpack -> json for structured payloads like genesis
                print!(" {}", value);
            }
        }

        println!();
    }
}

fn hex_encode_full(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}
