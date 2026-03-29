// src/cli/guards.rs
//
// Read-guard manifest command.
//
// ## Index
// - [cmd-guards] cmd_guards() — output JSON manifest of read-blocked paths

use super::commands::{load_config, resolve_config_dir, EXIT_SUCCESS};

// [cmd-guards]
pub fn cmd_guards(config_dir: &str) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let mut read_blocked: Vec<String> = config
        .guards
        .as_ref()
        .map(|g| g.read_blocked.clone())
        .unwrap_or_default();

    // Auto-include session key path (defense in depth)
    let session_key_path = format!("{}/session.key", config.paths.data_dir);
    if !read_blocked.contains(&session_key_path) {
        read_blocked.push(session_key_path);
    }

    // Deduplicate while preserving order
    let mut seen = std::collections::HashSet::new();
    read_blocked.retain(|p| seen.insert(p.clone()));

    let output = serde_json::json!({
        "read_blocked": read_blocked
    });

    println!("{}", serde_json::to_string_pretty(&output).unwrap());

    EXIT_SUCCESS
}
