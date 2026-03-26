//! JSON-RPC 2.0 types for MCP protocol

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};

static REQUEST_ID: AtomicU64 = AtomicU64::new(1);

fn next_id() -> u64 {
    REQUEST_ID.fetch_add(1, Ordering::Relaxed)
}

/// JSON-RPC 2.0 request
#[derive(Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(method: &str, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            id: next_id(),
            method: method.to_string(),
            params,
        }
    }
}

/// JSON-RPC 2.0 notification (no id, no response expected)
#[derive(Serialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: &'static str,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcNotification {
    pub fn new(method: &str) -> Self {
        Self {
            jsonrpc: "2.0",
            method: method.to_string(),
            params: None,
        }
    }
}

/// JSON-RPC 2.0 response
#[derive(Deserialize, Debug)]
pub struct JsonRpcResponse {
    #[allow(dead_code)]
    pub id: Option<u64>,
    pub result: Option<Value>,
    pub error: Option<JsonRpcError>,
}

#[derive(Deserialize, Debug)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

impl JsonRpcResponse {
    pub fn into_result(self) -> anyhow::Result<Value> {
        if let Some(err) = self.error {
            return Err(anyhow::anyhow!("MCP error ({}): {}", err.code, err.message));
        }
        self.result.ok_or_else(|| anyhow::anyhow!("MCP response missing both result and error"))
    }
}

// --- MCP-specific message types ---

/// MCP initialize params
pub fn initialize_params() -> Value {
    serde_json::json!({
        "protocolVersion": "2024-11-05",
        "capabilities": {},
        "clientInfo": {
            "name": "miniclaw",
            "version": env!("CARGO_PKG_VERSION")
        }
    })
}

/// Parsed MCP tool definition (from tools/list response)
#[derive(Debug, Clone, Deserialize)]
pub struct McpToolDef {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(rename = "inputSchema", default)]
    pub input_schema: Option<Value>,
}

/// Parse tools from a tools/list response
pub fn parse_tools_list(result: &Value) -> Vec<McpToolDef> {
    result["tools"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Build tools/call params
pub fn tool_call_params(name: &str, arguments: &Value) -> Value {
    serde_json::json!({
        "name": name,
        "arguments": arguments
    })
}

/// Parse tool result content from tools/call response
pub fn parse_tool_result(result: &Value) -> String {
    // MCP tool results have a "content" array with text/image blocks
    if let Some(content) = result["content"].as_array() {
        content
            .iter()
            .filter_map(|block| {
                if block["type"].as_str() == Some("text") {
                    block["text"].as_str().map(|s| s.to_string())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else if let Some(text) = result.as_str() {
        text.to_string()
    } else {
        result.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_serialization() {
        let req = JsonRpcRequest::new("tools/list", None);
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"jsonrpc\":\"2.0\""));
        assert!(json.contains("\"method\":\"tools/list\""));
        assert!(!json.contains("\"params\"")); // None skipped
    }

    #[test]
    fn test_request_with_params() {
        let params = serde_json::json!({"name": "test"});
        let req = JsonRpcRequest::new("tools/call", Some(params));
        let json = serde_json::to_string(&req).unwrap();
        assert!(json.contains("\"params\""));
    }

    #[test]
    fn test_response_success() {
        let resp: JsonRpcResponse = serde_json::from_str(
            r#"{"jsonrpc":"2.0","id":1,"result":{"tools":[]}}"#
        ).unwrap();
        assert!(resp.error.is_none());
        let result = resp.into_result().unwrap();
        assert!(result["tools"].is_array());
    }

    #[test]
    fn test_response_error() {
        let resp: JsonRpcResponse = serde_json::from_str(
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32600,"message":"Invalid request"}}"#
        ).unwrap();
        let err = resp.into_result().unwrap_err();
        assert!(err.to_string().contains("Invalid request"));
    }

    #[test]
    fn test_parse_tools_list() {
        let result = serde_json::json!({
            "tools": [
                {"name": "read_file", "description": "Read a file", "inputSchema": {"type": "object"}},
                {"name": "write_file", "description": "Write a file"}
            ]
        });
        let tools = parse_tools_list(&result);
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "read_file");
        assert_eq!(tools[1].name, "write_file");
    }

    #[test]
    fn test_parse_tool_result() {
        let result = serde_json::json!({
            "content": [
                {"type": "text", "text": "Hello world"},
                {"type": "text", "text": "Second line"}
            ]
        });
        assert_eq!(parse_tool_result(&result), "Hello world\nSecond line");
    }
}
