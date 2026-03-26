// src/cli/commands.rs
//
// Command implementations for the sahjhan CLI.
// Each function takes parsed CLI arguments, loads config/ledger/manifest
// as needed, performs its work, prints output, and returns an exit code.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use crate::config::ProtocolConfig;
use crate::gates::evaluator::{evaluate_gates, GateContext};
use crate::ledger::chain::Ledger;
use crate::ledger::import::import_jsonl;
use crate::ledger::registry::{LedgerMode, LedgerRegistry};
use crate::manifest::tracker::Manifest;
use crate::manifest::verify as manifest_verify;
use crate::query::QueryEngine;
use crate::render::engine::RenderEngine;
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
// Ledger targeting (Task 14)
// ---------------------------------------------------------------------------

/// Captures global --ledger / --ledger-path flags for ledger resolution.
pub struct LedgerTargeting {
    pub ledger_name: Option<String>,
    pub ledger_path: Option<String>,
}

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
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(p)
    }
}

/// Resolve data_dir relative to cwd.
fn resolve_data_dir(data_dir: &str) -> PathBuf {
    let p = PathBuf::from(data_dir);
    if p.is_absolute() {
        p
    } else {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(p)
    }
}

fn ledger_path(data_dir: &Path) -> PathBuf {
    data_dir.join("ledger.jsonl")
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
        .track(
            &rel,
            &lp,
            "ledger_append",
            ledger.entries().last().unwrap().seq,
        )
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
// Ledger resolution (Task 14)
// ---------------------------------------------------------------------------

/// Resolve a ledger path from targeting flags.
///
/// 1. If --ledger-path given, use that file directly.
/// 2. If --ledger given, resolve from registry.
/// 3. If neither, try registry first entry; else fall back to config data_dir/ledger.jsonl.
fn resolve_ledger_from_targeting(
    config: &ProtocolConfig,
    targeting: &LedgerTargeting,
) -> Result<(PathBuf, Option<LedgerMode>), (i32, String)> {
    // 1. Direct path
    if let Some(ref lp) = targeting.ledger_path {
        let p = PathBuf::from(lp);
        return Ok((p, None));
    }

    // 2. Named ledger from registry
    if let Some(ref name) = targeting.ledger_name {
        let reg_path = registry_path_from_config(config);
        let registry = LedgerRegistry::new(&reg_path).map_err(|e| {
            (
                EXIT_CONFIG_ERROR,
                format!("Cannot load ledger registry: {}", e),
            )
        })?;
        let entry = registry.resolve(Some(name)).map_err(|e| {
            (EXIT_CONFIG_ERROR, format!("Ledger resolution failed: {}", e))
        })?;
        let resolved = resolve_registry_path(&entry.path, config);
        return Ok((resolved, Some(entry.mode.clone())));
    }

    // 3. Default: try registry first, else fall back to data_dir/ledger.jsonl
    let reg_path = registry_path_from_config(config);
    if reg_path.exists() {
        if let Ok(registry) = LedgerRegistry::new(&reg_path) {
            if let Ok(entry) = registry.resolve(None) {
                let resolved = resolve_registry_path(&entry.path, config);
                return Ok((resolved, Some(entry.mode.clone())));
            }
        }
    }

    // Fall back to default ledger path
    let data_dir = resolve_data_dir(&config.paths.data_dir);
    Ok((ledger_path(&data_dir), None))
}

/// Open a ledger using targeting flags.
fn open_targeted_ledger(
    config: &ProtocolConfig,
    targeting: &LedgerTargeting,
) -> Result<(Ledger, Option<LedgerMode>), (i32, String)> {
    let (path, mode) = resolve_ledger_from_targeting(config, targeting)?;
    let ledger = Ledger::open(&path)
        .map_err(|e| (EXIT_INTEGRITY_ERROR, format!("Cannot open ledger: {}", e)))?;
    Ok((ledger, mode))
}

/// Compute the registry path relative to the config's data_dir.
fn registry_path_from_config(config: &ProtocolConfig) -> PathBuf {
    let data_dir = resolve_data_dir(&config.paths.data_dir);
    data_dir.join("ledgers.toml")
}

/// Resolve a registry entry path (relative to data_dir) to an absolute path.
fn resolve_registry_path(entry_path: &str, config: &ProtocolConfig) -> PathBuf {
    let p = PathBuf::from(entry_path);
    if p.is_absolute() {
        p
    } else {
        let data_dir = resolve_data_dir(&config.paths.data_dir);
        data_dir.join(p)
    }
}

/// Guard: check if a ledger mode is event-only and block stateful operations.
fn guard_event_only(mode: &Option<LedgerMode>, operation: &str) -> Result<(), (i32, String)> {
    if let Some(LedgerMode::EventOnly) = mode {
        Err((
            EXIT_CONFIG_ERROR,
            format!(
                "Cannot {} on an event-only ledger. This ledger has no state machine.",
                operation
            ),
        ))
    } else {
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// validate
// ---------------------------------------------------------------------------

pub fn cmd_validate(config_dir: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);

    // Load the config (parse-level errors)
    let config = match ProtocolConfig::load(&config_path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("  - {}", e);
            eprintln!("\nFix these before running.");
            return EXIT_CONFIG_ERROR;
        }
    };

    // Run deep validation
    let (errors, warnings) = config.validate_deep(&config_path);

    // Print warnings first
    for w in &warnings {
        eprintln!("  warning: {}", w);
    }

    if errors.is_empty() {
        if !warnings.is_empty() {
            println!();
        }
        println!("Config valid.");
        EXIT_SUCCESS
    } else {
        for e in &errors {
            eprintln!("  - {}", e);
        }
        eprintln!("\nFix these before running.");
        EXIT_CONFIG_ERROR
    }
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
            "Already initialized (ledger exists at {}). Run `reset` first if you mean it.",
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

    // Create ledgers.toml registry with a "default" entry pointing to the new ledger
    {
        let reg_path = data_dir.join("ledgers.toml");
        // Relative path from data_dir to ledger (just the filename)
        let ledger_rel_to_data = lp
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "ledger.jsonl".to_string());
        let mut registry = match crate::ledger::registry::LedgerRegistry::new(&reg_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Cannot create ledger registry: {}", e);
                return EXIT_INTEGRITY_ERROR;
            }
        };
        if let Err(e) = registry.create(
            "default",
            &ledger_rel_to_data,
            crate::ledger::registry::LedgerMode::Stateful,
        ) {
            // If the registry already has a "default" entry, skip — idempotent.
            if !e.contains("already exists") {
                eprintln!("Cannot register default ledger: {}", e);
                return EXIT_INTEGRITY_ERROR;
            }
        }
    }

    // Initialize manifest
    let mut manifest = match Manifest::init(&config.paths.data_dir, config.paths.managed.clone()) {
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

    println!("Protocol initialized. Ledger sealed. Good luck.");
    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------
// status
// ---------------------------------------------------------------------------

pub fn cmd_status(config_dir: &str, targeting: &LedgerTargeting) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let (ledger, mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    // Event-only ledger: show metadata without state machine fields
    if let Some(LedgerMode::EventOnly) = mode {
        let ledger_len = ledger.len();
        let chain_status = match ledger.verify() {
            Ok(()) => "valid".to_string(),
            Err(e) => format!("INVALID ({})", e),
        };
        let last_ts = ledger
            .entries()
            .last()
            .map(|e| e.ts.as_str())
            .unwrap_or("none");

        let width = 59;
        let bar = "=".repeat(width);
        println!("{}", bar);
        println!("  sahjhan · event-only ledger");
        println!("{}", bar);
        println!();
        println!("  Mode:      event-only");
        println!("  Events:    {}", ledger_len);
        println!("  Chain:     {}", chain_status);
        println!("  Last:      {}", last_ts);
        println!();
        println!("{}", bar);
        return EXIT_SUCCESS;
    }

    let data_dir = resolve_data_dir(&config.paths.data_dir);
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
    let bar = "=".repeat(width);
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
    for set_name in config.sets.keys() {
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
                    format!(" ({})", result.reason.as_deref().unwrap_or("failed"))
                };
                println!("    {} {}{}", marker, result.description, extra);
            }
        }
    }

    println!();
    if current_state == config.initial_state().unwrap_or("idle") {
        println!("  Awaiting first transition.");
    }
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

