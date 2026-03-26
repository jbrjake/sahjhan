// src/cli/aliases.rs
//
// Alias resolution: rewrites CLI arguments when the first subcommand matches
// an alias defined in protocol.toml [aliases].

use std::collections::HashMap;
use std::path::Path;

use crate::config::ProtocolConfig;

/// Attempt to resolve an alias from the raw CLI args.
///
/// If the first positional argument (after flags like `--config-dir`) matches
/// an alias key, the args are rewritten by replacing that argument with the
/// alias expansion (which may be multiple words, e.g. "transition begin").
///
/// Returns `None` if no alias matched; returns `Some(new_args)` if rewriting
/// occurred.
pub fn resolve_alias(args: &[String], config_dir: &str) -> Option<Vec<String>> {
    // Try to load config to get aliases.  If config can't be loaded, no alias
    // resolution is possible — silently return None and let clap handle it.
    let config = ProtocolConfig::load(Path::new(config_dir)).ok()?;

    if config.aliases.is_empty() {
        return None;
    }

    // Find the first positional arg (skip binary name and --config-dir/value pairs).
    let (prefix, subcommand_idx) = find_subcommand_index(args)?;

    let subcommand = &args[subcommand_idx];

    if let Some(expansion) = config.aliases.get(subcommand.as_str()) {
        let expanded_words: Vec<&str> = expansion.split_whitespace().collect();
        let mut new_args = Vec::with_capacity(args.len() + expanded_words.len());
        new_args.extend_from_slice(&prefix);
        for word in &expanded_words {
            new_args.push(word.to_string());
        }
        // Append any remaining args after the alias
        if subcommand_idx + 1 < args.len() {
            new_args.extend_from_slice(&args[subcommand_idx + 1..]);
        }
        Some(new_args)
    } else {
        None
    }
}

/// Find the index of the first subcommand argument and the prefix before it.
///
/// Skips over the binary name and any `--config-dir <value>` pairs.
/// Returns (prefix_args, subcommand_index) or None if no subcommand found.
fn find_subcommand_index(args: &[String]) -> Option<(Vec<String>, usize)> {
    let mut prefix = Vec::new();
    let mut i = 0;

    // Binary name
    if i < args.len() {
        prefix.push(args[i].clone());
        i += 1;
    }

    // Skip flag pairs
    while i < args.len() {
        if args[i] == "--config-dir" {
            prefix.push(args[i].clone());
            i += 1;
            if i < args.len() {
                prefix.push(args[i].clone());
                i += 1;
            }
        } else if args[i].starts_with("--config-dir=") {
            prefix.push(args[i].clone());
            i += 1;
        } else if args[i].starts_with('-') {
            // Other flags — skip
            prefix.push(args[i].clone());
            i += 1;
        } else {
            // This is the subcommand
            break;
        }
    }

    if i < args.len() {
        Some((prefix, i))
    } else {
        None
    }
}

/// Resolve aliases given already-parsed aliases map and the raw args.
///
/// This is a simpler version used when we already have the aliases loaded.
pub fn resolve_with_map(args: &[String], aliases: &HashMap<String, String>) -> Option<Vec<String>> {
    if aliases.is_empty() {
        return None;
    }

    let (prefix, subcommand_idx) = find_subcommand_index(args)?;
    let subcommand = &args[subcommand_idx];

    if let Some(expansion) = aliases.get(subcommand.as_str()) {
        let expanded_words: Vec<&str> = expansion.split_whitespace().collect();
        let mut new_args = Vec::with_capacity(args.len() + expanded_words.len());
        new_args.extend_from_slice(&prefix);
        for word in &expanded_words {
            new_args.push(word.to_string());
        }
        if subcommand_idx + 1 < args.len() {
            new_args.extend_from_slice(&args[subcommand_idx + 1..]);
        }
        Some(new_args)
    } else {
        None
    }
}
