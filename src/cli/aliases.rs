// src/cli/aliases.rs
//
// Alias resolution: rewrites CLI arguments when the first subcommand
// matches an alias defined in protocol.toml [aliases].
//
// ## Index
// - [resolve-alias]          resolve_alias()     — resolve alias from raw CLI args
// - [resolve-with-map]       resolve_with_map()  — resolve given already-parsed alias map
// - [expand-alias]           expand()            — longest-key match + arg rewrite

use std::collections::HashMap;
use std::path::Path;

use crate::config::ProtocolConfig;

// [resolve-alias]
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

    expand(args, &config.aliases)
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

// [resolve-with-map]
/// Resolve aliases given already-parsed aliases map and the raw args.
///
/// This is a simpler version used when we already have the aliases loaded.
pub fn resolve_with_map(args: &[String], aliases: &HashMap<String, String>) -> Option<Vec<String>> {
    expand(args, aliases)
}

// [expand-alias]
/// Match the widest alias key the args begin with, and rewrite them.
///
/// Keys may name more than one word — `"defer low" = "transition defer_low"`
/// makes `sahjhan defer low BH-001` legal. Matching only the first word, as
/// this did until 0.22.0, silently ignored every such key: the config loaded,
/// `validate` passed, and the command died at clap with `unrecognized
/// subcommand 'defer'`.
///
/// Two rules make the match unambiguous:
///
/// * **Widest key wins.** With both `"defer"` and `"defer low"` declared, the
///   longer one is what `sahjhan defer low` means; the shorter would swallow
///   `low` as an argument to a different command.
/// * **A flag ends the key.** Candidate words stop at the first `-`-prefixed
///   arg, so `sahjhan defer batch --severity low,medium` matches
///   `"defer batch"` and leaves the flag for the expanded command to parse.
fn expand(args: &[String], aliases: &HashMap<String, String>) -> Option<Vec<String>> {
    if aliases.is_empty() {
        return None;
    }

    let (prefix, subcommand_idx) = find_subcommand_index(args)?;

    let widest = aliases
        .keys()
        .map(|k| k.split_whitespace().count())
        .max()
        .unwrap_or(1);
    let available = args[subcommand_idx..]
        .iter()
        .take_while(|a| !a.starts_with('-'))
        .count();

    for len in (1..=widest.min(available)).rev() {
        let key = args[subcommand_idx..subcommand_idx + len].join(" ");
        let Some(expansion) = aliases.get(&key) else {
            continue;
        };
        let expanded_words: Vec<&str> = expansion.split_whitespace().collect();
        let mut new_args = Vec::with_capacity(args.len() + expanded_words.len());
        new_args.extend_from_slice(&prefix);
        for word in &expanded_words {
            new_args.push(word.to_string());
        }
        new_args.extend_from_slice(&args[subcommand_idx + len..]);
        return Some(new_args);
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(words: &[&str]) -> Vec<String> {
        words.iter().map(|w| w.to_string()).collect()
    }

    fn aliases(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn one_word_key_still_resolves() {
        let out = expand(
            &args(["sahjhan", "start"].as_ref()),
            &aliases(&[("start", "transition begin")]),
        );
        assert_eq!(out, Some(args(&["sahjhan", "transition", "begin"])));
    }

    #[test]
    fn two_word_key_resolves_and_keeps_its_argument() {
        let out = expand(
            &args(&[
                "sahjhan",
                "--config-dir",
                "enforcement",
                "defer",
                "low",
                "BH-001",
            ]),
            &aliases(&[("defer low", "transition defer_low")]),
        );
        assert_eq!(
            out,
            Some(args(&[
                "sahjhan",
                "--config-dir",
                "enforcement",
                "transition",
                "defer_low",
                "BH-001",
            ]))
        );
    }

    #[test]
    fn a_flag_after_the_key_survives_the_rewrite() {
        let out = expand(
            &args(&["sahjhan", "defer", "batch", "--severity", "low,medium"]),
            &aliases(&[("defer batch", "batch defer")]),
        );
        assert_eq!(
            out,
            Some(args(&[
                "sahjhan",
                "batch",
                "defer",
                "--severity",
                "low,medium",
            ]))
        );
    }

    #[test]
    fn the_widest_key_wins() {
        // A one-word key would swallow "low" as an argument to the wrong
        // command, and the caller could never reach the two-word alias.
        let out = expand(
            &args(&["sahjhan", "defer", "low", "BH-001"]),
            &aliases(&[
                ("defer", "transition defer_any"),
                ("defer low", "transition defer_low"),
            ]),
        );
        assert_eq!(
            out,
            Some(args(&["sahjhan", "transition", "defer_low", "BH-001"]))
        );
    }

    #[test]
    fn a_key_cannot_reach_across_a_flag() {
        let out = expand(
            &args(&["sahjhan", "defer", "--dry-run", "low"]),
            &aliases(&[("defer low", "transition defer_low")]),
        );
        assert_eq!(out, None);
    }

    #[test]
    fn an_unmatched_first_word_is_left_alone() {
        let out = expand(
            &args(&["sahjhan", "status"]),
            &aliases(&[("defer low", "transition defer_low")]),
        );
        assert_eq!(out, None);
    }
}
