//! MCP client — connects to a single MCP server, discovers tools, executes calls

use anyhow::{anyhow, Result};
use serde_json::Value;
use std::sync::Arc;

use super::protocol::{self, McpToolDef};
use super::transport::Transport;

/// Configuration for a single MCP server
#[derive(Debug, Clone, serde::Deserialize)]
pub struct McpServerConfig {
    pub name: String,
    #[serde(default = "default_transport")]
    pub transport: String,
    /// Command to run (stdio transport)
    pub command: Option<String>,
    /// Arguments for the command
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables for the command
    #[serde(default)]
    pub env: std::collections::HashMap<String, String>,
    /// URL for HTTP transport
    pub url: Option<String>,
}

fn default_transport() -> String {
    "stdio".into()
}

/// Connected MCP client with discovered tools
pub struct McpClient {
    pub name: String,
    transport: Arc<Transport>,
    pub tools: Vec<McpToolDef>,
}

impl McpClient {
    /// Connect to an MCP server: spawn/connect, initialize, discover tools
    pub async fn connect(config: &McpServerConfig) -> Result<Self> {
        let transport = match config.transport.as_str() {
            "stdio" => {
                let command = config.command.as_ref()
                    .ok_or_else(|| anyhow!("MCP server '{}': stdio transport requires 'command'", config.name))?;
                let t = super::transport::StdioTransport::spawn(
                    command, &config.args, &config.env,
                ).await?;
                Transport::Stdio(t)
            }
            "http" => {
                let url = config.url.as_ref()
                    .ok_or_else(|| anyhow!("MCP server '{}': http transport requires 'url'", config.name))?;
                Transport::Http(super::transport::HttpTransport::new(url))
            }
            other => {
                return Err(anyhow!("MCP server '{}': unsupported transport '{other}'", config.name));
            }
        };

        // Initialize handshake
        tracing::debug!("MCP '{}': sending initialize", config.name);
        let init_result = transport
            .request("initialize", Some(protocol::initialize_params()))
            .await
            .map_err(|e| anyhow!("MCP '{}' initialize failed: {e}", config.name))?;

        let server_name = init_result["serverInfo"]["name"]
            .as_str()
            .unwrap_or("unknown");
        let protocol_version = init_result["protocolVersion"]
            .as_str()
            .unwrap_or("unknown");
        tracing::debug!(
            "MCP '{}': connected to server '{}' (protocol {})",
            config.name, server_name, protocol_version
        );

        // Send initialized notification
        transport.notify("notifications/initialized").await?;

        // Discover tools
        let tools_result = transport
            .request("tools/list", None)
            .await
            .map_err(|e| anyhow!("MCP '{}' tools/list failed: {e}", config.name))?;

        let tools = protocol::parse_tools_list(&tools_result);
        tracing::info!(
            "MCP '{}': {} tools discovered",
            config.name,
            tools.len()
        );

        Ok(Self {
            name: config.name.clone(),
            transport: Arc::new(transport),
            tools,
        })
    }

    /// Call a tool on this MCP server
    pub async fn call_tool(&self, name: &str, arguments: &Value) -> Result<String> {
        let params = protocol::tool_call_params(name, arguments);

        let result = self.transport
            .request("tools/call", Some(params))
            .await
            .map_err(|e| anyhow!("MCP '{}' tool '{}' call failed: {e}", self.name, name))?;

        // Check for isError flag
        if result["isError"].as_bool() == Some(true) {
            let text = protocol::parse_tool_result(&result);
            return Err(anyhow!("MCP tool error: {text}"));
        }

        Ok(protocol::parse_tool_result(&result))
    }

    /// Shut down the connection
    #[allow(dead_code)]
    pub async fn shutdown(&self) {
        self.transport.shutdown().await;
    }
}
