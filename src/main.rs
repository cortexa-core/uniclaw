mod agent;
mod channels;
mod commands;
mod config;
mod llm;
mod mcp;
mod robot;
mod server;
mod tools;
mod utils;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "uniclaw",
    version,
    about = "Privacy-first AI agent for ARM Linux SBCs"
)]
struct Cli {
    /// Path to config file
    #[arg(long, default_value = "config/config.toml")]
    config: PathBuf,

    /// Path to data directory
    #[arg(long, default_value = "data")]
    data_dir: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize data directories and default config
    Init,
    /// Start an interactive chat session
    Chat {
        /// Single message (non-interactive mode)
        #[arg(long, short)]
        message: Option<String>,
        /// Session ID (default: "cli")
        #[arg(long, default_value = "cli")]
        session: String,
    },
    /// Start the server (HTTP API + MQTT + cron + heartbeat)
    Serve,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Init => commands::run_init(&cli.config, &cli.data_dir),
        Commands::Chat { message, session } => {
            commands::chat::run(&cli.config, &cli.data_dir, message, &session).await
        }
        Commands::Serve => commands::serve::run(&cli.config, &cli.data_dir).await,
    }
}
