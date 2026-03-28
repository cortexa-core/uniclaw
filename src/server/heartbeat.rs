use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

use crate::agent::{Input, Output};

/// Find the largest byte index <= `max` that lies on a UTF-8 character boundary.
fn floor_char_boundary(s: &str, max: usize) -> usize {
    if max >= s.len() {
        return s.len();
    }
    let mut i = max;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

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
        let content = match tokio::fs::read_to_string(&heartbeat_path).await {
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
                    let end = floor_char_boundary(&output.content, 200);
                    tracing::info!(
                        "Heartbeat response: {}",
                        &output.content[..end]
                    );
                }
                Ok(Err(_)) => tracing::warn!("Heartbeat: agent worker dropped request"),
                Err(_) => tracing::warn!("Heartbeat: timed out (120s)"),
            }
        });
    }
}
