use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;

use super::types::*;
use super::LlmProvider;
use crate::config::LlmConfig;

pub struct GeminiProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: u32,
    temperature: f32,
}

/// Find the tool name for a given tool_use_id by scanning messages.
fn find_tool_name(messages: &[Message], tool_use_id: &str) -> String {
    for msg in messages {
        if let MessageContent::ToolUse { tool_calls, .. } = &msg.content {
            for tc in tool_calls {
                if tc.id == tool_use_id {
                    return tc.name.clone();
                }
            }
        }
    }
    // Fallback: use the id itself (may contain the tool name)
    tool_use_id.to_string()
}

impl GeminiProvider {
    pub fn new(config: &LlmConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()?;

        // Use Gemini default if config has the Anthropic default or is empty
        let base_url =
            if config.base_url.is_empty() || config.base_url == "https://api.anthropic.com" {
                "https://generativelanguage.googleapis.com".to_string()
            } else {
                config.base_url.clone()
            };

        Ok(Self {
            client,
            api_key: config.api_key()?,
            base_url: base_url.trim_end_matches('/').to_string(),
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            temperature: config.temperature,
        })
    }

    fn serialize_request(&self, context: &Context) -> Value {
        let contents = self.serialize_messages(&context.messages);

        let mut body = json!({
            "contents": contents,
            "generationConfig": {
                "maxOutputTokens": self.max_tokens,
                "temperature": self.temperature,
            },
        });

        if !context.system.is_empty() {
            body["systemInstruction"] = json!({
                "parts": [{"text": context.system}]
            });
        }

        if !context.tool_schemas.is_empty() {
            let declarations: Vec<Value> = context
                .tool_schemas
                .iter()
                .map(|s| {
                    json!({
                        "name": s.name,
                        "description": s.description,
                        "parameters": s.parameters,
                    })
                })
                .collect();
            body["tools"] = json!([{"functionDeclarations": declarations}]);
        }

        body
    }

    fn serialize_messages(&self, messages: &[Message]) -> Vec<Value> {
        let mut result = Vec::new();
        for msg in messages {
            match &msg.content {
                MessageContent::Text { text } => {
                    let role = match msg.role {
                        Role::User => "user",
                        Role::Assistant => "model",
                        Role::Tool => continue, // handled via ToolResult
                    };
                    result.push(json!({
                        "role": role,
                        "parts": [{"text": text}]
                    }));
                }
                MessageContent::ToolUse { text, tool_calls } => {
                    let mut parts = Vec::new();
                    if let Some(t) = text {
                        parts.push(json!({"text": t}));
                    }
                    for tc in tool_calls {
                        parts.push(json!({
                            "functionCall": {
                                "name": tc.name,
                                "args": tc.arguments,
                            }
                        }));
                    }
                    result.push(json!({
                        "role": "model",
                        "parts": parts,
                    }));
                }
                MessageContent::ToolResult {
                    tool_use_id,
                    content,
                } => {
                    // Gemini requires the tool name in functionResponse.
                    // Look up the name from the preceding ToolUse message.
                    let tool_name = find_tool_name(messages, tool_use_id);
                    result.push(json!({
                        "role": "user",
                        "parts": [{
                            "functionResponse": {
                                "name": tool_name,
                                "response": {
                                    "result": content,
                                }
                            }
                        }]
                    }));
                }
            }
        }
        result
    }