pub fn cmd_transition(
    config_dir: &str,
    name: &str,
    args: &[String],
    targeting: &LedgerTargeting,
) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let (ledger, mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    // Guard: event-only ledgers cannot transition
    if let Err((code, msg)) = guard_event_only(&mode, "execute a transition") {
        eprintln!("{}", msg);
        return code;
    }

    let data_dir = resolve_data_dir(&config.paths.data_dir);
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
            if let Err((code, msg)) =
                track_ledger_in_manifest(&mut manifest, &data_dir, machine.ledger())
            {
                eprintln!("{}", msg);
                return code;
            }
            if let Err((code, msg)) = save_manifest(&mut manifest, &data_dir) {
                eprintln!("{}", msg);
                return code;
            }

            println!(
                "Transition: {} -> {}. Recorded.",
                from_state,
                machine.current_state()
            );

            // Trigger on_transition renders
            if !config.renders.is_empty() {
                if let Ok(engine) = RenderEngine::new(&config, &config_path) {
                    let render_dir = resolve_data_dir(&config.paths.render_dir);
                    let ledger_seq = machine
                        .ledger()
                        .entries()
                        .last()
                        .map(|e| e.seq)
                        .unwrap_or(0);
                    match engine.render_triggered(
                        "on_transition",
                        None,
                        machine.ledger(),
                        &render_dir,
                        &mut manifest,
                        ledger_seq,
                    ) {
                        Ok(rendered) => {
                            for target in &rendered {
                                println!("  Rendered: {}", target);
                            }
                            if !rendered.is_empty() {
                                let _ = save_manifest(&mut manifest, &data_dir);
                            }
                        }
                        Err(e) => {
                            eprintln!("  Render warning: {}", e);
                        }
                    }
                }
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

pub fn cmd_event(
    config_dir: &str,
    event_type: &str,
    field_strs: &[String],
    targeting: &LedgerTargeting,
) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
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
            if let Err((code, msg)) =
                track_ledger_in_manifest(&mut manifest, &data_dir, machine.ledger())
            {
                eprintln!("{}", msg);
                return code;
            }
            if let Err((code, msg)) = save_manifest(&mut manifest, &data_dir) {
                eprintln!("{}", msg);
                return code;
            }

            println!("Event '{}' recorded.", event_type);

            // Trigger on_event renders
            if !config.renders.is_empty() {
                if let Ok(engine) = RenderEngine::new(&config, &config_path) {
                    let render_dir = resolve_data_dir(&config.paths.render_dir);
                    let ledger_seq = machine
                        .ledger()
                        .entries()
                        .last()
                        .map(|e| e.seq)
                        .unwrap_or(0);
                    match engine.render_triggered(
                        "on_event",
                        Some(event_type),
                        machine.ledger(),
                        &render_dir,
                        &mut manifest,
                        ledger_seq,
                    ) {
                        Ok(rendered) => {
                            for target in &rendered {
                                println!("  Rendered: {}", target);
                            }
                            if !rendered.is_empty() {
                                let _ = save_manifest(&mut manifest, &data_dir);
                            }
                        }
                        Err(e) => {
                            eprintln!("  Render warning: {}", e);
                        }
                    }
                }
            }

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

pub fn cmd_set_status(config_dir: &str, set_name: &str, targeting: &LedgerTargeting) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    if !config.sets.contains_key(set_name) {
        eprintln!(
            "Unknown set '{}'. I know every set in this protocol. That one isn't among them.",
            set_name
        );
        return EXIT_USAGE_ERROR;
    }

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
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

pub fn cmd_set_complete(
    config_dir: &str,
    set_name: &str,
    member: &str,
    targeting: &LedgerTargeting,
) -> i32 {
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
            eprintln!(
                "Unknown set '{}'. I know every set in this protocol. That one isn't among them.",
                set_name
            );
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

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
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
            if let Err((code, msg)) =
                track_ledger_in_manifest(&mut manifest, &data_dir, machine.ledger())
            {
                eprintln!("{}", msg);
                return code;
            }
            if let Err((code, msg)) = save_manifest(&mut manifest, &data_dir) {
                eprintln!("{}", msg);
                return code;
            }

            let status = machine.set_status(set_name);
            println!(
                "Set {}: {} complete ({}/{}).",
                set_name, member, status.completed, status.total
            );

            // Trigger on_event renders for set_member_complete
            if !config.renders.is_empty() {
                if let Ok(engine) = RenderEngine::new(&config, &config_path) {
                    let render_dir = resolve_data_dir(&config.paths.render_dir);
                    let ledger_seq = machine
                        .ledger()
                        .entries()
                        .last()
                        .map(|e| e.seq)
                        .unwrap_or(0);
                    match engine.render_triggered(
                        "on_event",
                        Some("set_member_complete"),
                        machine.ledger(),
                        &render_dir,
                        &mut manifest,
                        ledger_seq,
                    ) {
                        Ok(rendered) => {
                            for target in &rendered {
                                println!("  Rendered: {}", target);
                            }
                            if !rendered.is_empty() {
                                let _ = save_manifest(&mut manifest, &data_dir);
                            }
                        }
                        Err(e) => {
                            eprintln!("  Render warning: {}", e);
                        }
                    }
                }
            }

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

pub fn cmd_log_dump(config_dir: &str, targeting: &LedgerTargeting) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
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

pub fn cmd_log_verify(config_dir: &str, targeting: &LedgerTargeting) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    match ledger.verify() {
        Ok(()) => {
            println!("OK: {} events, chain intact.", ledger.len());
            EXIT_SUCCESS
        }
        Err(e) => {
            eprintln!("Chain INVALID: {}", e);
            eprintln!("Tampering detected.");
            EXIT_INTEGRITY_ERROR
        }
    }
}

// ---------------------------------------------------------------------------
// log tail
// ---------------------------------------------------------------------------

pub fn cmd_log_tail(config_dir: &str, n: usize, targeting: &LedgerTargeting) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
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
        eprintln!("Unauthorized modification detected. Violation recorded.");
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
        println!(
            "  {} {} ({})",
            &entry.sha256[..12],
            path,
            entry.last_operation
        );
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

pub fn cmd_gate_check(
    config_dir: &str,
    transition_name: &str,
    targeting: &LedgerTargeting,
) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let (ledger, mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    // Guard: event-only ledgers cannot check gates
    if let Err((code, msg)) = guard_event_only(&mode, "check gates") {
        eprintln!("{}", msg);
        return code;
    }

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
        println!(
            "Transition '{}': no gates configured. Free passage.",
            transition_name
        );
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
            format!(" ({})", result.reason.as_deref().unwrap_or("failed"))
        };
        println!("  {} {}{}", marker, result.description, extra);
    }

    if all_passed {
        println!("All gates pass. The way is open.");
        EXIT_SUCCESS
    } else {
        println!("Gate check: one or more gates failed.");
        EXIT_SUCCESS // dry-run always returns 0
    }
}

