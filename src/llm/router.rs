use std::collections::HashMap;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use tracing::warn;

use super::types::{ChatResponse, Context};
use super::LlmProvider;

/// Routes requests to different providers based on model hint prefixes.
///
/// Model strings like "hint:fast" or "hint:reasoning" are resolved via a
/// routes table to a specific (provider, model) pair. Plain model strings
/// (no "hint:" prefix) go to the default provider unchanged.
impl std::fmt::Debug for RouterProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RouterProvider")
            .field("providers", &self.providers.keys().collect::<Vec<_>>())
            .field("routes", &self.routes)
            .field("default_name", &self.default_name)
            .finish()
    }
}

#[allow(dead_code)]
pub struct RouterProvider {
    providers: HashMap<String, Box<dyn LlmProvider>>,
    routes: HashMap<String, (String, String)>, // hint → (provider_name, model)
    default_name: String,
}

#[allow(dead_code)]
impl RouterProvider {
    pub fn new(
        providers: HashMap<String, Box<dyn LlmProvider>>,
        routes: HashMap<String, (String, String)>,
        default_name: String,
    ) -> Result<Self> {
        if !providers.contains_key(&default_name) {
            return Err(anyhow!(
                "Default provider '{}' not found in providers map",
                default_name
            ));
        }
        // Validate that all routes point to known providers
        for (hint, (provider_name, _)) in &routes {
            if !providers.contains_key(provider_name) {
                return Err(anyhow!(
                    "Route 'hint:{}' references unknown provider '{}'",
                    hint,
                    provider_name
                ));
            }
        }
        Ok(Self {
            providers,
            routes,
            default_name,
        })
    }

    /// Resolve a model string to (provider_name, resolved_model).
    ///
    /// - "hint:fast" → looks up "fast" in routes table
    /// - "hint:unknown" → warns, falls back to default provider with original model stripped
    /// - "gpt-4o" → default provider, model unchanged
    pub fn resolve(&self, model: &str) -> (String, String) {
        if let Some(hint) = model.strip_prefix("hint:") {
            if let Some((provider_name, resolved_model)) = self.routes.get(hint) {
                return (provider_name.clone(), resolved_model.clone());
            }
            warn!(
                hint = hint,
                default = %self.default_name,
                "Unknown hint, falling back to default provider"
            );
            return (self.default_name.clone(), hint.to_string());
        }
        (self.default_name.clone(), model.to_string())
    }

    fn default_provider(&self) -> &dyn LlmProvider {
        self.providers[&self.default_name].as_ref()
    }
}

#[async_trait]
impl LlmProvider for RouterProvider {
    async fn chat(&self, context: &Context) -> Result<ChatResponse> {
        // Router delegates to the default provider for direct chat calls.
        self.default_provider().chat(context).await
    }

    fn name(&self) -> &str {
        "router"
    }

    fn supports_native_tools(&self) -> bool {
        self.default_provider().supports_native_tools()
    }

    fn supports_vision(&self) -> bool {
        self.default_provider().supports_vision()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::{ChatResponse, StopReason, Usage};

    struct StubProvider {
        label: String,
    }

    impl StubProvider {
        fn new(label: &str) -> Self {
            Self {
                label: label.to_string(),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for StubProvider {
        async fn chat(&self, _context: &Context) -> Result<ChatResponse> {
            Ok(ChatResponse {
                text: Some(format!("from-{}", self.label)),
                tool_calls: vec![],
                stop_reason: StopReason::EndTurn,
                usage: Usage::default(),
            })
        }

        fn name(&self) -> &str {
            &self.label
        }

        fn supports_native_tools(&self) -> bool {
            self.label == "anthropic"
        }

        fn supports_vision(&self) -> bool {
            self.label == "anthropic"
        }
    }

    fn build_router() -> RouterProvider {
        let mut providers: HashMap<String, Box<dyn LlmProvider>> = HashMap::new();
        providers.insert(
            "anthropic".to_string(),
            Box::new(StubProvider::new("anthropic")),
        );
        providers.insert("openai".to_string(), Box::new(StubProvider::new("openai")));

        let mut routes: HashMap<String, (String, String)> = HashMap::new();
        routes.insert(
            "fast".to_string(),
            ("openai".to_string(), "gpt-4o-mini".to_string()),
        );
        routes.insert(
            "reasoning".to_string(),
            (
                "anthropic".to_string(),
                "claude-sonnet-4-20250514".to_string(),
            ),
        );

        RouterProvider::new(providers, routes, "anthropic".to_string()).unwrap()
    }

    #[test]
    fn test_resolve_hint() {
        let router = build_router();

        let (provider, model) = router.resolve("hint:fast");
        assert_eq!(provider, "openai");
        assert_eq!(model, "gpt-4o-mini");

        let (provider, model) = router.resolve("hint:reasoning");
        assert_eq!(provider, "anthropic");
        assert_eq!(model, "claude-sonnet-4-20250514");

        // Unknown hint falls back to default
        let (provider, model) = router.resolve("hint:unknown");
        assert_eq!(provider, "anthropic");
        assert_eq!(model, "unknown");
    }

    #[test]
    fn test_resolve_no_hint() {
        let router = build_router();

        let (provider, model) = router.resolve("gpt-4o");
        assert_eq!(provider, "anthropic"); // default provider
        assert_eq!(model, "gpt-4o"); // model unchanged
    }

    #[tokio::test]
    async fn test_chat_delegates_to_default() {
        let router = build_router();
        let ctx = Context::simple_query("hello");
        let resp = router.chat(&ctx).await.unwrap();
        assert_eq!(resp.text.as_deref(), Some("from-anthropic"));
    }

    #[test]
    fn test_trait_delegation() {
        let router = build_router();
        assert_eq!(router.name(), "router");
        assert!(router.supports_native_tools()); // anthropic stub returns true
        assert!(router.supports_vision()); // anthropic stub returns true
    }

    #[test]
    fn test_invalid_default_provider() {
        let providers: HashMap<String, Box<dyn LlmProvider>> = HashMap::new();
        let routes: HashMap<String, (String, String)> = HashMap::new();
        let err = RouterProvider::new(providers, routes, "missing".to_string()).unwrap_err();
        assert!(err.to_string().contains("not found"));
    }
}
