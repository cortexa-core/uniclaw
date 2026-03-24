use async_trait::async_trait;
use serde_json::json;
use std::time::Duration;

use super::registry::{Tool, ToolContext, ToolResult};

pub struct HttpFetchTool;

#[async_trait]
impl Tool for HttpFetchTool {
    fn name(&self) -> &str { "http_fetch" }

    fn description(&self) -> &str {
        "Fetch content from a URL. Returns the response body as text. \
         Supports GET and POST methods."
    }

    fn parameters_schema(&self) -> serde_json::Value {
        json!({
            "type": "object",
            "required": ["url"],
            "properties": {
                "url": {
                    "type": "string",
                    "description": "URL to fetch (must be http:// or https://)"
                },
                "method": {
                    "type": "string",
                    "description": "HTTP method: GET (default) or POST",
                    "enum": ["GET", "POST"]
                }
            }
        })
    }

    async fn execute(&self, args: serde_json::Value, ctx: &ToolContext) -> ToolResult {
        let url = match args["url"].as_str() {
            Some(u) => u,
            None => return ToolResult::Error("Missing required parameter: url".into()),
        };

        // Basic URL validation
        if !url.starts_with("http://") && !url.starts_with("https://") {
            return ToolResult::Error("URL must start with http:// or https://".into());
        }

        let method = args["method"].as_str().unwrap_or("GET");
        let timeout = Duration::from_secs(ctx.config.tools.http_fetch_timeout_secs);

        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build();

        let client = match client {
            Ok(c) => c,
            Err(e) => return ToolResult::Error(format!("Failed to create HTTP client: {e}")),
        };

        let response = match method.to_uppercase().as_str() {
            "POST" => client.post(url).send().await,
            _ => client.get(url).send().await,
        };

        match response {
            Ok(resp) => {
                let status = resp.status();
                let body = match resp.text().await {
                    Ok(b) => b,
                    Err(e) => return ToolResult::Error(format!("Failed to read response: {e}")),
                };

                // Truncate long responses
                let body = if body.len() > 8192 {
                    format!("{}...\n(truncated at 8192 chars, total {} chars)", &body[..8192], body.len())
                } else {
                    body
                };

                // Scan for leaked credentials (credential boundary injection)
                let safe_body = redact_known_secrets(&body, ctx);

                if status.is_success() {
                    ToolResult::Success(format!("HTTP {status}\n\n{safe_body}"))
                } else {
                    ToolResult::Error(format!("HTTP {status}\n\n{safe_body}"))
                }
            }
            Err(e) => {
                if e.is_timeout() {
                    ToolResult::Error(format!("Request timed out after {}s", timeout.as_secs()))
                } else {
                    ToolResult::Error(format!("Request failed: {e}"))
                }
            }
        }
    }
}

/// Scan response body for any known secrets and redact them.
/// This implements the credential boundary injection pattern from IronClaw.
fn redact_known_secrets(text: &str, ctx: &ToolContext) -> String {
    let mut result = text.to_string();

    // Check all known API keys from env vars
    for env_name in &["ANTHROPIC_API_KEY", "OPENAI_API_KEY"] {
        if let Ok(key) = std::env::var(env_name) {
            if !key.is_empty() && result.contains(&key) {
                result = result.replace(&key, "[REDACTED]");
                tracing::warn!("Redacted leaked credential ({env_name}) from HTTP response");
            }
        }
    }

    result
}
