//! MCP (Model Context Protocol) client support
//!
//! Connects MiniClaw to external MCP servers, discovers their tools,
//! and registers them alongside built-in tools.

pub mod protocol;
pub mod transport;
pub mod client;

use async_trait::async_trait;
use std::sync::Arc;

use client::{McpClient, McpServerConfig};
use crate::tools::registry::{Tool, ToolContext, ToolResult, ToolRegistry};

/// Wraps an MCP tool as a local Tool trait impl.
/// The agent loop doesn't know or care that this tool is from MCP.
struct McpTool {
    tool_name: String,
    description: String,
    schema: serde_json::Value,
    client: Arc<McpClient>,
}

#[async_trait]
impl Tool for McpTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.schema.clone()
    }

    async fn execute(&self, args: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        match self.client.call_tool(&self.tool_name, &args).await {
            Ok(result) => ToolResult::Success(result),
            Err(e) => ToolResult::Error(format!("MCP error: {e}")),
        }
    }
}

/// Connect to all configured MCP servers and register their tools
pub async fn register_mcp_tools(
    configs: &[McpServerConfig],
    registry: &mut ToolRegistry,
) -> Vec<Arc<McpClient>> {
    let mut clients = Vec::new();

    for config in configs {
        tracing::info!("Connecting to MCP server '{}'...", config.name);

        match McpClient::connect(config).await {
            Ok(mcp_client) => {
                let client = Arc::new(mcp_client);

                // Register each tool from this server
                let mut registered = 0;
                for tool_def in &client.tools {
                    let tool_name = tool_def.name.clone();

                    // Check for name conflicts
                    let existing_names: Vec<&str> = registry.tool_names();
                    if existing_names.contains(&tool_name.as_str()) {
                        tracing::warn!(
                            "MCP '{}': tool '{}' conflicts with existing tool, skipping",
                            config.name, tool_name
                        );
                        continue;
                    }

                    let mcp_tool = McpTool {
                        tool_name: tool_def.name.clone(),
                        description: tool_def.description.clone().unwrap_or_default(),
                        schema: tool_def.input_schema.clone().unwrap_or(serde_json::json!({
                            "type": "object",
                            "properties": {}
                        })),
                        client: client.clone(),
                    };

                    registry.register(mcp_tool);
                    registered += 1;
                }

                tracing::info!(
                    "MCP '{}': {registered} tools registered",
                    config.name
                );
                clients.push(client);
            }
            Err(e) => {
                // Non-fatal: log and continue without this server
                tracing::error!("MCP '{}': failed to connect: {e}", config.name);
            }
        }
    }

    clients
}

/// Shut down all MCP clients
#[allow(dead_code)] // used in graceful shutdown
pub async fn shutdown_clients(clients: &[Arc<McpClient>]) {
    for client in clients {
        tracing::debug!("Shutting down MCP client '{}'", client.name);
        client.shutdown().await;
    }
}
