// src/cli/render.rs
//
// Template rendering commands.
//
// ## Index
// - [cmd-render] cmd_render() — regenerate all markdown views
// - [cmd-render-dump-context] cmd_render_dump_context() — dump template render context as JSON

use crate::render::engine::RenderEngine;

use super::commands::{
    load_config, load_manifest, open_targeted_ledger, resolve_config_dir, resolve_data_dir,
    save_manifest, LedgerTargeting, EXIT_CONFIG_ERROR, EXIT_INTEGRITY_ERROR, EXIT_SUCCESS,
};

// ---------------------------------------------------------------------------
// render
// ---------------------------------------------------------------------------

// [cmd-render]
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

    let registry_path = super::commands::registry_path_from_config(&config);
    let engine = match RenderEngine::new(&config, &config_path) {
        Ok(e) => e.with_registry(registry_path),
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

// [cmd-render-dump-context]
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

    let registry_path = super::commands::registry_path_from_config(&config);
    let engine = match RenderEngine::new(&config, &config_path) {
        Ok(e) => e.with_registry(registry_path),
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
