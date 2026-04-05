use serde::{Deserialize, Serialize};

/// Context sent to the LLM provider
#[derive(Debug, Clone)]
pub struct Context {
    pub system: String,
    pub messages: Vec<Message>,
    pub tool_schemas: Vec<ToolSchema>,
}

impl Context {
    /// Simple context for one-off queries (e.g., memory consolidation)
    pub fn simple_query(prompt: &str) -> Self {
        Self {
            system: String::new(),
            messages: vec![Message::user(prompt)],
            tool_schemas: vec![],
        }
    }
}

/// A single message in the conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: Role,
    pub content: MessageContent,
}

impl Message {
    pub fn user(text: &str) -> Self {
        Self {
            role: Role::User,
            content: MessageContent::Text {
                text: text.to_string(),
            },
        }
    }

    #[allow(dead_code)] // used in tests and future consolidation
    pub fn assistant(text: &str) -> Self {
        Self {
            role: Role::Assistant,
            content: MessageContent::Text {
                text: text.to_string(),
            },
        }
    }

    pub fn assistant_tool_use(text: Option<String>, tool_calls: Vec<ToolCall>) -> Self {
        Self {
            role: Role::Assistant,
            content: MessageContent::ToolUse { text, tool_calls },
        }
    }

    pub fn tool_result(tool_use_id: &str, content: &str) -> Self {
        Self {
            role: Role::Tool,
            content: MessageContent::ToolResult {
                tool_use_id: tool_use_id.to_string(),
                content: content.to_string(),
            },
        }
    }

    pub fn content_text(&self) -> &str {
        match &self.content {
            MessageContent::Text { text } => text,
            MessageContent::ToolUse { text, .. } => text.as_deref().unwrap_or("[tool call]"),
            MessageContent::ToolResult { content, .. } => content,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
    Tool,
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Role::User => write!(f, "user"),
            Role::Assistant => write!(f, "assistant"),
            Role::Tool => write!(f, "tool"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MessageContent {
    Text {
        text: String,
    },
    ToolUse {
        text: Option<String>,
        tool_calls: Vec<ToolCall>,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
    },
}

/// A tool call from the LLM
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Tool schema provided to the LLM
#[derive(Debug, Clone)]
pub struct ToolSchema {
    pub name: String,
    pub description: String,
    pub parameters: serde_json::Value,
}

/// LLM response parsed from provider-specific format
#[derive(Debug)]
pub struct ChatResponse {
    pub text: Option<String>,
    pub tool_calls: Vec<ToolCall>,
    pub stop_reason: StopReason,
    pub usage: Usage,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
}

#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

/// Classified LLM error for retry/failover decisions.
#[derive(Debug, Clone, PartialEq)]
pub enum LlmErrorKind {
    RateLimited,
    ContextTooLong,
    AuthFailed,
    ModelNotFound,
    ServerError,
    Timeout,
    Other,
}

impl LlmErrorKind {
    pub fn classify(status: Option<u16>, body: &str) -> Self {
        let lower = body.to_lowercase();
        if let Some(code) = status {
            match code {
                429 => return Self::RateLimited,
                413 => return Self::ContextTooLong,
                401 | 403 => return Self::AuthFailed,
                404 => return Self::ModelNotFound,
                408 => return Self::Timeout,
                500..=599 => return Self::ServerError,
                _ => {}
            }
        }
        if lower.contains("rate limit")
            || lower.contains("rate_limit")
            || lower.contains("too many requests")
            || lower.contains("quota exceeded")
        {
            return Self::RateLimited;
        }
        if lower.contains("context length")
            || lower.contains("context window")
            || lower.contains("too many tokens")
            || lower.contains("prompt is too long")
        {
            return Self::ContextTooLong;
        }
        if lower.contains("invalid api key")
            || lower.contains("unauthorized")
            || lower.contains("authentication")
        {
            return Self::AuthFailed;
        }
        if lower.contains("model not found")
            || lower.contains("does not exist")
            || lower.contains("model_not_found")
        {
            return Self::ModelNotFound;
        }
        Self::Other
    }

    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::RateLimited | Self::ServerError | Self::Timeout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_roundtrip() {
        let msg = Message::user("hello world");
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.content_text(), "hello world");
        assert_eq!(parsed.role, Role::User);
    }

    #[test]
    fn test_tool_use_message_roundtrip() {
        let msg = Message::assistant_tool_use(
            Some("Let me check.".into()),
            vec![ToolCall {
                id: "call_1".into(),
                name: "get_time".into(),
                arguments: serde_json::json!({}),
            }],
        );
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.role, Role::Assistant);
        if let MessageContent::ToolUse { text, tool_calls } = &parsed.content {
            assert_eq!(text.as_deref(), Some("Let me check."));
            assert_eq!(tool_calls.len(), 1);
            assert_eq!(tool_calls[0].name, "get_time");
        } else {
            panic!("Expected ToolUse content");
        }
    }

    #[test]
    fn test_tool_result_roundtrip() {
        let msg = Message::tool_result("call_1", "Current time: 3:42 PM");
        let json = serde_json::to_string(&msg).unwrap();
        let parsed: Message = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.role, Role::Tool);
        assert_eq!(parsed.content_text(), "Current time: 3:42 PM");
    }

    #[test]
    fn test_context_simple_query() {
        let ctx = Context::simple_query("Summarize this");
        assert!(ctx.system.is_empty());
        assert_eq!(ctx.messages.len(), 1);
        assert_eq!(ctx.messages[0].content_text(), "Summarize this");
    }

    #[test]
    fn test_error_classify_status_codes() {
        assert_eq!(
            LlmErrorKind::classify(Some(429), ""),
            LlmErrorKind::RateLimited
        );
        assert_eq!(
            LlmErrorKind::classify(Some(413), ""),
            LlmErrorKind::ContextTooLong
        );
        assert_eq!(
            LlmErrorKind::classify(Some(401), ""),
            LlmErrorKind::AuthFailed
        );
        assert_eq!(
            LlmErrorKind::classify(Some(404), ""),
            LlmErrorKind::ModelNotFound
        );
        assert_eq!(
            LlmErrorKind::classify(Some(500), ""),
            LlmErrorKind::ServerError
        );
        assert_eq!(
            LlmErrorKind::classify(Some(503), ""),
            LlmErrorKind::ServerError
        );
        assert_eq!(LlmErrorKind::classify(Some(200), "ok"), LlmErrorKind::Other);
    }

    #[test]
    fn test_error_classify_body_patterns() {
        assert_eq!(
            LlmErrorKind::classify(None, "Rate limit exceeded"),
            LlmErrorKind::RateLimited
        );
        assert_eq!(
            LlmErrorKind::classify(None, "maximum context length exceeded"),
            LlmErrorKind::ContextTooLong
        );
        assert_eq!(
            LlmErrorKind::classify(None, "Invalid API key provided"),
            LlmErrorKind::AuthFailed
        );
        assert_eq!(
            LlmErrorKind::classify(None, "The model gpt-5 does not exist"),
            LlmErrorKind::ModelNotFound
        );
    }

    #[test]
    fn test_error_retryable() {
        assert!(LlmErrorKind::RateLimited.is_retryable());
        assert!(LlmErrorKind::ServerError.is_retryable());
        assert!(LlmErrorKind::Timeout.is_retryable());
        assert!(!LlmErrorKind::AuthFailed.is_retryable());
        assert!(!LlmErrorKind::ModelNotFound.is_retryable());
        assert!(!LlmErrorKind::ContextTooLong.is_retryable());
    }
}
