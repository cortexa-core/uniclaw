mod agent;
mod config;
mod llm;
mod server;
mod tools;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::io::{self, BufRead, Write};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

use agent::{Agent, Input, Output};
use config::Config;

#[derive(Parser)]
#[command(name = "miniclaw", version, about = "Privacy-first AI agent for ARM Linux SBCs")]
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
        Commands::Init => run_init(&cli.config, &cli.data_dir),
        Commands::Chat { message, session } => {
            run_chat(&cli.config, &cli.data_dir, message, &session).await
        }
        Commands::Serve => run_serve(&cli.config, &cli.data_dir).await,
    }
}

fn run_init(config_path: &PathBuf, data_dir: &PathBuf) -> Result<()> {
    println!("Initializing MiniClaw...");

    let dirs = [
        data_dir.to_path_buf(),
        data_dir.join("memory"),
        data_dir.join("sessions"),
        data_dir.join("skills"),
        PathBuf::from("config"),
        PathBuf::from("logs"),
    ];
    for dir in &dirs {
        std::fs::create_dir_all(dir)?;
        println!("  Created {}/", dir.display());
    }

    let soul_path = data_dir.join("SOUL.md");
    if !soul_path.exists() {
        std::fs::write(&soul_path, agent::context::DEFAULT_SOUL)?;
        println!("  Written {}", soul_path.display());
    }

    if !config_path.exists() {
        let default_config = include_str!("../config/default_config.toml");
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(config_path, default_config)?;
        println!("  Written {}", config_path.display());
    }

    let memory_path = data_dir.join("memory/MEMORY.md");
    if !memory_path.exists() {
        std::fs::write(&memory_path, "")?;
    }

    println!("\nPlease set your API key:");
    println!("  export ANTHROPIC_API_KEY=\"your-key-here\"");
    println!("\nThen run:");
    println!("  ./miniclaw chat    # interactive chat");
    println!("  ./miniclaw serve   # HTTP + MQTT server");
    Ok(())
}

fn setup_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();
}

fn create_agent(config: &Config, data_dir: &PathBuf) -> Result<Agent> {
    std::fs::create_dir_all(data_dir.join("memory"))?;
    std::fs::create_dir_all(data_dir.join("sessions"))?;
    std::fs::create_dir_all(data_dir.join("skills"))?;

    let primary = llm::create_provider(&config.llm)?;
    let fallback = config
        .llm
        .fallback
        .as_ref()
        .and_then(|f| llm::create_provider(f).ok());

    let mut tool_registry = tools::registry::ToolRegistry::new();
    tools::register_default_tools(&mut tool_registry);

    Ok(Agent::new(primary, fallback, tool_registry, config, data_dir.clone()))
}

/// Spawn the agent worker task. Returns the inbound sender.
/// The agent worker is the sole owner of Agent — processes one request at a time.
fn spawn_agent_worker(
    mut agent: Agent,
) -> mpsc::Sender<(Input, oneshot::Sender<Output>)> {
    let (inbound_tx, mut inbound_rx) = mpsc::channel::<(Input, oneshot::Sender<Output>)>(32);

    tokio::spawn(async move {
        while let Some((input, reply_tx)) = inbound_rx.recv().await {
            let result = agent.process(&input).await;
            match result {
                Ok(output) => {
                    reply_tx.send(output).ok();
                }
                Err(e) => {
                    tracing::error!("Agent error: {e}");
                    reply_tx
                        .send(Output::text(format!("Error: {e}")))
                        .ok();
                }
            }
        }
        // Channel closed — persist sessions before exit
        if let Err(e) = agent.session_store.persist_all() {
            tracing::error!("Failed to persist sessions on shutdown: {e}");
        }
    });

    inbound_tx
}

// --- Chat command ---

