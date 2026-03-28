use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

use crate::agent::{Input, Output};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub name: String,
    pub schedule: CronSchedule,
    pub action: String,
    pub last_run: Option<DateTime<Utc>>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool { true }

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CronSchedule {
    Every { seconds: u64 },
    Once { at: DateTime<Utc> },
}

impl CronJob {
    fn is_due(&self, now: DateTime<Utc>) -> bool {
        if !self.enabled {
            return false;
        }
        match &self.schedule {
            CronSchedule::Every { seconds } => {
                match self.last_run {
                    None => true,
                    Some(last) => {
                        let elapsed = (now - last).num_seconds();
                        elapsed >= *seconds as i64
                    }
                }
            }
            CronSchedule::Once { at } => {
                self.last_run.is_none() && now >= *at
            }
        }
    }
}

pub async fn load_cron_jobs(data_dir: &PathBuf) -> Vec<CronJob> {
    let path = data_dir.join("cron.json");
    match tokio::fs::read_to_string(&path).await {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => Vec::new(),
    }
}

pub async fn save_cron_jobs(data_dir: &PathBuf, jobs: &[CronJob]) -> Result<()> {
    let path = data_dir.join("cron.json");
    let content = serde_json::to_string_pretty(jobs)?;
    tokio::fs::write(&path, content).await?;
    Ok(())
}

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

pub async fn cron_task(
    data_dir: PathBuf,
    inbound_tx: mpsc::Sender<(Input, oneshot::Sender<Output>)>,
    check_interval_secs: u64,
) {
    let mut interval = tokio::time::interval(Duration::from_secs(check_interval_secs));

    loop {
        interval.tick().await;

        let mut jobs = load_cron_jobs(&data_dir).await;
        if jobs.is_empty() {
            continue;
        }

        let now = Utc::now();
        let mut changed = false;

        for job in &mut jobs {
            if !job.is_due(now) {
                continue;
            }

            tracing::info!("Cron job '{}' (id={}) is due, executing", job.name, job.id);

            let input = Input {
                id: uuid::Uuid::new_v4().to_string(),
                session_id: format!("cron-{}", job.id),
                content: format!(
                    "Execute this scheduled task: {}\n\nTask name: {}",
                    job.action, job.name
                ),
            };

            let (reply_tx, reply_rx) = oneshot::channel();

            if inbound_tx.send((input, reply_tx)).await.is_err() {
                tracing::error!("Agent worker channel closed, stopping cron");
                return;
            }

            // Don't block the cron loop waiting for response — fire and forget
            tokio::spawn(async move {
                match tokio::time::timeout(Duration::from_secs(120), reply_rx).await {
                    Ok(Ok(output)) => {
                        let end = floor_char_boundary(&output.content, 200);
                        tracing::info!("Cron job response: {}", &output.content[..end]);
                    }
                    Ok(Err(_)) => tracing::warn!("Cron job: agent worker dropped request"),
                    Err(_) => tracing::warn!("Cron job timed out (120s)"),
                }
            });

            job.last_run = Some(now);
            changed = true;

            // Disable one-shot jobs after execution
            if matches!(job.schedule, CronSchedule::Once { .. }) {
                job.enabled = false;
            }
        }

        if changed {
            if let Err(e) = save_cron_jobs(&data_dir, &jobs).await {
                tracing::error!("Failed to save cron jobs: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cron_every_is_due() {
        let job = CronJob {
            id: "1".into(),
            name: "test".into(),
            schedule: CronSchedule::Every { seconds: 60 },
            action: "do something".into(),
            last_run: None,
            enabled: true,
        };
        assert!(job.is_due(Utc::now()));
    }

    #[test]
    fn test_cron_every_not_yet() {
        let job = CronJob {
            id: "1".into(),
            name: "test".into(),
            schedule: CronSchedule::Every { seconds: 3600 },
            action: "do something".into(),
            last_run: Some(Utc::now()),
            enabled: true,
        };
        assert!(!job.is_due(Utc::now()));
    }

    #[test]
    fn test_cron_disabled() {
        let job = CronJob {
            id: "1".into(),
            name: "test".into(),
            schedule: CronSchedule::Every { seconds: 1 },
            action: "do something".into(),
            last_run: None,
            enabled: false,
        };
        assert!(!job.is_due(Utc::now()));
    }

    #[test]
    fn test_cron_once_due() {
        let past = Utc::now() - chrono::Duration::hours(1);
        let job = CronJob {
            id: "1".into(),
            name: "test".into(),
            schedule: CronSchedule::Once { at: past },
            action: "do it".into(),
            last_run: None,
            enabled: true,
        };
        assert!(job.is_due(Utc::now()));
    }

    #[test]
    fn test_cron_once_already_ran() {
        let past = Utc::now() - chrono::Duration::hours(1);
        let job = CronJob {
            id: "1".into(),
            name: "test".into(),
            schedule: CronSchedule::Once { at: past },
            action: "do it".into(),
            last_run: Some(Utc::now()),
            enabled: true,
        };
        assert!(!job.is_due(Utc::now()));
    }

    #[tokio::test]
    async fn test_cron_save_load() {
        let dir = tempfile::tempdir().unwrap();
        let jobs = vec![CronJob {
            id: "test-1".into(),
            name: "Test Job".into(),
            schedule: CronSchedule::Every { seconds: 300 },
            action: "check status".into(),
            last_run: None,
            enabled: true,
        }];
        save_cron_jobs(&dir.path().to_path_buf(), &jobs).await.unwrap();
        let loaded = load_cron_jobs(&dir.path().to_path_buf()).await;
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].name, "Test Job");
    }
}