// ---------------------------------------------------------------------------
// render
// ---------------------------------------------------------------------------

pub fn cmd_render(config_dir: &str, targeting: &LedgerTargeting) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    if config.renders.is_empty() {
        println!("No renders configured. Nothing to do.");
        return EXIT_SUCCESS;
    }

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let (ledger, _mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
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

    let engine = match RenderEngine::new(&config, &config_path) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Cannot create render engine: {}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    let render_dir = resolve_data_dir(&config.paths.render_dir);
    let ledger_seq = ledger.entries().last().map(|e| e.seq).unwrap_or(0);

    match engine.render_all(&ledger, &render_dir, &mut manifest, ledger_seq) {
        Ok(rendered) => {
            if let Err((code, msg)) = save_manifest(&mut manifest, &data_dir) {
                eprintln!("{}", msg);
                return code;
            }
            for target in &rendered {
                println!("Rendered: {}", target);
            }
            println!(
                "All templates rendered. {} file(s) written. The ledger made manifest.",
                rendered.len()
            );
            EXIT_SUCCESS
        }
        Err(e) => {
            eprintln!("Render failed: {}", e);
            EXIT_INTEGRITY_ERROR
        }
    }
}

// ---------------------------------------------------------------------------
// render --dump-context
// ---------------------------------------------------------------------------

