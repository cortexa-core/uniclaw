use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

use crate::agent::{Input, Output};

pub async fn heartbeat_task(
    data_dir: PathBuf,
    inbound_tx: mpsc::Sender<(Input, oneshot::Sender<Output>)>,
    interval_secs: u64,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
    // Skip the first immediate tick
    interval.tick().await;

    loop {
        interval.tick().await;

        let heartbeat_path = data_dir.join("HEARTBEAT.md");
        let content = match std::fs::read_to_string(&heartbeat_path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if content.trim().is_empty() {
            continue;
        }

        // Check for uncompleted items
        let has_pending = content.lines().any(|l| {
            let trimmed = l.trim();
            trimmed.starts_with("- [ ]") || trimmed.starts_with("- []")
        });

        if !has_pending {
            continue;
        }

        tracing::info!("Heartbeat: found pending tasks in HEARTBEAT.md");

        let input = Input {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: "heartbeat".into(),
            content: format!(
                "Check your HEARTBEAT.md for pending tasks and handle them. \
                 Mark completed tasks with [x].\n\n{content}"
            ),
        };

        let (reply_tx, reply_rx) = oneshot::channel();

        if inbound_tx.send((input, reply_tx)).await.is_err() {
            tracing::error!("Agent worker channel closed, stopping heartbeat");
            return;
        }

        // Don't block — fire and forget
        tokio::spawn(async move {
            match tokio::time::timeout(Duration::from_secs(120), reply_rx).await {
                Ok(Ok(output)) => {
                    tracing::info!(
                        "Heartbeat response: {}",
                        &output.content[..output.content.len().min(200)]
                    );
                }
                Ok(Err(_)) => tracing::warn!("Heartbeat: agent worker dropped request"),
                Err(_) => tracing::warn!("Heartbeat: timed out (120s)"),
            }
        });
    }
}
