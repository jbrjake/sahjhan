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
// - DaemonAction             — daemon subcommand enum
// - VaultAction              — vault subcommand enum

use clap::{Parser, Subcommand};

use sahjhan::cli::aliases;
use sahjhan::cli::authed_event;
use sahjhan::cli::commands;
use sahjhan::cli::daemon_cmd;
use sahjhan::cli::guards;
use sahjhan::cli::hooks_cmd;
use sahjhan::cli::init;
use sahjhan::cli::ledger;
use sahjhan::cli::log;
use sahjhan::cli::manifest_cmd;
use sahjhan::cli::mermaid as mermaid_cmd;
use sahjhan::cli::query;
use sahjhan::cli::render;
use sahjhan::cli::sign_cmd;
use sahjhan::cli::status;
use sahjhan::cli::transition;
use sahjhan::cli::vault_cmd;
use sahjhan::cli::verify_cmd;

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

    /// Output JSON instead of text
    #[arg(long, global = true)]
    json: bool,

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

    /// Re-seal config file hashes after legitimate changes (requires HMAC proof)
    Reseal {
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

    /// Daemon process management
    Daemon {
        #[command(subcommand)]
        action: DaemonAction,
    },

    /// Request HMAC-SHA256 proof from daemon
    Sign {
        /// Event type
        #[arg(long = "event-type")]
        event_type: String,

        /// Field values (key=value)
        #[arg(long = "field", value_name = "KEY=VALUE")]
        fields: Vec<String>,
    },

    /// Vault operations (in-memory secret store)
    Vault {
        #[command(subcommand)]
        action: VaultAction,
    },

    /// Verify an HMAC-SHA256 proof via daemon
    Verify {
        /// Event type
        #[arg(long = "event-type")]
        event_type: String,

        /// Field values (key=value)
        #[arg(long = "field", value_name = "KEY=VALUE")]
        fields: Vec<String>,

        /// HMAC-SHA256 proof to verify
        #[arg(long)]
        proof: String,
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
    /// Evaluate hook rules against current state
    Eval {
        /// Hook event type (PreToolUse, PostToolUse, Stop)
        #[arg(long)]
        event: String,
        /// Tool name
        #[arg(long)]
        tool: Option<String>,
        /// File path being operated on
        #[arg(long)]
        file: Option<String>,
        /// Agent output text (for Stop hooks)
        #[arg(long)]
        output_text: Option<String>,
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
enum DaemonAction {
    /// Start daemon in foreground
    Start,
    /// Stop running daemon
    Stop,
    /// Query daemon status
    Status,
}

#[derive(Subcommand)]
enum VaultAction {
    /// Store data in daemon vault
    Store {
        /// Entry name
        #[arg(long)]
        name: String,
        /// File to read data from
        #[arg(long)]
        file: String,
    },
    /// Read data from daemon vault
    Read {
        /// Entry name
        #[arg(long)]
        name: String,
    },
    /// Delete vault entry
    Delete {
        /// Entry name
        #[arg(long)]
        name: String,
    },
    /// List vault entry names
    List,
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

    use sahjhan::cli::output::{CommandOutput, LegacyResult};

    // Whether to emit JSON envelope at the end.
    // Normally follows cli.json, but Query's own --json flag conflicts with the
    // global one (both share the same name via `global = true`), so we suppress
    // the envelope for Query when its local json flag was the trigger.
    let mut use_json_envelope = cli.json;

    let result: Box<dyn CommandOutput> = match cli.command {
        // Converted commands return Box<dyn CommandOutput> directly
        Commands::Status => status::cmd_status(&cli.config_dir, &targeting),
        Commands::Log { action } => match action {
            LogAction::Dump => log::cmd_log_dump(&cli.config_dir, &targeting),
            LogAction::Tail { n } => log::cmd_log_tail(&cli.config_dir, n, &targeting),
            LogAction::Verify => {
                let code = log::cmd_log_verify(&cli.config_dir, &targeting);
                Box::new(LegacyResult::new("log_verify", code))
            }
        },
        Commands::Manifest { action } => match action {
            ManifestAction::Verify => manifest_cmd::cmd_manifest_verify(&cli.config_dir),
            ManifestAction::List => {
                let code = manifest_cmd::cmd_manifest_list(&cli.config_dir);
                Box::new(LegacyResult::new("manifest_list", code))
            }
            ManifestAction::Restore { path } => {
                let code = manifest_cmd::cmd_manifest_restore(&cli.config_dir, &path);
                Box::new(LegacyResult::new("manifest_restore", code))
            }
        },
        Commands::Set { action } => match action {
            SetAction::Status { set } => status::cmd_set_status(&cli.config_dir, &set, &targeting),
            SetAction::Complete { set, member } => {
                let code = status::cmd_set_complete(&cli.config_dir, &set, &member, &targeting);
                Box::new(LegacyResult::new("set_complete", code))
            }
        },
        Commands::Gate { action } => match action {
            GateAction::Check { transition, args } => {
                transition::cmd_gate_check(&cli.config_dir, &transition, &args, &targeting)
            }
        },
        // All other commands wrapped in LegacyResult
        Commands::Validate => {
            let code = init::cmd_validate(&cli.config_dir);
            Box::new(LegacyResult::new("validate", code))
        }
        Commands::Init => {
            let code = init::cmd_init(&cli.config_dir);
            Box::new(LegacyResult::new("init", code))
        }
        Commands::Render { dump_context } => {
            let code = if dump_context {
                render::cmd_render_dump_context(&cli.config_dir, &targeting)
            } else {
                render::cmd_render(&cli.config_dir, &targeting)
            };
            Box::new(LegacyResult::new("render", code))
        }
        Commands::Transition { name, args } => {
            let code = transition::cmd_transition(&cli.config_dir, &name, &args, &targeting);
            Box::new(LegacyResult::new("transition", code))
        }
        Commands::Event { event_type, fields } => {
            let code = transition::cmd_event(&cli.config_dir, &event_type, &fields, &targeting);
            Box::new(LegacyResult::new("event", code))
        }
        Commands::AuthedEvent {
            event_type,
            fields,
            proof,
        } => {
            let code = authed_event::cmd_authed_event(
                &cli.config_dir,
                &event_type,
                &fields,
                &proof,
                &targeting,
            );
            Box::new(LegacyResult::new("authed_event", code))
        }
        Commands::Reseal { proof } => {
            let code = authed_event::cmd_reseal(&cli.config_dir, &proof, &targeting);
            Box::new(LegacyResult::new("reseal", code))
        }
        Commands::Reset { confirm, token } => {
            let code = init::cmd_reset(&cli.config_dir, confirm, &token);
            Box::new(LegacyResult::new("reset", code))
        }
        Commands::Hook { action } => match action {
            HookAction::Generate {
                harness,
                output_dir,
            } => {
                let code = hooks_cmd::cmd_hook_generate(&cli.config_dir, &harness, &output_dir);
                Box::new(LegacyResult::new("hook_generate", code))
            }
            HookAction::Eval {
                event,
                tool,
                file,
                output_text,
            } => hooks_cmd::cmd_hook_eval(
                &cli.config_dir,
                &event,
                &tool,
                &file,
                &output_text,
                &targeting,
            ),
        },
        Commands::Ledger { action } => match action {
            LedgerAction::Create {
                name,
                path,
                from,
                instance_id,
                mode,
            } => {
                let code = ledger::cmd_ledger_create(
                    &cli.config_dir,
                    name.as_deref(),
                    path.as_deref(),
                    from.as_deref(),
                    instance_id.as_deref(),
                    &mode,
                );
                Box::new(LegacyResult::new("ledger_create", code))
            }
            LedgerAction::List => {
                let code = ledger::cmd_ledger_list(&cli.config_dir);
                Box::new(LegacyResult::new("ledger_list", code))
            }
            LedgerAction::Remove { name } => {
                let code = ledger::cmd_ledger_remove(&cli.config_dir, &name);
                Box::new(LegacyResult::new("ledger_remove", code))
            }
            LedgerAction::Verify { name, path } => {
                let code =
                    ledger::cmd_ledger_verify(&cli.config_dir, name.as_deref(), path.as_deref());
                Box::new(LegacyResult::new("ledger_verify", code))
            }
            LedgerAction::Checkpoint {
                name,
                scope,
                snapshot,
            } => {
                let code = ledger::cmd_ledger_checkpoint(&cli.config_dir, &name, &scope, &snapshot);
                Box::new(LegacyResult::new("ledger_checkpoint", code))
            }
            LedgerAction::Import { name, path } => {
                let code = ledger::cmd_ledger_import(&cli.config_dir, &name, &path);
                Box::new(LegacyResult::new("ledger_import", code))
            }
        },
        Commands::Guards => {
            let code = guards::cmd_guards(&cli.config_dir);
            Box::new(LegacyResult::new("guards", code))
        }
        Commands::Mermaid { rendered } => {
            let code = mermaid_cmd::cmd_mermaid(&cli.config_dir, rendered);
            Box::new(LegacyResult::new("mermaid", code))
        }
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
            // When query's own --json flag is set (shortcut for --format json),
            // it also sets cli.json via the global flag.  Disable the envelope
            // so the raw DataFusion JSON output is not double-wrapped.
            if json {
                use_json_envelope = false;
            }
            let effective_format = if json { "json".to_string() } else { format };
            let query_targeting = commands::LedgerTargeting {
                ledger_name: cli.ledger,
                ledger_path: query_path.or(cli.ledger_path),
            };
            let code = query::cmd_query(
                &cli.config_dir,
                sql.as_deref(),
                &query_targeting,
                glob.as_deref(),
                event_type.as_deref(),
                &fields,
                count,
                &effective_format,
            );
            Box::new(LegacyResult::new("query", code))
        }
        Commands::Daemon { action } => match action {
            DaemonAction::Start => {
                let code = daemon_cmd::cmd_daemon_start(&cli.config_dir);
                Box::new(LegacyResult::new("daemon_start", code))
            }
            DaemonAction::Stop => {
                let code = daemon_cmd::cmd_daemon_stop(&cli.config_dir);
                Box::new(LegacyResult::new("daemon_stop", code))
            }
            DaemonAction::Status => {
                let code = daemon_cmd::cmd_daemon_status(&cli.config_dir);
                Box::new(LegacyResult::new("daemon_status", code))
            }
        },
        Commands::Sign { event_type, fields } => {
            let code = sign_cmd::cmd_sign(&cli.config_dir, &event_type, &fields);
            Box::new(LegacyResult::new("sign", code))
        }
        Commands::Vault { action } => match action {
            VaultAction::Store { name, file } => {
                let code = vault_cmd::cmd_vault_store(&cli.config_dir, &name, &file);
                Box::new(LegacyResult::new("vault_store", code))
            }
            VaultAction::Read { name } => {
                let code = vault_cmd::cmd_vault_read(&cli.config_dir, &name);
                Box::new(LegacyResult::new("vault_read", code))
            }
            VaultAction::Delete { name } => {
                let code = vault_cmd::cmd_vault_delete(&cli.config_dir, &name);
                Box::new(LegacyResult::new("vault_delete", code))
            }
            VaultAction::List => {
                let code = vault_cmd::cmd_vault_list(&cli.config_dir);
                Box::new(LegacyResult::new("vault_list", code))
            }
        },
        Commands::Verify {
            event_type,
            fields,
            proof,
        } => {
            let code = verify_cmd::cmd_verify(&cli.config_dir, &event_type, &fields, &proof);
            Box::new(LegacyResult::new("verify", code))
        }
    };

    if use_json_envelope {
        println!("{}", result.to_json());
    } else {
        let text = result.to_text();
        if !text.is_empty() {
            if result.exit_code() == 0 {
                print!("{}", text);
            } else {
                eprint!("{}", text);
            }
        }
    }
    std::process::exit(result.exit_code());
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
