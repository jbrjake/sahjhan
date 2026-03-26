// src/main.rs
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "sahjhan", version, about = "Protocol enforcement engine for AI agents")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Path to protocol config directory
    #[arg(long, default_value = "enforcement")]
    config_dir: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize ledger, manifest, genesis block
    Init,
    /// Show current state and gate status
    Status,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Commands::Init => println!("init not yet implemented"),
        Commands::Status => println!("status not yet implemented"),
    }
}
