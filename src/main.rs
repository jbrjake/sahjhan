// src/main.rs
//
// Sahjhan CLI entry point.
// Parses arguments with clap, resolves aliases, and delegates to command
// implementations in cli/ modules.
//
// ## Index
// - [cli-main]               main()  — CLI entry point, clap parsing, dispatch
// - Cli                      — top-level clap struct
// - Commands                 — subcommand enum
// - SetAction                — set subcommand enum
// - GateAction               — gate subcommand enum
// - LedgerAction             — ledger subcommand enum
// - ConfigAction             — config subcommand enum

use clap::{Parser, Subcommand};

use sahjhan::cli::aliases;
use sahjhan::cli::authed_event;
use sahjhan::cli::commands;
use sahjhan::cli::config_cmd;
use sahjhan::cli::guards;
use sahjhan::cli::hooks_cmd;
use sahjhan::cli::mermaid as mermaid_cmd;
use sahjhan::cli::init;
use sahjhan::cli::ledger;
use sahjhan::cli::log;
use sahjhan::cli::manifest_cmd;
use sahjhan::cli::query;
use sahjhan::cli::render;
use sahjhan::cli::status;
use sahjhan::cli::transition;

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

    /// Target a named ledger from the registry
    #[arg(long, global = true)]
    ledger: Option<String>,

    /// Target a ledger file by path directly
    #[arg(long = "ledger-path", global = true)]
    ledger_path: Option<String>,

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
    Render {
        /// Dump the template render context as JSON instead of rendering
        #[arg(long)]
        dump_context: bool,
    },

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

    /// Record a restricted event with HMAC proof
    AuthedEvent {
        /// Event type
        #[arg(value_name = "TYPE")]
        event_type: String,

        /// Field values (key=value)
        #[arg(long = "field", value_name = "KEY=VALUE")]
        fields: Vec<String>,

        /// HMAC-SHA256 proof
        #[arg(long)]
        proof: String,
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

    /// Multi-ledger management
    Ledger {
        #[command(subcommand)]
        action: LedgerAction,
    },

    /// Configuration queries
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Show read-guard manifest for enforcement hooks
    Guards,

    /// Generate protocol diagram (Mermaid or ASCII)
    Mermaid {
        /// Output ASCII art instead of raw Mermaid text
        #[arg(long)]
        rendered: bool,
    },

    /// SQL queries over ledger events
    Query {
        /// SQL query string
        sql: Option<String>,

        /// Target ledger file path (for query)
        #[arg(long = "path")]
        query_path: Option<String>,

        /// Glob pattern for multi-file queries
        #[arg(long)]
        glob: Option<String>,

        /// Filter by event type
        #[arg(long = "type")]
        event_type: Option<String>,

        /// Filter by field value (key=value)
        #[arg(long = "field", value_name = "KEY=VALUE")]
        fields: Vec<String>,

        /// Show count only
        #[arg(long)]
        count: bool,

        /// Output format: table, json, csv, jsonl
        #[arg(long, default_value = "table")]
        format: String,

        /// Shortcut for --format json
        #[arg(long)]
        json: bool,
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

        /// Additional arguments (key=value pairs for template variables)
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
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

#[derive(Subcommand)]
enum LedgerAction {
    /// Register and initialize a new named ledger
    Create {
        /// Ledger name (for direct creation without template)
        #[arg(long, required_unless_present = "from")]
        name: Option<String>,

        /// File path for the new ledger (for direct creation without template)
        #[arg(long, required_unless_present = "from")]
        path: Option<String>,

        /// Create from a protocol-declared ledger template
        #[arg(long)]
        from: Option<String>,

        /// Instance identifier for the template (e.g., "25" creates run-25)
        #[arg(requires = "from")]
        instance_id: Option<String>,

        /// Ledger mode: stateful or event-only
        #[arg(long, default_value = "stateful")]
        mode: String,
    },
    /// List registered ledgers
    List,
    /// Remove a ledger from the registry (keeps the file)
    Remove {
        /// Ledger name
        #[arg(long)]
        name: String,
    },
    /// Verify hash chain integrity of a ledger
    Verify {
        /// Ledger name (from registry)
        #[arg(long)]
        name: Option<String>,

        /// Ledger file path (direct)
        #[arg(long)]
        path: Option<String>,
    },
    /// Write a checkpoint to a ledger
    Checkpoint {
        /// Ledger name
        #[arg(long)]
        name: String,

        /// Checkpoint scope
        #[arg(long, default_value = "state")]
        scope: String,

        /// Checkpoint snapshot description
        #[arg(long, default_value = "cli-checkpoint")]
        snapshot: String,
    },
    /// Import bare JSONL from stdin into a new named ledger
    Import {
        /// Ledger name (for registry)
        #[arg(long)]
        name: String,

        /// Output file path for the new ledger
        #[arg(long)]
        path: String,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Print the resolved session key path
    SessionKeyPath,
}

// [cli-main]
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

    let targeting = commands::LedgerTargeting {
        ledger_name: cli.ledger.clone(),
        ledger_path: cli.ledger_path.clone(),
    };

    let exit_code = match cli.command {
        Commands::Validate => init::cmd_validate(&cli.config_dir),
        Commands::Init => init::cmd_init(&cli.config_dir),
        Commands::Status => status::cmd_status(&cli.config_dir, &targeting),
        Commands::Log { action } => match action {
            LogAction::Dump => log::cmd_log_dump(&cli.config_dir, &targeting),
            LogAction::Verify => log::cmd_log_verify(&cli.config_dir, &targeting),
            LogAction::Tail { n } => log::cmd_log_tail(&cli.config_dir, n, &targeting),
        },
        Commands::Manifest { action } => match action {
            ManifestAction::Verify => manifest_cmd::cmd_manifest_verify(&cli.config_dir),
            ManifestAction::List => manifest_cmd::cmd_manifest_list(&cli.config_dir),
            ManifestAction::Restore { path } => {
                manifest_cmd::cmd_manifest_restore(&cli.config_dir, &path)
            }
        },
        Commands::Render { dump_context } => {
            if dump_context {
                render::cmd_render_dump_context(&cli.config_dir, &targeting)
            } else {
                render::cmd_render(&cli.config_dir, &targeting)
            }
        }
        Commands::Set { action } => match action {
            SetAction::Status { set } => status::cmd_set_status(&cli.config_dir, &set, &targeting),
            SetAction::Complete { set, member } => {
                status::cmd_set_complete(&cli.config_dir, &set, &member, &targeting)
            }
        },
        Commands::Transition { name, args } => {
            transition::cmd_transition(&cli.config_dir, &name, &args, &targeting)
        }
        Commands::Gate { action } => match action {
            GateAction::Check { transition, args } => {
                transition::cmd_gate_check(&cli.config_dir, &transition, &args, &targeting)
            }
        },
        Commands::Event { event_type, fields } => {
            transition::cmd_event(&cli.config_dir, &event_type, &fields, &targeting)
        }
        Commands::AuthedEvent {
            event_type,
            fields,
            proof,
        } => authed_event::cmd_authed_event(
            &cli.config_dir,
            &event_type,
            &fields,
            &proof,
            &targeting,
        ),
        Commands::Reset { confirm, token } => init::cmd_reset(&cli.config_dir, confirm, &token),
        Commands::Hook { action } => match action {
            HookAction::Generate {
                harness,
                output_dir,
            } => hooks_cmd::cmd_hook_generate(&cli.config_dir, &harness, &output_dir),
        },
        Commands::Ledger { action } => match action {
            LedgerAction::Create {
                name,
                path,
                from,
                instance_id,
                mode,
            } => ledger::cmd_ledger_create(
                &cli.config_dir,
                name.as_deref(),
                path.as_deref(),
                from.as_deref(),
                instance_id.as_deref(),
                &mode,
            ),
            LedgerAction::List => ledger::cmd_ledger_list(&cli.config_dir),
            LedgerAction::Remove { name } => ledger::cmd_ledger_remove(&cli.config_dir, &name),
            LedgerAction::Verify { name, path } => {
                ledger::cmd_ledger_verify(&cli.config_dir, name.as_deref(), path.as_deref())
            }
            LedgerAction::Checkpoint {
                name,
                scope,
                snapshot,
            } => ledger::cmd_ledger_checkpoint(&cli.config_dir, &name, &scope, &snapshot),
            LedgerAction::Import { name, path } => {
                ledger::cmd_ledger_import(&cli.config_dir, &name, &path)
            }
        },
        Commands::Config { action } => match action {
            ConfigAction::SessionKeyPath => {
                config_cmd::cmd_session_key_path(&cli.config_dir, &targeting)
            }
        },
        Commands::Guards => guards::cmd_guards(&cli.config_dir),
        Commands::Mermaid { rendered } => mermaid_cmd::cmd_mermaid(&cli.config_dir, rendered),
        Commands::Query {
            sql,
            query_path,
            glob,
            event_type,
            fields,
            count,
            format,
            json,
        } => {
            let effective_format = if json { "json".to_string() } else { format };
            // For query, use --path from query subcommand, falling back to global flags
            let query_targeting = commands::LedgerTargeting {
                ledger_name: cli.ledger,
                ledger_path: query_path.or(cli.ledger_path),
            };
            query::cmd_query(
                &cli.config_dir,
                sql.as_deref(),
                &query_targeting,
                glob.as_deref(),
                event_type.as_deref(),
                &fields,
                count,
                &effective_format,
            )
        }
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