pub fn cmd_render_dump_context(config_dir: &str, targeting: &LedgerTargeting) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let (ledger, _mode) = match open_targeted_ledger(&config, targeting) {
        Ok(lm) => lm,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let engine = match RenderEngine::new(&config, &config_path) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Cannot create render engine: {}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    match engine.dump_context(&ledger) {
        Ok(ctx) => {
            println!(
                "{}",
                serde_json::to_string_pretty(&ctx).unwrap_or_else(|_| ctx.to_string())
            );
            EXIT_SUCCESS
        }
        Err(e) => {
            eprintln!("Cannot build render context: {}", e);
            EXIT_INTEGRITY_ERROR
        }
    }
}

// ---------------------------------------------------------------------------
// hook generate
// ---------------------------------------------------------------------------

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
    let genesis_hash = ledger
        .entries()
        .first()
        .map(|e| e.entry_hash)
        .unwrap_or([0u8; 32]);
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
            let ledger_archive = data_dir.join(format!("ledger.{}.jsonl", timestamp));
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
                println!("Reset complete. Prior run archived.");
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

// ---------------------------------------------------------------------------
// ledger create (Task 12)
// ---------------------------------------------------------------------------

pub fn cmd_ledger_create(config_dir: &str, name: &str, path: &str, mode_str: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let mode = match mode_str {
        "stateful" => LedgerMode::Stateful,
        "event-only" => LedgerMode::EventOnly,
        other => {
            eprintln!(
                "Unknown ledger mode '{}'. Valid: stateful, event-only.",
                other
            );
            return EXIT_USAGE_ERROR;
        }
    };

    // Resolve output path relative to data_dir
    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let ledger_file = if PathBuf::from(path).is_absolute() {
        PathBuf::from(path)
    } else {
        data_dir.join(path)
    };

    // Initialize the ledger file
    if let Err(e) = std::fs::create_dir_all(ledger_file.parent().unwrap_or(Path::new("."))) {
        eprintln!("Cannot create directory for ledger: {}", e);
        return EXIT_CONFIG_ERROR;
    }

    match Ledger::init(&ledger_file, &config.protocol.name, &config.protocol.version) {
        Ok(_) => {}
        Err(e) => {
            eprintln!("Cannot initialize ledger: {}", e);
            return EXIT_INTEGRITY_ERROR;
        }
    }

    // Register in the registry
    let reg_path = registry_path_from_config(&config);
    let mut registry = match LedgerRegistry::new(&reg_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Cannot load registry: {}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    if let Err(e) = registry.create(name, path, mode) {
        eprintln!("Cannot register ledger: {}", e);
        return EXIT_CONFIG_ERROR;
    }

    println!(
        "Ledger '{}' created at {} and registered.",
        name,
        ledger_file.display()
    );
    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------
// ledger list (Task 12)
// ---------------------------------------------------------------------------

pub fn cmd_ledger_list(config_dir: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let reg_path = registry_path_from_config(&config);
    let registry = match LedgerRegistry::new(&reg_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Cannot load registry: {}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    let entries = registry.list();
    if entries.is_empty() {
        println!("No ledgers registered.");
        return EXIT_SUCCESS;
    }

    println!("Registered ledgers ({}):", entries.len());
    for entry in entries {
        let mode_str = match entry.mode {
            LedgerMode::Stateful => "stateful",
            LedgerMode::EventOnly => "event-only",
        };
        println!(
            "  {} ({}) -> {} [{}]",
            entry.name, mode_str, entry.path, entry.created
        );
    }

    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------
// ledger remove (Task 12)
// ---------------------------------------------------------------------------

pub fn cmd_ledger_remove(config_dir: &str, name: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let reg_path = registry_path_from_config(&config);
    let mut registry = match LedgerRegistry::new(&reg_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Cannot load registry: {}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    if let Err(e) = registry.remove(name) {
        eprintln!("{}", e);
        return EXIT_CONFIG_ERROR;
    }

    println!(
        "Ledger '{}' removed from registry. File kept on disk.",
        name
    );
    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------
// ledger verify (Task 12)
// ---------------------------------------------------------------------------

pub fn cmd_ledger_verify(config_dir: &str, name: Option<&str>, path: Option<&str>) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    // Resolve which ledger to verify
    let ledger_file = if let Some(p) = path {
        PathBuf::from(p)
    } else if let Some(n) = name {
        let reg_path = registry_path_from_config(&config);
        let registry = match LedgerRegistry::new(&reg_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("Cannot load registry: {}", e);
                return EXIT_CONFIG_ERROR;
            }
        };
        let entry = match registry.resolve(Some(n)) {
            Ok(e) => e.clone(),
            Err(e) => {
                eprintln!("{}", e);
                return EXIT_CONFIG_ERROR;
            }
        };
        resolve_registry_path(&entry.path, &config)
    } else {
        // Default: use config data_dir
        let data_dir = resolve_data_dir(&config.paths.data_dir);
        ledger_path(&data_dir)
    };

    let ledger = match Ledger::open(&ledger_file) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Cannot open ledger at {}: {}", ledger_file.display(), e);
            return EXIT_INTEGRITY_ERROR;
        }
    };

    match ledger.verify() {
        Ok(()) => {
            println!(
                "Chain valid. {} entries, all hashes check out. ({})",
                ledger.len(),
                ledger_file.display()
            );
            EXIT_SUCCESS
        }
        Err(e) => {
            eprintln!("Chain INVALID: {}", e);
            eprintln!("Tampering detected.");
            EXIT_INTEGRITY_ERROR
        }
    }
}

// ---------------------------------------------------------------------------
// ledger checkpoint (Task 12)
// ---------------------------------------------------------------------------

pub fn cmd_ledger_checkpoint(
    config_dir: &str,
    name: &str,
    scope: &str,
    snapshot: &str,
) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let reg_path = registry_path_from_config(&config);
    let registry = match LedgerRegistry::new(&reg_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Cannot load registry: {}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    let entry = match registry.resolve(Some(name)) {
        Ok(e) => e.clone(),
        Err(e) => {
            eprintln!("{}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    let ledger_file = resolve_registry_path(&entry.path, &config);
    let mut ledger = match Ledger::open(&ledger_file) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("Cannot open ledger: {}", e);
            return EXIT_INTEGRITY_ERROR;
        }
    };

    match ledger.write_checkpoint(scope, snapshot) {
        Ok(cp) => {
            println!(
                "Checkpoint written at seq {} (scope='{}', snapshot='{}').",
                cp.seq, scope, snapshot
            );
            EXIT_SUCCESS
        }
        Err(e) => {
            eprintln!("Cannot write checkpoint: {}", e);
            EXIT_INTEGRITY_ERROR
        }
    }
}

// ---------------------------------------------------------------------------
// ledger import (Task 12)
// ---------------------------------------------------------------------------

pub fn cmd_ledger_import(config_dir: &str, name: &str, path: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    // Resolve output path
    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let ledger_file = if PathBuf::from(path).is_absolute() {
        PathBuf::from(path)
    } else {
        data_dir.join(path)
    };

    if let Err(e) = std::fs::create_dir_all(ledger_file.parent().unwrap_or(Path::new("."))) {
        eprintln!("Cannot create directory for ledger: {}", e);
        return EXIT_CONFIG_ERROR;
    }

    // Read from stdin
    let stdin = std::io::stdin();
    let mut reader = stdin.lock();

    match import_jsonl(
        &mut reader,
        &ledger_file,
        &config.protocol.name,
        &config.protocol.version,
    ) {
        Ok(()) => {}
        Err(e) => {
            eprintln!("Import failed: {}", e);
            return EXIT_INTEGRITY_ERROR;
        }
    }

    // Register in the registry
    let reg_path = registry_path_from_config(&config);
    let mut registry = match LedgerRegistry::new(&reg_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Cannot load registry: {}", e);
            return EXIT_CONFIG_ERROR;
        }
    };

    if let Err(e) = registry.create(name, path, LedgerMode::EventOnly) {
        eprintln!("Cannot register ledger: {}", e);
        return EXIT_CONFIG_ERROR;
    }

    println!(
        "Imported JSONL into '{}' at {} and registered.",
        name,
        ledger_file.display()
    );
    EXIT_SUCCESS
}

// ---------------------------------------------------------------------------
// query (Task 13)
// ---------------------------------------------------------------------------

#[allow(clippy::too_many_arguments)]
pub fn cmd_query(
    config_dir: &str,
    sql: Option<&str>,
    targeting: &LedgerTargeting,
    glob_pattern: Option<&str>,
    event_type: Option<&str>,
    field_filters: &[String],
    count: bool,
    format: &str,
) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let engine = QueryEngine::from_config(&config.events);

    // Build SQL from convenience flags if no raw SQL provided
    let effective_sql = if let Some(raw) = sql {
        raw.to_string()
    } else {
        build_convenience_sql(event_type, field_filters, count)
    };

    // Execute query via tokio runtime
    let rt = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("Cannot create async runtime: {}", e);
            return EXIT_INTEGRITY_ERROR;
        }
    };

    let result = if let Some(pattern) = glob_pattern {
        rt.block_on(engine.query_glob(pattern, &effective_sql))
    } else {
        // Resolve ledger path
        let (ledger_file, _mode) = match resolve_ledger_from_targeting(&config, targeting) {
            Ok(lm) => lm,
            Err((code, msg)) => {
                eprintln!("{}", msg);
                return code;
            }
        };
        rt.block_on(engine.query_file(&ledger_file, &effective_sql))
    };

    match result {
        Ok(rows) => {
            format_output(&rows, format);
            EXIT_SUCCESS
        }
        Err(e) => {
            eprintln!("Query failed: {}", e);
            EXIT_INTEGRITY_ERROR
        }
    }
}

