use async_trait::async_trait;
use serde_json::json;

use super::registry::{Tool, ToolContext, ToolResult};

// --- MemoryStoreTool ---

pub struct MemoryStoreTool;

#[async_trait]
impl Tool for MemoryStoreTool {
    fn name(&self) -> &str { "memory_store" }

    fn description(&self) -> &str {
        "Store a fact or piece of information in long-term memory. Use this when the user \
         tells you something worth remembering across conversations."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["key", "value"],
            "properties": {
                "key": {
                    "type": "string",
                    "description": "A short label for the fact (e.g., 'user_name', 'preference_temp_unit')"
                },
                "value": {
                    "type": "string",
                    "description": "The fact to remember"
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let key = match args["key"].as_str() {
            Some(k) => k,
            None => return ToolResult::Error("Missing required parameter: key".into()),
        };
        let value = match args["value"].as_str() {
            Some(v) => v,
            None => return ToolResult::Error("Missing required parameter: value".into()),
        };

        let path = ctx.data_dir.join("memory/MEMORY.md");
        if let Some(parent) = path.parent() {
            if let Err(e) = tokio::fs::create_dir_all(parent).await {
                return ToolResult::Error(format!("Failed to create memory directory: {e}"));
            }
        }
        let mut content = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M");
        content.push_str(&format!("\n- [{timestamp}] {key}: {value}"));

        match tokio::fs::write(&path, &content).await {
            Ok(_) => ToolResult::Success(format!("Stored in memory: {key} = {value}")),
            Err(e) => ToolResult::Error(format!("Failed to write memory: {e}")),
        }
    }
}

// --- MemoryReadTool ---

pub struct MemoryReadTool;

#[async_trait]
impl Tool for MemoryReadTool {
    fn name(&self) -> &str { "memory_read" }

    fn description(&self) -> &str {
        "Read long-term memory contents. Optionally search for a specific key."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Optional key to search for. If omitted, returns all memory."
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let path = ctx.data_dir.join("memory/MEMORY.md");
        let content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(_) => return ToolResult::Success("Memory is empty.".into()),
        };

        if content.trim().is_empty() {
            return ToolResult::Success("Memory is empty.".into());
        }

        if let Some(key) = args["key"].as_str() {
            // Filter lines containing the key
            let matches: Vec<&str> = content
                .lines()
                .filter(|line| line.to_lowercase().contains(&key.to_lowercase()))
                .collect();
            if matches.is_empty() {
                ToolResult::Success(format!("No memory found for key: {key}"))
            } else {
                ToolResult::Success(matches.join("\n"))
            }
        } else {
            ToolResult::Success(content)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::Arc;

    fn test_ctx(dir: &Path) -> ToolContext {
        std::fs::create_dir_all(dir.join("memory")).unwrap();
        ToolContext {
            data_dir: dir.to_path_buf(),
            session_id: "test".into(),
            config: Arc::new(
                toml::from_str("[agent]\n[llm]\nprovider=\"test\"\nmodel=\"test\"").unwrap(),
            ),
        }
    }

    #[tokio::test]
    async fn test_memory_store_and_read() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());

        // Store
        let result = MemoryStoreTool
            .execute(json!({"key": "name", "value": "Jiekai"}), &ctx)
            .await;
        assert!(!result.is_error());
        assert!(result.content().contains("Jiekai"));

        // Read all
        let result = MemoryReadTool.execute(json!({}), &ctx).await;
        assert!(!result.is_error());
        assert!(result.content().contains("name: Jiekai"));

        // Read with key filter
        let result = MemoryReadTool
            .execute(json!({"key": "name"}), &ctx)
            .await;
        assert!(result.content().contains("Jiekai"));

        // Read with nonexistent key
        let result = MemoryReadTool
            .execute(json!({"key": "nonexistent"}), &ctx)
            .await;
        assert!(result.content().contains("No memory found"));
    }

    #[tokio::test]
    async fn test_memory_read_empty() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());

        let result = MemoryReadTool.execute(json!({}), &ctx).await;
        assert!(result.content().contains("empty"));
    }
}