    fn parse_response(&self, body: &Value) -> Result<ChatResponse> {
        let candidate = body["candidates"]
            .get(0)
            .ok_or_else(|| anyhow!("No candidates in Gemini response"))?;

        let parts = candidate["content"]["parts"]
            .as_array()
            .ok_or_else(|| anyhow!("No parts in Gemini response candidate"))?;

        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();

        for (i, part) in parts.iter().enumerate() {
            if let Some(t) = part["text"].as_str() {
                text_parts.push(t.to_string());
            }
            if let Some(fc) = part.get("functionCall") {
                tool_calls.push(ToolCall {
                    id: format!("gemini_{i}"),
                    name: fc["name"].as_str().unwrap_or("").to_string(),
                    arguments: fc["args"].clone(),
                });
            }
        }

        let stop_reason = if !tool_calls.is_empty() {
            StopReason::ToolUse
        } else {
            match candidate["finishReason"].as_str() {
                Some("MAX_TOKENS") => StopReason::MaxTokens,
                _ => StopReason::EndTurn,
            }
        };

        let usage = Usage {
            input_tokens: body["usageMetadata"]["promptTokenCount"]
                .as_u64()
                .unwrap_or(0) as u32,
            output_tokens: body["usageMetadata"]["candidatesTokenCount"]
                .as_u64()
                .unwrap_or(0) as u32,
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
impl LlmProvider for GeminiProvider {
    fn name(&self) -> &str {
        "gemini"
    }

    fn supports_vision(&self) -> bool {
        true
    }

    async fn chat(&self, context: &Context) -> Result<ChatResponse> {
        let body = self.serialize_request(context);
        let url = format!(
            "{}/v1beta/models/{}:generateContent?key={}",
            self.base_url, self.model, self.api_key
        );

        tracing::debug!("Gemini request to {}", self.base_url);

        let response = self
            .client
            .post(&url)
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
            return Err(anyhow!("Gemini API error ({}): {}", status, error_msg));
        }

        self.parse_response(&response_body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_provider() -> GeminiProvider {
        GeminiProvider {
            client: reqwest::Client::new(),
            api_key: "test-key".into(),
            base_url: "https://generativelanguage.googleapis.com".into(),
            model: "gemini-2.0-flash".into(),
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

        // System instruction
        assert_eq!(
            body["systemInstruction"]["parts"][0]["text"],
            "You are a helpful assistant."
        );

        // Contents
        let contents = body["contents"].as_array().unwrap();
        assert_eq!(contents.len(), 1);
        assert_eq!(contents[0]["role"], "user");
        assert_eq!(contents[0]["parts"][0]["text"], "Hello");

        // Generation config
        assert_eq!(body["generationConfig"]["maxOutputTokens"], 1024);
        let temp = body["generationConfig"]["temperature"].as_f64().unwrap();
        assert!((temp - 0.7).abs() < 0.01);

        // No tools
        assert!(body.get("tools").is_none());
    }

    #[test]
    fn test_parse_text_response() {
        let provider = test_provider();
        let body = json!({
            "candidates": [{
                "content": {
                    "parts": [{"text": "Hello!"}],
                    "role": "model"
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 10,
                "candidatesTokenCount": 5
            }
        });
        let resp = provider.parse_response(&body).unwrap();
        assert_eq!(resp.text.as_deref(), Some("Hello!"));
        assert!(resp.tool_calls.is_empty());
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
        assert_eq!(resp.usage.input_tokens, 10);
        assert_eq!(resp.usage.output_tokens, 5);
    }

    #[test]
    fn test_parse_tool_call_response() {
        let provider = test_provider();
        let body = json!({
            "candidates": [{
                "content": {
                    "parts": [
                        {"text": "Let me check the time."},
                        {
                            "functionCall": {
                                "name": "get_time",
                                "args": {"timezone": "UTC"}
                            }
                        }
                    ],
                    "role": "model"
                },
                "finishReason": "STOP"
            }],
            "usageMetadata": {
                "promptTokenCount": 20,
                "candidatesTokenCount": 15
            }
        });
        let resp = provider.parse_response(&body).unwrap();
        assert_eq!(resp.text.as_deref(), Some("Let me check the time."));
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].id, "gemini_1");
        assert_eq!(resp.tool_calls[0].name, "get_time");
        assert_eq!(resp.tool_calls[0].arguments["timezone"], "UTC");
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
    }

    #[test]
    fn test_serialize_tool_use_messages() {
        let provider = test_provider();
        let messages = vec![
            Message::user("What time is it?"),
            Message::assistant_tool_use(
                Some("Let me check.".into()),
                vec![ToolCall {
                    id: "gemini_0".into(),
                    name: "get_time".into(),
                    arguments: json!({}),
                }],
            ),
            Message::tool_result("gemini_0", "3:42 PM"),
        ];
        let serialized = provider.serialize_messages(&messages);
        assert_eq!(serialized.len(), 3);

        // User message
        assert_eq!(serialized[0]["role"], "user");
        assert_eq!(serialized[0]["parts"][0]["text"], "What time is it?");

        // Model message with functionCall
        assert_eq!(serialized[1]["role"], "model");
        let parts = serialized[1]["parts"].as_array().unwrap();
        assert_eq!(parts[0]["text"], "Let me check.");
        assert!(parts[1].get("functionCall").is_some());
        assert_eq!(parts[1]["functionCall"]["name"], "get_time");

        // Tool result as functionResponse
        assert_eq!(serialized[2]["role"], "user");
        let resp_part = &serialized[2]["parts"][0]["functionResponse"];
        assert_eq!(resp_part["name"], "get_time");
        assert_eq!(resp_part["response"]["result"], "3:42 PM");
    }
}
