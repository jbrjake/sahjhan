// src/cli/mermaid.rs
//
// CLI command for protocol visualization.
//
// ## Index
// - [cmd-mermaid] cmd_mermaid() — generate Mermaid or ASCII diagram

use super::commands::{load_config, resolve_config_dir, EXIT_SUCCESS};

// [cmd-mermaid]
pub fn cmd_mermaid(config_dir: &str, rendered: bool) -> i32 {
    let config_path = resolve_config_dir(config_dir);
    let config = match load_config(&config_path) {
        Ok(c) => c,
        Err((code, msg)) => {
            eprintln!("{}", msg);
            return code;
        }
    };

    if rendered {
        println!("{}", crate::mermaid::generate_ascii(&config));
    } else {
        println!("{}", crate::mermaid::generate_mermaid(&config));
    }

    EXIT_SUCCESS
}