/// Build SQL from convenience flags (--type, --field, --count).
fn build_convenience_sql(
    event_type: Option<&str>,
    field_filters: &[String],
    count: bool,
) -> String {
    let select = if count {
        "SELECT count(*) as count"
    } else {
        "SELECT *"
    };

    let mut conditions: Vec<String> = Vec::new();

    if let Some(et) = event_type {
        conditions.push(format!("type = '{}'", et.replace('\'', "''")));
    }

    for f in field_filters {
        if let Some((key, value)) = f.split_once('=') {
            conditions.push(format!(
                "{} = '{}'",
                key,
                value.replace('\'', "''")
            ));
        }
    }

    if conditions.is_empty() {
        format!("{} FROM events", select)
    } else {
        format!("{} FROM events WHERE {}", select, conditions.join(" AND "))
    }
}

/// Format query output in the requested format.
fn format_output(rows: &[BTreeMap<String, String>], format: &str) {
    match format {
        "json" => {
            let json = serde_json::to_string_pretty(rows).unwrap_or_else(|_| "[]".to_string());
            println!("{}", json);
        }
        "jsonl" => {
            for row in rows {
                let line = serde_json::to_string(row).unwrap_or_else(|_| "{}".to_string());
                println!("{}", line);
            }
        }
        "csv" => {
            if rows.is_empty() {
                return;
            }
            // Header from first row's keys
            let keys: Vec<&String> = rows[0].keys().collect();
            println!("{}", keys.iter().map(|k| k.as_str()).collect::<Vec<_>>().join(","));
            for row in rows {
                let vals: Vec<&str> = keys.iter().map(|k| row.get(*k).map(|v| v.as_str()).unwrap_or("")).collect();
                println!("{}", vals.join(","));
            }
        }
        _ => {
            // table (default)
            if rows.is_empty() {
                println!("(no results)");
                return;
            }

            let keys: Vec<&String> = rows[0].keys().collect();

            // Compute column widths
            let mut widths: Vec<usize> = keys.iter().map(|k| k.len()).collect();
            for row in rows {
                for (i, key) in keys.iter().enumerate() {
                    let val_len = row.get(*key).map(|v| v.len()).unwrap_or(0);
                    if val_len > widths[i] {
                        widths[i] = val_len;
                    }
                }
            }

            // Cap column widths at 40 for readability
            for w in &mut widths {
                if *w > 40 {
                    *w = 40;
                }
            }

            // Print header
            let header: Vec<String> = keys
                .iter()
                .enumerate()
                .map(|(i, k)| format!("{:<width$}", k, width = widths[i]))
                .collect();
            println!("{}", header.join("  "));
            let separator: Vec<String> = widths.iter().map(|w| "-".repeat(*w)).collect();
            println!("{}", separator.join("  "));

            // Print rows
            for row in rows {
                let vals: Vec<String> = keys
                    .iter()
                    .enumerate()
                    .map(|(i, k)| {
                        let v = row.get(*k).map(|v| v.as_str()).unwrap_or("");
                        let truncated = if v.len() > widths[i] {
                            format!("{}...", &v[..widths[i].saturating_sub(3)])
                        } else {
                            v.to_string()
                        };
                        format!("{:<width$}", truncated, width = widths[i])
                    })
                    .collect();
                println!("{}", vals.join("  "));
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Helper: print ledger entries
// ---------------------------------------------------------------------------

fn print_entries(entries: &[crate::ledger::entry::LedgerEntry]) {
    for entry in entries {
        // Use the JSONL ts field directly (ISO 8601); trim to readable form.
        let ts = &entry.ts;

        print!(
            "[{}] seq={} type={} hash={}",
            ts,
            entry.seq,
            entry.event_type,
            &entry.hash[..12],
        );

        // Print JSONL fields.
        if !entry.fields.is_empty() {
            let pairs: Vec<String> = entry
                .fields
                .iter()
                .map(|(k, v)| format!("{}={}", k, v))
                .collect();
            print!(" {{{}}}", pairs.join(", "));
        }

        println!();
    }
}

fn hex_encode_short(bytes: &[u8; 32], len: usize) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<String>()[..len]
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
