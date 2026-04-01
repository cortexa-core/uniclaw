use async_trait::async_trait;
use serde_json::json;
use std::collections::HashSet;
use std::time::Duration;

use super::registry::{Tool, ToolContext, ToolResult};

pub struct ShellExecTool;

#[async_trait]
impl Tool for ShellExecTool {
    fn name(&self) -> &str {
        "shell_exec"
    }

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

        // Reject shell metacharacters that allow command injection.
        // Pipes (|) are allowed — they're useful for data flow (sort, head, grep).
        // Redirects (>, <) are blocked to prevent file overwrites.
        const DANGEROUS_CHARS: &[char] = &[
            ';', '&', '`', '$', '(', ')', '{', '}', '<', '>', '\n', '\r', '\0',
        ];
        if command.chars().any(|c| DANGEROUS_CHARS.contains(&c)) {
            return ToolResult::Error(
                "Command contains disallowed characters (;, &, `, $, etc.). \
                 Pipes (|) are allowed. Redirects (<, >) are not."
                    .into(),
            );
        }

        // Split on pipes to extract all programs for whitelist checking
        let segments: Vec<&str> = command.split('|').collect();
        let has_pipe = segments.len() > 1;

        // Check each pipeline segment's program against whitelist
        let allowed: HashSet<&str> = ctx
            .config
            .tools
            .shell_allowed_commands
            .iter()
            .map(|s| s.as_str())
            .collect();

        if allowed.is_empty() {
            return ToolResult::Error(
                "No commands are allowed: shell_allowed_commands is empty in config.".into(),
            );
        }

        let data_dir_str = ctx.data_dir.to_string_lossy();

        for segment in &segments {
            let parts: Vec<&str> = segment.split_whitespace().collect();
            let program = match parts.first() {
                Some(p) => *p,
                None => return ToolResult::Error("Empty command segment in pipeline".into()),
            };

            if !allowed.contains(program) {
                return ToolResult::Error(format!(
                    "Command '{program}' is not in the allowed list. Allowed: {}",
                    ctx.config.tools.shell_allowed_commands.join(", ")
                ));
            }

            // Reject absolute path arguments that escape the data directory
            for arg in &parts[1..] {
                if arg.starts_with('/') && !arg.starts_with(data_dir_str.as_ref()) {
                    return ToolResult::Error(format!(
                        "Argument '{arg}' references an absolute path outside the data directory. \
                         Use relative paths or paths within the data directory."
                    ));
                }
            }
        }

        let timeout_duration = Duration::from_secs(ctx.config.tools.shell_timeout_secs);

        // Use sh -c when pipes are present so the shell handles the pipeline.
        // For simple commands, execute directly to avoid shell interpretation.
        let result = if has_pipe {
            tokio::time::timeout(
                timeout_duration,
                tokio::process::Command::new("sh")
                    .args(["-c", command])
                    .current_dir(&ctx.data_dir)
                    .output(),
            )
            .await
        } else {
            let parts: Vec<&str> = command.split_whitespace().collect();
            let program = parts[0]; // already validated non-empty above
            let args = &parts[1..];
            tokio::time::timeout(
                timeout_duration,
                tokio::process::Command::new(program)
                    .args(args)
                    .current_dir(&ctx.data_dir)
                    .output(),
            )
            .await
        };

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                let stderr = String::from_utf8_lossy(&output.stderr);

                if output.status.success() {
                    let mut result = stdout.to_string();
                    if !stderr.is_empty() {
                        result.push_str(&format!("\nstderr: {stderr}"));
                    }
                    // Truncate long output (snap to UTF-8 char boundary)
                    if result.len() > 4096 {
                        let mut end = 4096;
                        while end > 0 && !result.is_char_boundary(end) {
                            end -= 1;
                        }
                        result.truncate(end);
                        result.push_str("\n... (output truncated)");
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
            Err(_) => ToolResult::Error(format!(
                "Command timed out after {}s",
                timeout_duration.as_secs()
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::sync::Arc;

    fn test_ctx(dir: &std::path::Path) -> ToolContext {
        let config: Config = toml::from_str(
            r#"
[agent]
[llm]
provider = "test"
model = "test"
[tools]
shell_allowed_commands = ["echo", "date", "ls", "cat"]
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

    #[tokio::test]
    async fn test_shell_exec_rejects_newline_injection() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());
        let result = ShellExecTool
            .execute(json!({"command": "echo hello\ncat /etc/passwd"}), &ctx)
            .await;
        assert!(result.is_error());
        assert!(result.content().contains("disallowed characters"));
    }

    #[tokio::test]
    async fn test_shell_exec_rejects_semicolon() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());
        let result = ShellExecTool
            .execute(json!({"command": "echo hello; cat /etc/passwd"}), &ctx)
            .await;
        assert!(result.is_error());
        assert!(result.content().contains("disallowed characters"));
    }

    #[tokio::test]
    async fn test_shell_exec_empty_whitelist_denies() {
        let dir = tempfile::tempdir().unwrap();
        let config: Config = toml::from_str(
            r#"
[agent]
[llm]
provider = "test"
model = "test"
[tools]
shell_allowed_commands = []
"#,
        )
        .unwrap();
        let ctx = ToolContext {
            data_dir: dir.path().to_path_buf(),
            session_id: "test".into(),
            config: Arc::new(config),
        };
        let result = ShellExecTool
            .execute(json!({"command": "echo hello"}), &ctx)
            .await;
        assert!(result.is_error());
        assert!(result.content().contains("shell_allowed_commands is empty"));
    }

    #[tokio::test]
    async fn test_shell_exec_rejects_absolute_path_args() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());
        let result = ShellExecTool
            .execute(json!({"command": "cat /etc/passwd"}), &ctx)
            .await;
        assert!(result.is_error());
        assert!(result.content().contains("absolute path outside"));
    }

    #[tokio::test]
    async fn test_shell_exec_allows_relative_path_args() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello").unwrap();
        let ctx = test_ctx(dir.path());
        let result = ShellExecTool
            .execute(json!({"command": "cat test.txt"}), &ctx)
            .await;
        assert!(!result.is_error());
        assert!(result.content().contains("hello"));
    }
}
