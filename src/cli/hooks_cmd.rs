// src/cli/hooks_cmd.rs
//
// Hook generation commands.
//
// ## Index
// - [cmd-hook-generate] cmd_hook_generate() — generate hook scripts for a harness

use std::path::PathBuf;

use super::commands::{
    load_config, resolve_config_dir, EXIT_CONFIG_ERROR, EXIT_SUCCESS,
};

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
