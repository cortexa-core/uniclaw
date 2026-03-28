use async_trait::async_trait;
use serde_json::json;

use crate::server::cron::{self, CronJob, CronSchedule};
use super::registry::{Tool, ToolContext, ToolResult};

// --- CronAddTool ---

pub struct CronAddTool;

#[async_trait]
impl Tool for CronAddTool {
    fn name(&self) -> &str { "cron_add" }

    fn description(&self) -> &str {
        "Schedule a recurring task. The action is a natural language description \
         of what to do. The schedule is an interval in seconds."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["action", "interval_seconds"],
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Human-readable name for the job"
                },
                "action": {
                    "type": "string",
                    "description": "Natural language description of what to do"
                },
                "interval_seconds": {
                    "type": "integer",
                    "description": "How often to run, in seconds (e.g., 3600 for hourly)"
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let action = match args["action"].as_str() {
            Some(a) => a.to_string(),
            None => return ToolResult::Error("Missing required parameter: action".into()),
        };
        let interval = match args["interval_seconds"].as_u64() {
            Some(s) => s,
            None => return ToolResult::Error("Missing required parameter: interval_seconds".into()),
        };
        let name = args["name"]
            .as_str()
            .unwrap_or("Unnamed job")
            .to_string();

        let id = uuid::Uuid::new_v4().to_string()[..8].to_string();

        let job = CronJob {
            id: id.clone(),
            name: name.clone(),
            schedule: CronSchedule::Every { seconds: interval },
            action,
            last_run: None,
            enabled: true,
        };

        let mut jobs = cron::load_cron_jobs(&ctx.data_dir).await;

        // Max 16 jobs
        if jobs.len() >= 16 {
            return ToolResult::Error("Maximum of 16 cron jobs reached. Remove one first.".into());
        }

        jobs.push(job);

        match cron::save_cron_jobs(&ctx.data_dir, &jobs).await {
            Ok(_) => ToolResult::Success(format!(
                "Created cron job '{name}' (id={id}), runs every {interval}s"
            )),
            Err(e) => ToolResult::Error(format!("Failed to save cron job: {e}")),
        }
    }
}

// --- CronListTool ---

pub struct CronListTool;

#[async_trait]
impl Tool for CronListTool {
    fn name(&self) -> &str { "cron_list" }

    fn description(&self) -> &str {
        "List all scheduled cron jobs."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({"type": "object", "properties": {}})
    }

    async fn execute(&self, _args: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let jobs = cron::load_cron_jobs(&ctx.data_dir).await;

        if jobs.is_empty() {
            return ToolResult::Success("No cron jobs scheduled.".into());
        }

        let lines: Vec<String> = jobs
            .iter()
            .map(|j| {
                let schedule_desc = match &j.schedule {
                    CronSchedule::Every { seconds } => format!("every {seconds}s"),
                    CronSchedule::Once { at } => format!("once at {at}"),
                };
                let status = if j.enabled { "enabled" } else { "disabled" };
                format!(
                    "- [{}] {} (id={}) — {} — {}\n  Action: {}",
                    status, j.name, j.id, schedule_desc,
                    j.last_run
                        .map(|t| format!("last run: {}", t.format("%Y-%m-%d %H:%M")))
                        .unwrap_or_else(|| "never run".into()),
                    j.action
                )
            })
            .collect();

        ToolResult::Success(format!("{} cron jobs:\n{}", jobs.len(), lines.join("\n")))
    }
}

// --- CronRemoveTool ---

pub struct CronRemoveTool;

#[async_trait]
impl Tool for CronRemoveTool {
    fn name(&self) -> &str { "cron_remove" }

    fn description(&self) -> &str {
        "Remove a scheduled cron job by its ID."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["id"],
            "properties": {
                "id": {
                    "type": "string",
                    "description": "The ID of the cron job to remove"
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let id = match args["id"].as_str() {
            Some(i) => i,
            None => return ToolResult::Error("Missing required parameter: id".into()),
        };

        let mut jobs = cron::load_cron_jobs(&ctx.data_dir).await;
        let before = jobs.len();
        jobs.retain(|j| j.id != id);

        if jobs.len() == before {
            return ToolResult::Error(format!("No cron job found with id: {id}"));
        }

        match cron::save_cron_jobs(&ctx.data_dir, &jobs).await {
            Ok(_) => ToolResult::Success(format!("Removed cron job {id}")),
            Err(e) => ToolResult::Error(format!("Failed to save: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;
    use std::sync::Arc;

    fn test_ctx(dir: &Path) -> ToolContext {
        ToolContext {
            data_dir: dir.to_path_buf(),
            session_id: "test".into(),
            config: Arc::new(
                toml::from_str("[agent]\n[llm]\nprovider=\"test\"\nmodel=\"test\"").unwrap(),
            ),
        }
    }

    #[tokio::test]
    async fn test_cron_add_list_remove_cycle() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());

        // Add a job
        let result = CronAddTool
            .execute(
                json!({"name": "Test Job", "action": "check status", "interval_seconds": 3600}),
                &ctx,
            )
            .await;
        assert!(!result.is_error());
        assert!(result.content().contains("Test Job"));

        // List jobs — should have 1
        let result = CronListTool.execute(json!({}), &ctx).await;
        assert!(!result.is_error());
        assert!(result.content().contains("1 cron jobs"));
        assert!(result.content().contains("check status"));

        // Extract the job ID from the add result
        let jobs = cron::load_cron_jobs(&dir.path().to_path_buf()).await;
        assert_eq!(jobs.len(), 1);
        let job_id = jobs[0].id.clone();

        // Remove the job
        let result = CronRemoveTool
            .execute(json!({"id": job_id}), &ctx)
            .await;
        assert!(!result.is_error());

        // List should be empty now
        let result = CronListTool.execute(json!({}), &ctx).await;
        assert!(result.content().contains("No cron jobs"));
    }

    #[tokio::test]
    async fn test_cron_remove_nonexistent() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());

        let result = CronRemoveTool
            .execute(json!({"id": "nonexistent"}), &ctx)
            .await;
        assert!(result.is_error());
        assert!(result.content().contains("No cron job found"));
    }
}
