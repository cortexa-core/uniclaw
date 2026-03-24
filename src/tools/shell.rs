use async_trait::async_trait;
use serde_json::json;
use std::collections::HashSet;
use std::time::Duration;

use super::registry::{Tool, ToolContext, ToolResult};

pub struct ShellExecTool;

#[async_trait]
impl Tool for ShellExecTool {
    fn name(&self) -> &str { "shell_exec" }

    fn description(&self) -> &str {
        "Execute a shell command on the device. Commands are sandboxed: \
         only whitelisted commands are allowed, with a timeout and working directory restriction."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["command"],
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to execute (e.g., 'df -h', 'uptime', 'ls data/')"
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let command = match args["command"].as_str() {
            Some(c) => c,
            None => return ToolResult::Error("Missing required parameter: command".into()),
        };

        // Parse allowed commands from config
        let allowed: HashSet<String> = ctx
            .config
            .tools
            .shell_allowed_commands
            .iter()
            .cloned()
            .collect();

        // Extract the program name (first word)
        let program = command.split_whitespace().next().unwrap_or("");

        if !allowed.is_empty() && !allowed.contains(program) {
            return ToolResult::Error(format!(
                "Command '{program}' is not in the allowed list. Allowed: {}",
                ctx.config.tools.shell_allowed_commands.join(", ")
            ));
        }

        let timeout = Duration::from_secs(ctx.config.tools.shell_timeout_secs);

        let result = tokio::time::timeout(
            timeout,
            tokio::process::Command::new("sh")
                .args(["-c", command])
                .current_dir(&ctx.data_dir)
                .output(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                if output.status.success() {
                    let mut result = stdout.to_string();
                    if !stderr.is_empty() {
                        result.push_str(&format!("\nstderr: {stderr}"));
                    }
                    // Truncate long output
                    if result.len() > 4096 {
                        result.truncate(4096);
                        result.push_str("\n... (output truncated at 4096 chars)");
                    }
                    ToolResult::Success(result)
                } else {
                    ToolResult::Error(format!(
                        "Command exited with code {}\nstdout: {stdout}\nstderr: {stderr}",
                        output.status
                    ))
                }
            }
            Ok(Err(e)) => ToolResult::Error(format!("Failed to execute command: {e}")),
            Err(_) => ToolResult::Error(format!("Command timed out after {}s", timeout.as_secs())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::Arc;
    use crate::config::Config;

    fn test_ctx(dir: &std::path::Path) -> ToolContext {
        let config: Config = toml::from_str(
            r#"
[agent]
[llm]
provider = "test"
model = "test"
[tools]
shell_allowed_commands = ["echo", "date", "ls"]
shell_timeout_secs = 5
"#,
        )
        .unwrap();
        ToolContext {
            data_dir: dir.to_path_buf(),
            session_id: "test".into(),
            config: Arc::new(config),
        }
    }

    #[tokio::test]
    async fn test_shell_exec_allowed() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());
        let result = ShellExecTool
            .execute(json!({"command": "echo hello"}), &ctx)
            .await;
        assert!(!result.is_error());
        assert!(result.content().contains("hello"));
    }

    #[tokio::test]
    async fn test_shell_exec_blocked() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());
        let result = ShellExecTool
            .execute(json!({"command": "rm -rf /"}), &ctx)
            .await;
        assert!(result.is_error());
        assert!(result.content().contains("not in the allowed list"));
    }
}
