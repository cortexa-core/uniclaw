use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::json;
use std::path::{Path, PathBuf};

use super::registry::{Tool, ToolContext, ToolResult};

/// Validate that a requested path doesn't escape the data directory.
pub fn validate_path(data_dir: &Path, requested: &str) -> Result<PathBuf> {
    let requested = requested.trim_start_matches('/');

    // Reject traversal attempts immediately — before any filesystem operations
    if requested.contains("..") {
        return Err(anyhow!("Path traversal not allowed"));
    }

    let data_dir_canonical = data_dir
        .canonicalize()
        .map_err(|e| anyhow!("Data directory not accessible: {e}"))?;
    let joined = data_dir_canonical.join(requested);

    // For existing files, canonicalize and verify containment
    if joined.exists() {
        let canonical = joined.canonicalize()?;
        if !canonical.starts_with(&data_dir_canonical) {
            return Err(anyhow!("Path escapes data directory"));
        }
        return Ok(canonical);
    }

    // For new files, walk up to the nearest existing ancestor and verify it's inside data_dir.
    // This prevents creating files in directories outside the sandbox.
    let mut ancestor = joined.parent();
    while let Some(dir) = ancestor {
        if dir.exists() {
            let canonical_ancestor = dir.canonicalize()?;
            if !canonical_ancestor.starts_with(&data_dir_canonical) {
                return Err(anyhow!("Path escapes data directory"));
            }
            break;
        }
        ancestor = dir.parent();
    }

    Ok(joined)
}

// --- ReadFileTool ---

pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str { "read_file" }

    fn description(&self) -> &str {
        "Read the contents of a file from the data directory."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to the data directory (e.g., 'SOUL.md', 'memory/MEMORY.md')"
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let path = match args["path"].as_str() {
            Some(p) => p,
            None => return ToolResult::Error("Missing required parameter: path".into()),
        };

        let full_path = match validate_path(&ctx.data_dir, path) {
            Ok(p) => p,
            Err(e) => return ToolResult::Error(format!("Invalid path: {e}")),
        };

        match std::fs::read_to_string(&full_path) {
            Ok(content) => ToolResult::Success(content),
            Err(e) => ToolResult::Error(format!("Failed to read file: {e}")),
        }
    }
}

// --- WriteFileTool ---

pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str { "write_file" }

    fn description(&self) -> &str {
        "Write content to a file in the data directory. Creates parent directories if needed."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["path", "content"],
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to the data directory"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let path = match args["path"].as_str() {
            Some(p) => p,
            None => return ToolResult::Error("Missing required parameter: path".into()),
        };
        let content = match args["content"].as_str() {
            Some(c) => c,
            None => return ToolResult::Error("Missing required parameter: content".into()),
        };

        let full_path = match validate_path(&ctx.data_dir, path) {
            Ok(p) => p,
            Err(e) => return ToolResult::Error(format!("Invalid path: {e}")),
        };

        // Create parent directories
        if let Some(parent) = full_path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return ToolResult::Error(format!("Failed to create directories: {e}"));
            }
        }

        match std::fs::write(&full_path, content) {
            Ok(_) => ToolResult::Success(format!("Written {} bytes to {path}", content.len())),
            Err(e) => ToolResult::Error(format!("Failed to write file: {e}")),
        }
    }
}

// --- ListDirTool ---

pub struct ListDirTool;

#[async_trait]
impl Tool for ListDirTool {
    fn name(&self) -> &str { "list_dir" }

    fn description(&self) -> &str {
        "List files and directories in the data directory."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path relative to data directory. Defaults to root of data directory."
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let path = args["path"].as_str().unwrap_or("");

        let dir_path = if path.is_empty() {
            ctx.data_dir.clone()
        } else {
            match validate_path(&ctx.data_dir, path) {
                Ok(p) => p,
                Err(e) => return ToolResult::Error(format!("Invalid path: {e}")),
            }
        };

        let entries = match std::fs::read_dir(&dir_path) {
            Ok(entries) => entries,
            Err(e) => return ToolResult::Error(format!("Failed to list directory: {e}")),
        };

        let mut lines = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            let metadata = entry.metadata();
            let (kind, size) = match metadata {
                Ok(m) => {
                    if m.is_dir() {
                        ("dir".to_string(), String::new())
                    } else {
                        ("file".to_string(), format!(" ({} bytes)", m.len()))
                    }
                }
                Err(_) => ("unknown".to_string(), String::new()),
            };
            lines.push(format!("  [{kind}] {name}{size}"));
        }

        if lines.is_empty() {
            ToolResult::Success("Directory is empty.".into())
        } else {
            lines.sort();
            ToolResult::Success(lines.join("\n"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn test_ctx(dir: &Path) -> ToolContext {
        ToolContext {
            data_dir: dir.to_path_buf(),
            session_id: "test".into(),
            config: Arc::new(
                toml::from_str("[agent]\n[llm]\nprovider=\"anthropic\"\nmodel=\"test\"").unwrap(),
            ),
        }
    }

    #[test]
    fn test_validate_path_normal() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello").unwrap();
        let result = validate_path(dir.path(), "test.txt");
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_path_escape() {
        let dir = tempfile::tempdir().unwrap();
        let result = validate_path(dir.path(), "../../../etc/passwd");
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_path_dotdot() {
        let dir = tempfile::tempdir().unwrap();
        let result = validate_path(dir.path(), "subdir/../../escape");
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_read_file() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("hello.txt"), "world").unwrap();
        let ctx = test_ctx(dir.path());
        let result = ReadFileTool.execute(json!({"path": "hello.txt"}), &ctx).await;
        assert!(!result.is_error());
        assert_eq!(result.content(), "world");
    }

    #[tokio::test]
    async fn test_read_file_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());
        let result = ReadFileTool.execute(json!({"path": "nope.txt"}), &ctx).await;
        assert!(result.is_error());
    }

    #[tokio::test]
    async fn test_write_file() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());
        let result = WriteFileTool
            .execute(json!({"path": "out.txt", "content": "hello"}), &ctx)
            .await;
        assert!(!result.is_error());
        assert_eq!(std::fs::read_to_string(dir.path().join("out.txt")).unwrap(), "hello");
    }

    #[tokio::test]
    async fn test_write_file_creates_parents() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());
        let result = WriteFileTool
            .execute(json!({"path": "sub/dir/file.txt", "content": "nested"}), &ctx)
            .await;
        assert!(!result.is_error());
        assert!(dir.path().join("sub/dir/file.txt").exists());
    }

    #[tokio::test]
    async fn test_list_dir() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "").unwrap();
        std::fs::write(dir.path().join("b.txt"), "hi").unwrap();
        std::fs::create_dir(dir.path().join("subdir")).unwrap();
        let ctx = test_ctx(dir.path());
        let result = ListDirTool.execute(json!({}), &ctx).await;
        assert!(!result.is_error());
        let content = result.content();
        assert!(content.contains("a.txt"));
        assert!(content.contains("b.txt"));
        assert!(content.contains("[dir] subdir"));
    }
}
