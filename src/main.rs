// src/main.rs
//
// Sahjhan CLI entry point.
// Parses arguments with clap, resolves aliases, and delegates to command
// implementations in cli::commands.

use clap::{Parser, Subcommand};

use sahjhan::cli::aliases;
use sahjhan::cli::commands;

#[derive(Parser)]
#[command(
    name = "sahjhan",
    version,
    about = "Protocol enforcement engine for AI agents"
)]
struct Cli {
    /// Path to protocol config directory
    #[arg(long, default_value = "enforcement", global = true)]
    config_dir: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Validate protocol config without initializing a run
    Validate,

    /// Initialize ledger, manifest, genesis block
    Init,

    /// Show current state, set progress, gate status
    Status,

    /// Ledger operations
    Log {
        #[command(subcommand)]
        action: LogAction,
    },

    /// Manifest operations
    Manifest {
        #[command(subcommand)]
        action: ManifestAction,
    },

    /// Regenerate all markdown views
    Render,

    /// Set operations
    Set {
        #[command(subcommand)]
        action: SetAction,
    },

    /// Execute a named transition (runs gates)
    Transition {
        /// Transition command name
        name: String,

        /// Additional arguments
        #[arg(trailing_var_arg = true)]
        args: Vec<String>,
    },

    /// Gate operations
    Gate {
        #[command(subcommand)]
        action: GateAction,
    },

    /// Record a protocol event
    Event {
        /// Event type
        #[arg(value_name = "TYPE")]
        event_type: String,

        /// Field values (key=value)
        #[arg(long = "field", value_name = "KEY=VALUE")]
        fields: Vec<String>,
    },

    /// Archive current run and start fresh
    Reset {
        /// Confirm the reset
        #[arg(long)]
        confirm: bool,

        /// Confirmation token derived from genesis hash
        #[arg(long)]
        token: Option<String>,
    },

    /// Generate hook scripts for a harness
    Hook {
        #[command(subcommand)]
        action: HookAction,
    },
}

#[derive(Subcommand)]
enum LogAction {
    /// Human-readable ledger dump
    Dump,
    /// Validate hash chain integrity
    Verify,
    /// Show last N events (default 10)
    Tail {
        /// Number of entries to show
        #[arg(default_value = "10")]
        n: usize,
    },
}

#[derive(Subcommand)]
enum ManifestAction {
    /// Check managed files against manifest
    Verify,
    /// Show managed files and hashes
    List,
    /// Restore file from last known-good state
    Restore {
        /// File path to restore
        path: String,
    },
}

#[derive(Subcommand)]
enum SetAction {
    /// Show completion status for a set
    Status {
        /// Set name
        set: String,
    },
    /// Record member completion (runs gates)
    Complete {
        /// Set name
        set: String,
        /// Member name
        member: String,
    },
}

#[derive(Subcommand)]
enum GateAction {
    /// Dry-run: show which gates pass/fail
    Check {
        /// Transition name
        transition: String,
    },
}

#[derive(Subcommand)]
enum HookAction {
    /// Generate hook scripts
    Generate {
        /// Harness type (e.g. "cc" for claude code)
        #[arg(long)]
        harness: Option<String>,

        /// Output directory for generated hook files
        #[arg(long)]
        output_dir: Option<String>,
    },
}

fn main() {
    // Collect raw args for alias resolution
    let raw_args: Vec<String> = std::env::args().collect();

    // Attempt alias resolution before clap parsing
    let effective_args = match aliases::resolve_alias(&raw_args, &extract_config_dir(&raw_args)) {
        Some(rewritten) => rewritten,
        None => raw_args,
    };

    let cli = match Cli::try_parse_from(&effective_args) {
        Ok(c) => c,
        Err(e) => {
            e.exit();
        }
    };

    let exit_code = match cli.command {
        Commands::Validate => commands::cmd_validate(&cli.config_dir),
        Commands::Init => commands::cmd_init(&cli.config_dir),
        Commands::Status => commands::cmd_status(&cli.config_dir),
        Commands::Log { action } => match action {
            LogAction::Dump => commands::cmd_log_dump(&cli.config_dir),
            LogAction::Verify => commands::cmd_log_verify(&cli.config_dir),
            LogAction::Tail { n } => commands::cmd_log_tail(&cli.config_dir, n),
        },
        Commands::Manifest { action } => match action {
            ManifestAction::Verify => commands::cmd_manifest_verify(&cli.config_dir),
            ManifestAction::List => commands::cmd_manifest_list(&cli.config_dir),
            ManifestAction::Restore { path } => {
                commands::cmd_manifest_restore(&cli.config_dir, &path)
            }
        },
        Commands::Render => commands::cmd_render(&cli.config_dir),
        Commands::Set { action } => match action {
            SetAction::Status { set } => commands::cmd_set_status(&cli.config_dir, &set),
            SetAction::Complete { set, member } => {
                commands::cmd_set_complete(&cli.config_dir, &set, &member)
            }
        },
        Commands::Transition { name, args } => {
            commands::cmd_transition(&cli.config_dir, &name, &args)
        }
        Commands::Gate { action } => match action {
            GateAction::Check { transition } => {
                commands::cmd_gate_check(&cli.config_dir, &transition)
            }
        },
        Commands::Event { event_type, fields } => {
            commands::cmd_event(&cli.config_dir, &event_type, &fields)
        }
        Commands::Reset { confirm, token } => commands::cmd_reset(&cli.config_dir, confirm, &token),
        Commands::Hook { action } => match action {
            HookAction::Generate {
                harness,
                output_dir,
            } => commands::cmd_hook_generate(&cli.config_dir, &harness, &output_dir),
        },
    };

    std::process::exit(exit_code);
}

/// Extract --config-dir value from raw args (before clap parsing).
fn extract_config_dir(args: &[String]) -> String {
    for i in 0..args.len() {
        if args[i] == "--config-dir" {
            if let Some(val) = args.get(i + 1) {
                return val.clone();
            }
        } else if let Some(val) = args[i].strip_prefix("--config-dir=") {
            return val.to_string();
        }
    }
    "enforcement".to_string()
}
