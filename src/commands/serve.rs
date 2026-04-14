use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

use crate::channels;
use crate::config::Config;
use crate::server;

use super::{create_agent, setup_logging, spawn_agent_worker};

pub async fn run(config_path: &Path, data_dir: &Path) -> Result<()> {
    setup_logging();
    let config = Config::load(config_path)?;
    let agent = create_agent(&config, data_dir).await?;
    let inbound_tx = spawn_agent_worker(agent);

    tracing::info!(
        "UniClaw v{} starting server mode",
        env!("CARGO_PKG_VERSION")
    );

    let mut tasks = Vec::new();

    // HTTP server
    if config
        .server
        .as_ref()
        .map(|s| s.http_enabled)
        .unwrap_or(true)
    {
        let api_token = config
            .server
            .as_ref()
            .and_then(|s| {
                if s.api_token_env.is_empty() {
                    None
                } else {
                    std::env::var(&s.api_token_env).ok()
                }
            })
            .unwrap_or_default();

        if api_token.is_empty() {
            tracing::warn!("HTTP API has no authentication configured. Set [server] api_token_env to secure it.");
        }

        let http_state = Arc::new(server::http::HttpState {
            inbound_tx: inbound_tx.clone(),
            version: env!("CARGO_PKG_VERSION").into(),
            model: config.llm.model.clone(),
            start_time: std::time::Instant::now(),
            config_path: config_path.to_path_buf(),
            data_dir: data_dir.to_path_buf(),
            api_token,
            rate_limiter: Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
            rate_limit_per_minute: config
                .server
                .as_ref()
                .map(|s| s.rate_limit_per_minute)
                .unwrap_or(60),
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
    if config
        .server
        .as_ref()
        .map(|s| s.mqtt_enabled)
        .unwrap_or(false)
    {
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
        let cron_interval = config
            .cron
            .as_ref()
            .map(|c| c.check_interval_secs)
            .unwrap_or(60);
        let cron_dir = data_dir.to_path_buf();
        let cron_tx = inbound_tx.clone();
        tracing::info!("Cron scheduler enabled (check every {cron_interval}s)");
        tasks.push(tokio::spawn(async move {
            server::cron::cron_task(cron_dir, cron_tx, cron_interval).await;
        }));
    }

    // Heartbeat service
    if config
        .heartbeat
        .as_ref()
        .map(|h| h.enabled)
        .unwrap_or(false)
    {
        let hb_interval = config
            .heartbeat
            .as_ref()
            .map(|h| h.interval_secs)
            .unwrap_or(1800);
        let hb_dir = data_dir.to_path_buf();
        let hb_tx = inbound_tx.clone();
        tracing::info!("Heartbeat service enabled (every {hb_interval}s)");
        tasks.push(tokio::spawn(async move {
            server::heartbeat::heartbeat_task(hb_dir, hb_tx, hb_interval).await;
        }));
    }

    // Messaging channels (Telegram, Discord, etc.)
    channels::spawn_channels(&config, inbound_tx.clone(), &mut tasks);

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
