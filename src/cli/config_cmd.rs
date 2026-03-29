// src/cli/config_cmd.rs
//
// Config query commands.
//
// ## Index
// - [cmd-session-key-path] cmd_session_key_path() — print the resolved session key path

use super::authed_event::resolve_session_key_path;
use super::commands::{
    load_config, resolve_config_dir, resolve_data_dir, LedgerTargeting, EXIT_CONFIG_ERROR,
    EXIT_SUCCESS,
};

// [cmd-session-key-path]
pub fn cmd_session_key_path(config_dir: &str, targeting: &LedgerTargeting) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    let data_dir = resolve_data_dir(&config.paths.data_dir);
    let key_path = resolve_session_key_path(&data_dir, targeting);

    if !key_path.exists() {
        eprintln!(
            "error: session key not found at {}. Run 'sahjhan init' first.",
            key_path.display()
        );
        return EXIT_CONFIG_ERROR;
    }

    match key_path.canonicalize() {
        Ok(abs) => {
            println!("{}", abs.display());
            EXIT_SUCCESS
        }
        Err(e) => {
            eprintln!("error: cannot resolve session key path: {}", e);
            EXIT_CONFIG_ERROR
        }
    }
}