async fn run_chat(
    config_path: &PathBuf,
    data_dir: &PathBuf,
    message: Option<String>,
    session_id: &str,
) -> Result<()> {
    setup_logging();
    let config = Config::load(config_path)?;
    let agent = create_agent(&config, data_dir)?;
    let inbound_tx = spawn_agent_worker(agent);

    // Single-shot mode
    if let Some(msg) = message {
        let output = send_and_wait(&inbound_tx, &msg, session_id).await?;
        println!("{}", output.content);
        return Ok(());
    }

    // REPL mode
    let is_tty = atty_check();
    if is_tty {
        println!(
            "MiniClaw v{} | {} | {}",
            env!("CARGO_PKG_VERSION"),
            config.llm.model,
            std::env::consts::ARCH
        );
        println!("Type 'exit' or Ctrl+C to quit.\n");
    }

    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut line = String::new();

    loop {
        if is_tty {
            print!("You> ");
            io::stdout().flush()?;
        }

        line.clear();
        if reader.read_line(&mut line)? == 0 {
            break;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed == "exit" || trimmed == "quit" {
            if is_tty {
                println!("Goodbye!");
            }
            break;
        }

        match send_and_wait(&inbound_tx, trimmed, session_id).await {
            Ok(output) => {
                if is_tty {
                    println!("MiniClaw> {}\n", output.content);
                } else {
                    println!("{}", output.content);
                }
            }
            Err(e) => eprintln!("Error: {e}"),
        }
    }

    Ok(())
}

// --- Serve command ---

async fn run_serve(config_path: &PathBuf, data_dir: &PathBuf) -> Result<()> {
    setup_logging();
    let config = Config::load(config_path)?;
    let agent = create_agent(&config, data_dir)?;
    let inbound_tx = spawn_agent_worker(agent);

    tracing::info!("MiniClaw v{} starting server mode", env!("CARGO_PKG_VERSION"));

    let mut tasks = Vec::new();

    // HTTP server
    if config.server.as_ref().map(|s| s.http_enabled).unwrap_or(true) {
        let http_state = Arc::new(server::http::HttpState {
            inbound_tx: inbound_tx.clone(),
            version: env!("CARGO_PKG_VERSION").into(),
            model: config.llm.model.clone(),
            start_time: std::time::Instant::now(),
        });

        let port = config.server.as_ref().map(|s| s.http_port).unwrap_or(3000);
        let bind = config
            .server
            .as_ref()
            .map(|s| s.http_bind.clone())
            .unwrap_or_else(|| "0.0.0.0".into());

        let addr = format!("{bind}:{port}");
        let router = server::http::router(http_state);
        let listener = tokio::net::TcpListener::bind(&addr).await?;
        tracing::info!("HTTP server listening on {addr}");

        tasks.push(tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, router).await {
                tracing::error!("HTTP server error: {e}");
            }
        }));
    }

    // MQTT client
    if config.server.as_ref().map(|s| s.mqtt_enabled).unwrap_or(false) {
        let mqtt_config = config.clone();
        let mqtt_tx = inbound_tx.clone();
        tasks.push(tokio::spawn(async move {
            if let Err(e) = server::mqtt::mqtt_task(&mqtt_config, mqtt_tx).await {
                tracing::error!("MQTT task error: {e}");
            }
        }));
    }

    // Cron scheduler
    if config.cron.as_ref().map(|c| c.enabled).unwrap_or(false) {
        let cron_interval = config.cron.as_ref().map(|c| c.check_interval_secs).unwrap_or(60);
        let cron_dir = data_dir.clone();
        let cron_tx = inbound_tx.clone();
        tracing::info!("Cron scheduler enabled (check every {cron_interval}s)");
        tasks.push(tokio::spawn(async move {
            server::cron::cron_task(cron_dir, cron_tx, cron_interval).await;
        }));
    }

    // Heartbeat service
    if config.heartbeat.as_ref().map(|h| h.enabled).unwrap_or(false) {
        let hb_interval = config.heartbeat.as_ref().map(|h| h.interval_secs).unwrap_or(1800);
        let hb_dir = data_dir.clone();
        let hb_tx = inbound_tx.clone();
        tracing::info!("Heartbeat service enabled (every {hb_interval}s)");
        tasks.push(tokio::spawn(async move {
            server::heartbeat::heartbeat_task(hb_dir, hb_tx, hb_interval).await;
        }));
    }

    if tasks.is_empty() {
        tracing::warn!("No server tasks enabled. Add [server] section to config.");
        return Ok(());
    }

    // Wait for shutdown signal
    tracing::info!("Server running. Press Ctrl+C to stop.");
    tokio::signal::ctrl_c().await?;
    tracing::info!("Shutting down...");

    // Tasks will be dropped — agent_worker persists sessions when channel closes
    Ok(())
}

// --- Helpers ---

async fn send_and_wait(
    tx: &mpsc::Sender<(Input, oneshot::Sender<Output>)>,
    message: &str,
    session_id: &str,
) -> Result<Output> {
    let input = Input {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: session_id.to_string(),
        content: message.to_string(),
    };
    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send((input, reply_tx))
        .await
        .map_err(|_| anyhow::anyhow!("Agent worker unavailable"))?;
    reply_rx
        .await
        .map_err(|_| anyhow::anyhow!("Agent worker dropped request"))
}

fn atty_check() -> bool {
    use std::os::unix::io::AsRawFd;
    unsafe { libc_isatty(io::stdin().as_raw_fd()) != 0 }
}

extern "C" {
    fn isatty(fd: i32) -> i32;
}

unsafe fn libc_isatty(fd: i32) -> i32 {
    unsafe { isatty(fd) }
}
