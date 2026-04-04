use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tracing::{info, warn};

use super::LlmProvider;
use super::types::{ChatResponse, Context, LlmErrorKind};

/// Wraps a primary provider + fallbacks with retry and exponential backoff.
pub struct ReliableProvider {
    primary: Box<dyn LlmProvider>,
    fallbacks: Vec<Box<dyn LlmProvider>>,
    max_retries: u32,
    base_backoff_ms: u64,
}

impl ReliableProvider {
    pub fn new(
        primary: Box<dyn LlmProvider>,
        fallbacks: Vec<Box<dyn LlmProvider>>,
        max_retries: u32,
        base_backoff_ms: u64,
    ) -> Self {
        Self {
            primary,
            fallbacks,
            max_retries,
            base_backoff_ms,
        }
    }

    /// Try a single provider with retry loop for retryable errors.
    /// Returns Ok(response) on success, or Err with the last error.
    async fn try_provider(
        &self,
        provider: &dyn LlmProvider,
        context: &Context,
    ) -> Result<ChatResponse> {
        let mut last_error = None;

        for attempt in 0..self.max_retries {
            match provider.chat(context).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    let kind = classify_anyhow_error(&e);

                    if !kind.is_retryable() {
                        warn!(
                            provider = provider.name(),
                            error = %e,
                            kind = ?kind,
                            "Non-retryable error, skipping provider"
                        );
                        return Err(e);
                    }

                    let backoff_ms =
                        std::cmp::min(self.base_backoff_ms * 2u64.pow(attempt), 10_000);
                    warn!(
                        provider = provider.name(),
                        attempt = attempt + 1,
                        max = self.max_retries,
                        backoff_ms,
                        error = %e,
                        "Retryable error, backing off"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("Provider {} failed with no attempts", provider.name())))
    }
}

#[async_trait]
impl LlmProvider for ReliableProvider {
    async fn chat(&self, context: &Context) -> Result<ChatResponse> {
        let mut errors: Vec<String> = Vec::new();

        // Try primary
        match self.try_provider(self.primary.as_ref(), context).await {
            Ok(resp) => return Ok(resp),
            Err(e) => {
                errors.push(format!("{}: {}", self.primary.name(), e));
            }
        }

        // Try fallbacks
        for fallback in &self.fallbacks {
            info!(
                fallback = fallback.name(),
                "Primary failed, trying fallback"
            );
            match self.try_provider(fallback.as_ref(), context).await {
                Ok(resp) => return Ok(resp),
                Err(e) => {
                    errors.push(format!("{}: {}", fallback.name(), e));
                }
            }
        }

        Err(anyhow!(
            "All providers failed:\n  {}",
            errors.join("\n  ")
        ))
    }

    fn name(&self) -> &str {
        self.primary.name()
    }

    fn supports_native_tools(&self) -> bool {
        self.primary.supports_native_tools()
    }

    fn supports_vision(&self) -> bool {
        self.primary.supports_vision()
    }
}

/// Extract an HTTP status code from an anyhow error message.
/// Looks for patterns like "(429)" or "(500)".
fn extract_status_code(msg: &str) -> Option<u16> {
    // Find pattern: "(" followed by 3 digits followed by ")"
    let bytes = msg.as_bytes();
    for i in 0..bytes.len().saturating_sub(4) {
        if bytes[i] == b'('
            && bytes[i + 1].is_ascii_digit()
            && bytes[i + 2].is_ascii_digit()
            && bytes[i + 3].is_ascii_digit()
            && bytes[i + 4] == b')'
        {
            let code_str = &msg[i + 1..i + 4];
            if let Ok(code) = code_str.parse::<u16>() {
                return Some(code);
            }
        }
    }
    None
}

/// Classify an anyhow error by extracting status code and examining the message.
fn classify_anyhow_error(error: &anyhow::Error) -> LlmErrorKind {
    let msg = error.to_string();
    let status = extract_status_code(&msg);
    LlmErrorKind::classify(status, &msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::{ChatResponse, StopReason, Usage};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    struct MockProvider {
        name: String,
        call_count: Arc<AtomicU32>,
        fail_times: u32,
        error_msg: String,
    }

    impl MockProvider {
        fn new(name: &str, fail_times: u32, error_msg: &str) -> Self {
            Self {
                name: name.to_string(),
                call_count: Arc::new(AtomicU32::new(0)),
                fail_times,
                error_msg: error_msg.to_string(),
            }
        }

        #[allow(dead_code)]
        fn calls(&self) -> u32 {
            self.call_count.load(Ordering::SeqCst)
        }
    }

    fn mock_success_response() -> ChatResponse {
        ChatResponse {
            text: Some("Hello!".to_string()),
            tool_calls: vec![],
            stop_reason: StopReason::EndTurn,
            usage: Usage::default(),
        }
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn chat(&self, _context: &Context) -> Result<ChatResponse> {
            let count = self.call_count.fetch_add(1, Ordering::SeqCst);
            if count < self.fail_times {
                Err(anyhow!("{}", self.error_msg))
            } else {
                Ok(mock_success_response())
            }
        }

        fn name(&self) -> &str {
            &self.name
        }
    }

    fn test_context() -> Context {
        Context::simple_query("test")
    }

    #[tokio::test]
    async fn test_reliable_success_first_try() {
        let primary = MockProvider::new("primary", 0, "");
        let call_count = primary.call_count.clone();

        let reliable = ReliableProvider::new(Box::new(primary), vec![], 3, 10);

        let resp = reliable.chat(&test_context()).await.unwrap();
        assert_eq!(resp.text.as_deref(), Some("Hello!"));
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_reliable_retry_on_server_error() {
        let primary = MockProvider::new("primary", 2, "API error (500): internal server error");
        let call_count = primary.call_count.clone();

        let reliable = ReliableProvider::new(Box::new(primary), vec![], 3, 10);

        let resp = reliable.chat(&test_context()).await.unwrap();
        assert_eq!(resp.text.as_deref(), Some("Hello!"));
        assert_eq!(call_count.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_reliable_fallback_on_auth_error() {
        let primary = MockProvider::new("primary", 100, "API error (401): unauthorized");
        let primary_calls = primary.call_count.clone();

        let fallback = MockProvider::new("fallback", 0, "");
        let fallback_calls = fallback.call_count.clone();

        let reliable =
            ReliableProvider::new(Box::new(primary), vec![Box::new(fallback)], 3, 10);

        let resp = reliable.chat(&test_context()).await.unwrap();
        assert_eq!(resp.text.as_deref(), Some("Hello!"));
        // Auth error is non-retryable: primary called only once
        assert_eq!(primary_calls.load(Ordering::SeqCst), 1);
        assert_eq!(fallback_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_reliable_all_fail() {
        let primary = MockProvider::new("primary", 100, "API error (500): server error");
        let fallback = MockProvider::new("fallback", 100, "API error (500): server error");

        let reliable =
            ReliableProvider::new(Box::new(primary), vec![Box::new(fallback)], 2, 10);

        let err = reliable.chat(&test_context()).await.unwrap_err();
        assert!(
            err.to_string().contains("All providers failed"),
            "Error was: {}",
            err
        );
    }

    #[test]
    fn test_extract_status_code() {
        assert_eq!(
            extract_status_code("API error (429): rate limited"),
            Some(429)
        );
        assert_eq!(
            extract_status_code("API error (500): internal server error"),
            Some(500)
        );
        assert_eq!(extract_status_code("no status code here"), None);
        assert_eq!(
            extract_status_code("(401) unauthorized"),
            Some(401)
        );
    }
}
