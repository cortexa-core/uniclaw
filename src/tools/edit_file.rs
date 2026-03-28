use async_trait::async_trait;
use serde_json::json;

use super::file_ops::validate_path;
use super::registry::{Tool, ToolContext, ToolResult};

pub struct EditFileTool;

#[async_trait]
impl Tool for EditFileTool {
    fn name(&self) -> &str { "edit_file" }

    fn description(&self) -> &str {
        "Edit a file by replacing a specific text string with new text. \
         The old_text must match exactly (including whitespace)."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["path", "old_text", "new_text"],
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to the data directory"
                },
                "old_text": {
                    "type": "string",
                    "description": "Exact text to find and replace"
                },
                "new_text": {
                    "type": "string",
                    "description": "Text to replace it with"
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let path = match args["path"].as_str() {
            Some(p) => p,
            None => return ToolResult::Error("Missing required parameter: path".into()),
        };
        let old_text = match args["old_text"].as_str() {
            Some(t) => t,
            None => return ToolResult::Error("Missing required parameter: old_text".into()),
        };
        let new_text = match args["new_text"].as_str() {
            Some(t) => t,
            None => return ToolResult::Error("Missing required parameter: new_text".into()),
        };

        let full_path = match validate_path(&ctx.data_dir, path) {
            Ok(p) => p,
            Err(e) => return ToolResult::Error(format!("Invalid path: {e}")),
        };

        let content = match tokio::fs::read_to_string(&full_path).await {
            Ok(c) => c,
            Err(e) => return ToolResult::Error(format!("Failed to read file: {e}")),
        };

        let count = content.matches(old_text).count();
        if count == 0 {
            return ToolResult::Error(format!(
                "Text not found in {path}. Make sure old_text matches exactly."
            ));
        }

        let new_content = content.replacen(old_text, new_text, 1);
        match tokio::fs::write(&full_path, &new_content).await {
            Ok(_) => ToolResult::Success(format!(
                "Replaced text in {path} ({count} occurrence(s) found, replaced first)"
            )),
            Err(e) => ToolResult::Error(format!("Failed to write file: {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn test_ctx(dir: &std::path::Path) -> ToolContext {
        ToolContext {
            data_dir: dir.to_path_buf(),
            session_id: "test".into(),
            config: Arc::new(
                toml::from_str("[agent]\n[llm]\nprovider=\"test\"\nmodel=\"test\"").unwrap(),
            ),
        }
    }

    #[tokio::test]
    async fn test_edit_file_success() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "Hello World").unwrap();
        let ctx = test_ctx(dir.path());

        let result = EditFileTool
            .execute(
                json!({"path": "test.txt", "old_text": "World", "new_text": "Rust"}),
                &ctx,
            )
            .await;
        assert!(!result.is_error());

        let content = std::fs::read_to_string(dir.path().join("test.txt")).unwrap();
        assert_eq!(content, "Hello Rust");
    }

    #[tokio::test]
    async fn test_edit_file_not_found() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "Hello").unwrap();
        let ctx = test_ctx(dir.path());

        let result = EditFileTool
            .execute(
                json!({"path": "test.txt", "old_text": "MISSING", "new_text": "new"}),
                &ctx,
            )
            .await;
        assert!(result.is_error());
        assert!(result.content().contains("not found"));
    }
}
