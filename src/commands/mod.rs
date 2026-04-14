pub mod chat;
pub mod serve;

use anyhow::Result;
use std::path::{Path, PathBuf};
use tokio::sync::{mpsc, oneshot};

use crate::agent::{Agent, Input, Output};
use crate::config::Config;
use crate::llm;
use crate::mcp;
use crate::tools;

pub fn setup_logging() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_target(false)
        .init();
}

pub async fn create_agent(config: &Config, data_dir: &Path) -> Result<Agent> {
    std::fs::create_dir_all(data_dir.join("memory"))?;
    std::fs::create_dir_all(data_dir.join("sessions"))?;
    std::fs::create_dir_all(data_dir.join("skills"))?;

    let primary = llm::create_provider(&config.llm)?;

    let mut fallbacks: Vec<Box<dyn llm::LlmProvider>> = Vec::new();
    if let Some(ref fallback_config) = config.llm.fallback {
        fallbacks.push(llm::create_provider(fallback_config)?);
    }

    let reliable: Box<dyn llm::LlmProvider> = Box::new(llm::reliable::ReliableProvider::new(
        primary,
        fallbacks,
        config.llm.max_retries,
        config.llm.base_backoff_ms,
    ));

    let llm: Box<dyn llm::LlmProvider> = if !config.routes.is_empty() {
        let mut providers: std::collections::HashMap<String, Box<dyn llm::LlmProvider>> =
            std::collections::HashMap::new();
        providers.insert("default".to_string(), reliable);

        for named in &config.extra_providers {
            let p = llm::create_provider(&named.to_llm_config())?;
            let wrapped = Box::new(llm::reliable::ReliableProvider::new(
                p,
                vec![],
                config.llm.max_retries,
                config.llm.base_backoff_ms,
            ));
            providers.insert(named.name.clone(), wrapped);
        }

        let mut routes = std::collections::HashMap::new();
        for route in &config.routes {
            if !providers.contains_key(&route.use_provider) && route.use_provider != "default" {
                tracing::warn!(
                    "Route '{}' references unknown provider '{}', skipping",
                    route.hint,
                    route.use_provider
                );
                continue;
            }
            routes.insert(
                route.hint.clone(),
                (route.use_provider.clone(), String::new()),
            );
        }

        Box::new(llm::router::RouterProvider::new(
            providers,
            routes,
            "default".to_string(),
        )?)
    } else {
        reliable
    };

    let mut tool_registry = tools::registry::ToolRegistry::new();
    tools::register_default_tools(&mut tool_registry);

    // Connect to MCP servers and register their tools
    if !config.mcp_servers.is_empty() {
        let _clients = mcp::register_mcp_tools(&config.mcp_servers, &mut tool_registry).await;
        // Note: clients are kept alive by the Arc<McpClient> inside each McpTool
    }

    let mut agent = Agent::new(llm, tool_registry, config, data_dir.to_path_buf()).await;

    // Run session GC at startup — remove expired and excess session files
    if let Err(e) = agent.cleanup_sessions().await {
        tracing::warn!("Session cleanup failed: {e}");
    }

    Ok(agent)
}

/// Spawn the agent worker task. Returns the inbound sender.
/// The agent worker is the sole owner of Agent — processes one request at a time.
pub fn spawn_agent_worker(mut agent: Agent) -> mpsc::Sender<(Input, oneshot::Sender<Output>)> {
    let (inbound_tx, mut inbound_rx) = mpsc::channel::<(Input, oneshot::Sender<Output>)>(32);

    tokio::spawn(async move {
        while let Some((input, reply_tx)) = inbound_rx.recv().await {
            let session_id = input.session_id.clone();
            let start = std::time::Instant::now();

            tracing::info!(session = %session_id, "Processing request");

            let result = agent.process(&input).await;
            let elapsed_ms = start.elapsed().as_millis() as u64;

            match result {
                Ok(output) => {
                    let (tokens_in, tokens_out) = output
                        .usage
                        .as_ref()
                        .map(|u| (u.input_tokens, u.output_tokens))
                        .unwrap_or((0, 0));
                    tracing::info!(
                        session = %session_id,
                        elapsed_ms,
                        tokens_in,
                        tokens_out,
                        "Request completed"
                    );
                    reply_tx.send(output).ok();
                }
                Err(e) => {
                    tracing::error!(
                        session = %session_id,
                        elapsed_ms,
                        error = %e,
                        "Request failed"
                    );
                    reply_tx.send(Output::text(format!("Error: {e}"))).ok();
                }
            }
        }
        // Channel closed — persist sessions before exit
        if let Err(e) = agent.session_store.persist_all().await {
            tracing::error!("Failed to persist sessions on shutdown: {e}");
        }
    });

    inbound_tx
}

pub async fn send_and_wait(
    tx: &mpsc::Sender<(Input, oneshot::Sender<Output>)>,
    message: &str,
    session_id: &str,
) -> Result<Output> {
    let input = Input {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: session_id.to_string(),
        content: message.to_string(),
        stream_tx: None,
    };
    let (reply_tx, reply_rx) = oneshot::channel();
    tx.send((input, reply_tx))
        .await
        .map_err(|_| anyhow::anyhow!("Agent worker unavailable"))?;
    reply_rx
        .await
        .map_err(|_| anyhow::anyhow!("Agent worker dropped request"))
}

pub fn atty_check() -> bool {
    use std::io::IsTerminal;
    std::io::stdin().is_terminal()
}

pub fn run_init(config_path: &Path, data_dir: &Path) -> Result<()> {
    println!("Initializing UniClaw...");

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
        std::fs::write(&soul_path, crate::agent::context::DEFAULT_SOUL)?;
        println!("  Written {}", soul_path.display());
    }

    if !config_path.exists() {
        let default_config = include_str!("../../config/default_config.toml");
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
    println!("  ./uniclaw chat    # interactive chat");
    println!("  ./uniclaw serve   # HTTP + MQTT server");
    Ok(())
}
