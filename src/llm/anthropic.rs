use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;

use super::types::*;
use super::LlmProvider;
use crate::config::LlmConfig;

pub struct AnthropicProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: u32,
    temperature: f32,
}

impl AnthropicProvider {
    pub fn new(config: &LlmConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()?;
        Ok(Self {
            client,
            api_key: config.api_key()?,
            base_url: config.base_url.trim_end_matches('/').to_string(),
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            temperature: config.temperature,
        })
    }

    fn serialize_request(&self, context: &Context) -> Value {
        let messages = self.serialize_messages(&context.messages);
        let tools = self.serialize_tools(&context.tool_schemas);

        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "temperature": self.temperature,
            "messages": messages,
        });

        if !context.system.is_empty() {
            body["system"] = json!(context.system);
        }
        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }

        body
    }

    fn serialize_messages(&self, messages: &[Message]) -> Vec<Value> {
        let mut result = Vec::new();
        for msg in messages {
            match &msg.content {
                MessageContent::Text { text } => {
                    result.push(json!({
                        "role": msg.role.to_string(),
                        "content": text,
                    }));
                }
                MessageContent::ToolUse { text, tool_calls } => {
                    let mut content = Vec::new();
                    if let Some(t) = text {
                        content.push(json!({"type": "text", "text": t}));
                    }
                    for tc in tool_calls {
                        content.push(json!({
                            "type": "tool_use",
                            "id": tc.id,
                            "name": tc.name,
                            "input": tc.arguments,
                        }));
                    }
                    result.push(json!({
                        "role": "assistant",
                        "content": content,
                    }));
                }
                MessageContent::ToolResult {
                    tool_use_id,
                    content,
                } => {
                    result.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": tool_use_id,
                            "content": content,
                        }],
                    }));
                }
            }
        }
        result
    }

    fn serialize_tools(&self, schemas: &[ToolSchema]) -> Vec<Value> {
        schemas
            .iter()
            .map(|s| {
                json!({
                    "name": s.name,
                    "description": s.description,
                    "input_schema": s.parameters,
                })
            })
            .collect()
    }

    fn parse_response(&self, body: &Value) -> Result<ChatResponse> {
        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();

        if let Some(content) = body["content"].as_array() {
            for block in content {
                match block["type"].as_str() {
                    Some("text") => {
                        if let Some(t) = block["text"].as_str() {
                            text_parts.push(t.to_string());
                        }
                    }
                    Some("tool_use") => {
                        tool_calls.push(ToolCall {
                            id: block["id"].as_str().unwrap_or("").to_string(),
                            name: block["name"].as_str().unwrap_or("").to_string(),
                            arguments: block["input"].clone(),
                        });
                    }
                    _ => {}
                }
            }
        }

        let stop_reason = match body["stop_reason"].as_str() {
            Some("tool_use") => StopReason::ToolUse,
            Some("max_tokens") => StopReason::MaxTokens,
            _ => StopReason::EndTurn,
        };

        let usage = Usage {
            input_tokens: body["usage"]["input_tokens"].as_u64().unwrap_or(0) as u32,
            output_tokens: body["usage"]["output_tokens"].as_u64().unwrap_or(0) as u32,
        };

        let text = if text_parts.is_empty() {
            None
        } else {
            Some(text_parts.join(""))
        };

        Ok(ChatResponse {
            text,
            tool_calls,
            stop_reason,
            usage,
        })
    }
}

#[async_trait]
impl LlmProvider for AnthropicProvider {
    fn name(&self) -> &str {
        "anthropic"
    }

    async fn chat(&self, context: &Context) -> Result<ChatResponse> {
        let body = self.serialize_request(context);
        let url = format!("{}/v1/messages", self.base_url);

        tracing::debug!("Anthropic request to {url}");

        let response = self
            .client
            .post(&url)
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await?;

        let status = response.status();
        let response_body: Value = response.json().await?;

        if !status.is_success() {
            let error_msg = response_body["error"]["message"]
                .as_str()
                .unwrap_or("Unknown error");
            return Err(anyhow!("Anthropic API error ({}): {}", status, error_msg));
        }

        self.parse_response(&response_body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_provider() -> AnthropicProvider {
        AnthropicProvider {
            client: reqwest::Client::new(),
            api_key: "test-key".into(),
            base_url: "https://api.anthropic.com".into(),
            model: "claude-sonnet-4-6".into(),
            max_tokens: 1024,
            temperature: 0.7,
        }
    }

    #[test]
    fn test_serialize_simple_request() {
        let provider = test_provider();
        let ctx = Context {
            system: "You are a helpful assistant.".into(),
            messages: vec![Message::user("Hello")],
            tool_schemas: vec![],
        };
        let body = provider.serialize_request(&ctx);
        assert_eq!(body["model"], "claude-sonnet-4-6");
        assert_eq!(body["system"], "You are a helpful assistant.");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "Hello");
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn test_serialize_with_tools() {
        let provider = test_provider();
        let ctx = Context {
            system: String::new(),
            messages: vec![Message::user("What time is it?")],
            tool_schemas: vec![ToolSchema {
                name: "get_time".into(),
                description: "Get current time".into(),
                parameters: json!({"type": "object", "properties": {}}),
            }],
        };
        let body = provider.serialize_request(&ctx);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "get_time");
    }

    #[test]
    fn test_serialize_tool_use_message() {
        let provider = test_provider();
        let messages = vec![
            Message::user("What time is it?"),
            Message::assistant_tool_use(
                Some("Let me check.".into()),
                vec![ToolCall {
                    id: "call_1".into(),
                    name: "get_time".into(),
                    arguments: json!({}),
                }],
            ),
            Message::tool_result("call_1", "3:42 PM"),
        ];
        let serialized = provider.serialize_messages(&messages);
        assert_eq!(serialized.len(), 3);
        // assistant message has content blocks
        let assistant = &serialized[1];
        assert_eq!(assistant["role"], "assistant");
        let content = assistant["content"].as_array().unwrap();
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["type"], "tool_use");
        // tool result is a user message
        let tool_result = &serialized[2];
        assert_eq!(tool_result["role"], "user");
    }

    #[test]
    fn test_parse_text_response() {
        let provider = test_provider();
        let body = json!({
            "content": [{"type": "text", "text": "Hello!"}],
            "stop_reason": "end_turn",
            "usage": {"input_tokens": 10, "output_tokens": 5}
        });
        let resp = provider.parse_response(&body).unwrap();
        assert_eq!(resp.text.as_deref(), Some("Hello!"));
        assert!(resp.tool_calls.is_empty());
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
        assert_eq!(resp.usage.input_tokens, 10);
    }

    #[test]
    fn test_parse_tool_use_response() {
        let provider = test_provider();
        let body = json!({
            "content": [
                {"type": "text", "text": "Let me check."},
                {"type": "tool_use", "id": "call_1", "name": "get_time", "input": {}}
            ],
            "stop_reason": "tool_use",
            "usage": {"input_tokens": 20, "output_tokens": 15}
        });
        let resp = provider.parse_response(&body).unwrap();
        assert_eq!(resp.text.as_deref(), Some("Let me check."));
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].name, "get_time");
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
    }
}
