use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::config::Config;
use crate::llm::types::ToolSchema;

pub enum ToolResult {
    Success(String),
    Error(String),
}

impl ToolResult {
    #[allow(dead_code)]
    pub fn content(&self) -> &str {
        match self {
            ToolResult::Success(s) => s,
            ToolResult::Error(s) => s,
        }
    }

    pub fn is_error(&self) -> bool {
        matches!(self, ToolResult::Error(_))
    }
}

pub struct ToolContext {
    pub data_dir: PathBuf,
    #[allow(dead_code)] // available for future tool use
    pub session_id: String,
    pub config: Arc<Config>,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> ToolResult;
}

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            tools: HashMap::new(),
        }
    }

    pub fn register(&mut self, tool: impl Tool + 'static) {
        let name = tool.name().to_string();
        tracing::debug!("Registered tool: {name}");
        self.tools.insert(name, Box::new(tool));
    }

    pub fn schemas(&self) -> Vec<ToolSchema> {
        self.tools
            .values()
            .map(|t| ToolSchema {
                name: t.name().to_string(),
                description: t.description().to_string(),
                parameters: t.parameters_schema(),
            })
            .collect()
    }

    pub async fn execute(
        &self,
        name: &str,
        args: serde_json::Value,
        ctx: &ToolContext,
    ) -> ToolResult {
        match self.tools.get(name) {
            Some(tool) => {
                tracing::info!("Executing tool: {name}");
                tool.execute(args, ctx).await
            }
            None => ToolResult::Error(format!("Unknown tool: {name}")),
        }
    }

    pub fn tool_names(&self) -> Vec<&str> {
        self.tools.keys().map(|s| s.as_str()).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct DummyTool;

    #[async_trait]
    impl Tool for DummyTool {
        fn name(&self) -> &str { "dummy" }
        fn description(&self) -> &str { "A dummy tool for testing" }
        fn parameters_schema(&self) -> serde_json::Value {
            json!({"type": "object", "properties": {}})
        }
        async fn execute(&self, _args: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
            ToolResult::Success("dummy result".into())
        }
    }

    fn test_ctx() -> ToolContext {
        ToolContext {
            data_dir: PathBuf::from("/tmp/uniclaw-test"),
            session_id: "test".into(),
            config: Arc::new(toml::from_str::<Config>(
                "[agent]\n[llm]\nprovider=\"anthropic\"\nmodel=\"test\""
            ).unwrap()),
        }
    }

    use crate::config::Config;

    #[test]
    fn test_register_and_schemas() {
        let mut registry = ToolRegistry::new();
        registry.register(DummyTool);
        let schemas = registry.schemas();
        assert_eq!(schemas.len(), 1);
        assert_eq!(schemas[0].name, "dummy");
    }

    #[tokio::test]
    async fn test_dispatch_known_tool() {
        let mut registry = ToolRegistry::new();
        registry.register(DummyTool);
        let ctx = test_ctx();
        let result = registry.execute("dummy", json!({}), &ctx).await;
        assert!(!result.is_error());
        assert_eq!(result.content(), "dummy result");
    }

    #[tokio::test]
    async fn test_dispatch_unknown_tool() {
        let registry = ToolRegistry::new();
        let ctx = test_ctx();
        let result = registry.execute("nonexistent", json!({}), &ctx).await;
        assert!(result.is_error());
    }
}
