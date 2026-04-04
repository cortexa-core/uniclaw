use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::time::Duration;

use super::types::*;
use super::LlmProvider;
use crate::config::LlmConfig;

pub struct OpenAiProvider {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    model: String,
    max_tokens: u32,
    temperature: f32,
    auth_style: crate::llm::aliases::AuthStyle,
    extra_headers: Vec<(String, String)>,
    provider_name: String,
}

impl OpenAiProvider {
    pub fn new(config: &LlmConfig) -> Result<Self> {
        use crate::llm::aliases;

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()?;

        let alias = aliases::resolve(&config.provider);

        let auth_style = alias
            .as_ref()
            .map(|a| a.auth_style)
            .unwrap_or(aliases::AuthStyle::Bearer);

        let extra_headers: Vec<(String, String)> = alias
            .as_ref()
            .map(|a| {
                a.extra_headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        // Use alias base_url if config has the default or is empty
        let base_url = if config.base_url.is_empty()
            || config.base_url == "https://api.anthropic.com"
        {
            alias
                .as_ref()
                .map(|a| a.base_url.to_string())
                .unwrap_or_else(|| config.base_url.clone())
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
            auth_style,
            extra_headers,
            provider_name: config.provider.clone(),
        })
    }

    fn serialize_request(&self, context: &Context) -> Value {
        let mut messages = Vec::new();

        // System prompt as first message
        if !context.system.is_empty() {
            messages.push(json!({"role": "system", "content": context.system}));
        }

        // Conversation messages
        for msg in &context.messages {
            match &msg.content {
                MessageContent::Text { text } => {
                    messages.push(json!({
                        "role": msg.role.to_string(),
                        "content": text,
                    }));
                }
                MessageContent::ToolUse { text, tool_calls } => {
                    let mut assistant_msg = json!({
                        "role": "assistant",
                    });
                    if let Some(t) = text {
                        assistant_msg["content"] = json!(t);
                    }
                    let calls: Vec<Value> = tool_calls
                        .iter()
                        .map(|tc| {
                            json!({
                                "id": tc.id,
                                "type": "function",
                                "function": {
                                    "name": tc.name,
                                    "arguments": tc.arguments.to_string(),
                                }
                            })
                        })
                        .collect();
                    assistant_msg["tool_calls"] = json!(calls);
                    messages.push(assistant_msg);
                }
                MessageContent::ToolResult {
                    tool_use_id,
                    content,
                } => {
                    messages.push(json!({
                        "role": "tool",
                        "tool_call_id": tool_use_id,
                        "content": content,
                    }));
                }
            }
        }

        let tools: Vec<Value> = context
            .tool_schemas
            .iter()
            .map(|s| {
                json!({
                    "type": "function",
                    "function": {
                        "name": s.name,
                        "description": s.description,
                        "parameters": s.parameters,
                    }
                })
            })
            .collect();

        let mut body = json!({
            "model": self.model,
            "max_tokens": self.max_tokens,
            "temperature": self.temperature,
            "messages": messages,
        });

        if !tools.is_empty() {
            body["tools"] = json!(tools);
        }

        body
    }

    fn parse_response(&self, body: &Value) -> Result<ChatResponse> {
        let choice = body["choices"]
            .get(0)
            .ok_or_else(|| anyhow!("No choices in OpenAI response"))?;

        let message = &choice["message"];
        let text = message["content"].as_str().map(|s| s.to_string());

        let mut tool_calls = Vec::new();
        if let Some(calls) = message["tool_calls"].as_array() {
            for call in calls {
                let args_str = call["function"]["arguments"].as_str().unwrap_or("{}");
                let arguments: serde_json::Value =
                    serde_json::from_str(args_str).unwrap_or(json!({}));
                tool_calls.push(ToolCall {
                    id: call["id"].as_str().unwrap_or("").to_string(),
                    name: call["function"]["name"].as_str().unwrap_or("").to_string(),
                    arguments,
                });
            }
        }

        let stop_reason = match choice["finish_reason"].as_str() {
            Some("tool_calls") => StopReason::ToolUse,
            Some("length") => StopReason::MaxTokens,
            _ => {
                if tool_calls.is_empty() {
                    StopReason::EndTurn
                } else {
                    StopReason::ToolUse
                }
            }
        };

        let usage = Usage {
            input_tokens: body["usage"]["prompt_tokens"].as_u64().unwrap_or(0) as u32,
            output_tokens: body["usage"]["completion_tokens"].as_u64().unwrap_or(0) as u32,
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
impl LlmProvider for OpenAiProvider {
    fn name(&self) -> &str {
        &self.provider_name
    }

    async fn chat(&self, context: &Context) -> Result<ChatResponse> {
        use crate::llm::aliases::AuthStyle;

        let body = self.serialize_request(context);
        let url = format!("{}/v1/chat/completions", self.base_url);

        tracing::debug!("OpenAI-compatible request to {url}");

        let mut request = self
            .client
            .post(&url)
            .header("content-type", "application/json");

        match self.auth_style {
            AuthStyle::Bearer => {
                if !self.api_key.is_empty() {
                    request = request.bearer_auth(&self.api_key);
                }
            }
            AuthStyle::XApiKey => {
                request = request.header("x-api-key", &self.api_key);
            }
            AuthStyle::None | AuthStyle::QueryParam => {}
        }
        for (key, value) in &self.extra_headers {
            request = request.header(key.as_str(), value.as_str());
        }

        let response = request.json(&body).send().await?;
        let status = response.status();
        let response_body: Value = response.json().await?;

        if !status.is_success() {
            let error_msg = response_body["error"]["message"]
                .as_str()
                .unwrap_or("Unknown error");
            return Err(anyhow!("OpenAI API error ({}): {}", status, error_msg));
        }

        self.parse_response(&response_body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_provider() -> OpenAiProvider {
        OpenAiProvider {
            client: reqwest::Client::new(),
            api_key: "test-key".into(),
            base_url: "https://api.openai.com".into(),
            model: "gpt-4o".into(),
            max_tokens: 1024,
            temperature: 0.7,
            auth_style: crate::llm::aliases::AuthStyle::Bearer,
            extra_headers: vec![],
            provider_name: "openai".into(),
        }
    }

    #[test]
    fn test_serialize_simple_request() {
        let provider = test_provider();
        let ctx = Context {
            system: "You are helpful.".into(),
            messages: vec![Message::user("Hello")],
            tool_schemas: vec![],
        };
        let body = provider.serialize_request(&ctx);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages.len(), 2); // system + user
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[1]["role"], "user");
        assert_eq!(messages[1]["content"], "Hello");
    }

    #[test]
    fn test_serialize_with_tools() {
        let provider = test_provider();
        let ctx = Context {
            system: String::new(),
            messages: vec![Message::user("Hi")],
            tool_schemas: vec![ToolSchema {
                name: "get_time".into(),
                description: "Get time".into(),
                parameters: json!({"type": "object", "properties": {}}),
            }],
        };
        let body = provider.serialize_request(&ctx);
        let tools = body["tools"].as_array().unwrap();
        assert_eq!(tools[0]["type"], "function");
        assert_eq!(tools[0]["function"]["name"], "get_time");
    }

    #[test]
    fn test_serialize_tool_call_messages() {
        let provider = test_provider();
        let ctx = Context {
            system: String::new(),
            messages: vec![
                Message::user("What time?"),
                Message::assistant_tool_use(
                    None,
                    vec![ToolCall {
                        id: "call_1".into(),
                        name: "get_time".into(),
                        arguments: json!({}),
                    }],
                ),
                Message::tool_result("call_1", "3:42 PM"),
            ],
            tool_schemas: vec![],
        };
        let body = provider.serialize_request(&ctx);
        let messages = body["messages"].as_array().unwrap();
        assert_eq!(messages[1]["role"], "assistant");
        assert!(messages[1]["tool_calls"].is_array());
        assert_eq!(messages[2]["role"], "tool");
        assert_eq!(messages[2]["tool_call_id"], "call_1");
    }

    #[test]
    fn test_parse_text_response() {
        let provider = test_provider();
        let body = json!({
            "choices": [{"message": {"content": "Hello!"}, "finish_reason": "stop"}],
            "usage": {"prompt_tokens": 10, "completion_tokens": 5}
        });
        let resp = provider.parse_response(&body).unwrap();
        assert_eq!(resp.text.as_deref(), Some("Hello!"));
        assert!(resp.tool_calls.is_empty());
        assert_eq!(resp.stop_reason, StopReason::EndTurn);
    }

    #[test]
    fn test_parse_tool_call_response() {
        let provider = test_provider();
        let body = json!({
            "choices": [{
                "message": {
                    "content": null,
                    "tool_calls": [{
                        "id": "call_1",
                        "type": "function",
                        "function": {
                            "name": "get_time",
                            "arguments": "{}"
                        }
                    }]
                },
                "finish_reason": "tool_calls"
            }],
            "usage": {"prompt_tokens": 20, "completion_tokens": 10}
        });
        let resp = provider.parse_response(&body).unwrap();
        assert!(resp.text.is_none());
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].name, "get_time");
        assert_eq!(resp.stop_reason, StopReason::ToolUse);
    }
}
