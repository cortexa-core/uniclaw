# Extended Provider System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Extend UniClaw's LLM provider system from 2 to 40+ providers with router, failover, and error classification — matching ZeroClaw/PicoClaw/IronClaw breadth.

**Architecture:** Provider aliases map short names to OpenAI-compatible wrapper configs. Three native backends (Anthropic, Gemini, OpenAI-compatible). ReliableProvider wraps retry/failover. RouterProvider handles hint-based routing.

**Tech Stack:** Rust, async-trait, reqwest, serde_json, tokio

---

### Task 1: Extend LlmProvider trait and types

**Files:**
- Modify: `src/llm/mod.rs`
- Modify: `src/llm/types.rs`

- [ ] **Step 1: Add `name()` and capability methods to LlmProvider trait**

In `src/llm/mod.rs`, change the trait from:

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat(&self, context: &Context) -> Result<ChatResponse>;
}
```

To:

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat(&self, context: &Context) -> Result<ChatResponse>;
    fn name(&self) -> &str;
    fn supports_native_tools(&self) -> bool {
        true
    }
    fn supports_vision(&self) -> bool {
        false
    }
}
```

- [ ] **Step 2: Add LlmErrorKind to types.rs**

Add to `src/llm/types.rs`:

```rust
/// Classified LLM error for retry/failover decisions.
#[derive(Debug, Clone, PartialEq)]
pub enum LlmErrorKind {
    /// 429 or "rate limit" / "too many requests" — retryable with backoff
    RateLimited,
    /// 413 or "context length" / "too many tokens" — caller must truncate
    ContextTooLong,
    /// 401/403 or "invalid api key" — skip to next provider
    AuthFailed,
    /// 404 or "model not found" — skip to next provider
    ModelNotFound,
    /// 5xx — retryable with backoff
    ServerError,
    /// Request timeout or connection error — retryable
    Timeout,
    /// Other unclassified error
    Other,
}

impl LlmErrorKind {
    /// Classify an error from HTTP status code and response body text.
    pub fn classify(status: Option<u16>, body: &str) -> Self {
        let lower = body.to_lowercase();

        // Status-based classification first
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

        // Body text pattern matching
        if lower.contains("rate limit")
            || lower.contains("rate_limit")
            || lower.contains("too many requests")
            || lower.contains("quota exceeded")
            || lower.contains("throttle")
        {
            return Self::RateLimited;
        }
        if lower.contains("context length")
            || lower.contains("context window")
            || lower.contains("too many tokens")
            || lower.contains("maximum.*token")
            || lower.contains("prompt is too long")
        {
            return Self::ContextTooLong;
        }
        if lower.contains("invalid api key")
            || lower.contains("unauthorized")
            || lower.contains("invalid.*key")
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
        matches!(
            self,
            Self::RateLimited | Self::ServerError | Self::Timeout
        )
    }
}
```

- [ ] **Step 3: Add tests for error classification**

Add to `src/llm/types.rs` test module:

```rust
    #[test]
    fn test_error_classify_status_codes() {
        assert_eq!(LlmErrorKind::classify(Some(429), ""), LlmErrorKind::RateLimited);
        assert_eq!(LlmErrorKind::classify(Some(413), ""), LlmErrorKind::ContextTooLong);
        assert_eq!(LlmErrorKind::classify(Some(401), ""), LlmErrorKind::AuthFailed);
        assert_eq!(LlmErrorKind::classify(Some(403), ""), LlmErrorKind::AuthFailed);
        assert_eq!(LlmErrorKind::classify(Some(404), ""), LlmErrorKind::ModelNotFound);
        assert_eq!(LlmErrorKind::classify(Some(500), ""), LlmErrorKind::ServerError);
        assert_eq!(LlmErrorKind::classify(Some(503), ""), LlmErrorKind::ServerError);
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
```

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```
git add src/llm/mod.rs src/llm/types.rs
git commit -m "Extend LlmProvider trait with name() and capabilities, add error classification"
```

---

### Task 2: Provider aliases

**Files:**
- Create: `src/llm/aliases.rs`
- Modify: `src/llm/mod.rs`

- [ ] **Step 1: Create aliases.rs with alias resolution**

Create `src/llm/aliases.rs`:

```rust
/// Resolved provider alias: maps a short name to backend + defaults.
pub struct ProviderAlias {
    /// Backend implementation: "anthropic", "gemini", or "openai_compatible"
    pub backend: &'static str,
    /// Default base URL (can be overridden in config)
    pub base_url: &'static str,
    /// Default env var name for API key (can be overridden in config)
    pub api_key_env: &'static str,
    /// Auth style for the provider
    pub auth_style: AuthStyle,
    /// Extra HTTP headers to send
    pub extra_headers: &'static [(&'static str, &'static str)],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AuthStyle {
    /// Authorization: Bearer <key> (default for most providers)
    Bearer,
    /// x-api-key: <key> (Anthropic)
    XApiKey,
    /// API key as query parameter (Gemini)
    QueryParam,
    /// No authentication (local providers)
    None,
}

/// Resolve a provider name to its alias configuration.
/// Returns None for unknown names — caller should treat as openai_compatible.
pub fn resolve(name: &str) -> Option<ProviderAlias> {
    let alias = match name {
        // --- Native providers ---
        "anthropic" => ProviderAlias {
            backend: "anthropic",
            base_url: "https://api.anthropic.com",
            api_key_env: "ANTHROPIC_API_KEY",
            auth_style: AuthStyle::XApiKey,
            extra_headers: &[],
        },
        "gemini" | "google" | "google-gemini" => ProviderAlias {
            backend: "gemini",
            base_url: "https://generativelanguage.googleapis.com",
            api_key_env: "GEMINI_API_KEY",
            auth_style: AuthStyle::QueryParam,
            extra_headers: &[],
        },

        // --- Tier 1: Major cloud ---
        "openai" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://api.openai.com",
            api_key_env: "OPENAI_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },

        // --- Tier 2: Aggregators / Fast inference ---
        "openrouter" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://openrouter.ai/api",
            api_key_env: "OPENROUTER_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[
                ("HTTP-Referer", "https://github.com/cortexa-core/uniclaw"),
                ("X-Title", "uniclaw"),
            ],
        },
        "groq" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://api.groq.com/openai",
            api_key_env: "GROQ_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },
        "together" | "together-ai" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://api.together.xyz",
            api_key_env: "TOGETHER_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },
        "fireworks" | "fireworks-ai" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://api.fireworks.ai/inference",
            api_key_env: "FIREWORKS_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },
        "cerebras" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://api.cerebras.ai",
            api_key_env: "CEREBRAS_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },
        "perplexity" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://api.perplexity.ai",
            api_key_env: "PERPLEXITY_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },
        "sambanova" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://api.sambanova.ai",
            api_key_env: "SAMBANOVA_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },

        // --- Tier 3: Specialized ---
        "deepseek" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://api.deepseek.com",
            api_key_env: "DEEPSEEK_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },
        "mistral" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://api.mistral.ai",
            api_key_env: "MISTRAL_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },
        "xai" | "grok" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://api.x.ai",
            api_key_env: "XAI_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },
        "cohere" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://api.cohere.com/compatibility",
            api_key_env: "COHERE_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },

        // --- Tier 4: Local / self-hosted ---
        "ollama" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "http://localhost:11434",
            api_key_env: "",
            auth_style: AuthStyle::None,
            extra_headers: &[],
        },
        "lmstudio" | "lm-studio" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "http://localhost:1234",
            api_key_env: "",
            auth_style: AuthStyle::None,
            extra_headers: &[],
        },
        "vllm" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "http://localhost:8000",
            api_key_env: "",
            auth_style: AuthStyle::None,
            extra_headers: &[],
        },
        "litellm" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "http://localhost:4000",
            api_key_env: "",
            auth_style: AuthStyle::None,
            extra_headers: &[],
        },
        "llamacpp" | "llama.cpp" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "http://localhost:8080",
            api_key_env: "",
            auth_style: AuthStyle::None,
            extra_headers: &[],
        },

        // --- Tier 5: Chinese ecosystem ---
        "qwen" | "dashscope" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://dashscope.aliyuncs.com/compatible-mode",
            api_key_env: "DASHSCOPE_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },
        "glm" | "zhipu" | "bigmodel" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://open.bigmodel.cn/api/paas",
            api_key_env: "ZHIPU_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },
        "moonshot" | "kimi" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://api.moonshot.cn",
            api_key_env: "MOONSHOT_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },
        "minimax" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://api.minimax.chat",
            api_key_env: "MINIMAX_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },
        "doubao" | "volcengine" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://ark.cn-beijing.volces.com/api",
            api_key_env: "ARK_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },
        "stepfun" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://api.stepfun.com",
            api_key_env: "STEPFUN_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },
        "baichuan" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://api.baichuan-ai.com",
            api_key_env: "BAICHUAN_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },
        "yi" | "01ai" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://api.01.ai",
            api_key_env: "YI_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },

        // --- Tier 6: Hosting / gateways ---
        "deepinfra" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://api.deepinfra.com/v1/openai",
            api_key_env: "DEEPINFRA_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },
        "huggingface" | "hf" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://api-inference.huggingface.co",
            api_key_env: "HF_API_TOKEN",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },
        "venice" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://api.venice.ai",
            api_key_env: "VENICE_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },
        "nvidia" | "nim" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://integrate.api.nvidia.com",
            api_key_env: "NVIDIA_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },
        "hyperbolic" => ProviderAlias {
            backend: "openai_compatible",
            base_url: "https://api.hyperbolic.xyz",
            api_key_env: "HYPERBOLIC_API_KEY",
            auth_style: AuthStyle::Bearer,
            extra_headers: &[],
        },

        _ => return None,
    };
    Some(alias)
}

/// List all known provider aliases (for help/doctor commands).
pub fn all_aliases() -> &'static [&'static str] {
    &[
        "anthropic", "gemini", "google", "openai",
        "openrouter", "groq", "together", "fireworks", "cerebras",
        "perplexity", "sambanova",
        "deepseek", "mistral", "xai", "grok", "cohere",
        "ollama", "lmstudio", "vllm", "litellm", "llamacpp",
        "qwen", "dashscope", "glm", "zhipu", "bigmodel",
        "moonshot", "kimi", "minimax", "doubao", "volcengine",
        "stepfun", "baichuan", "yi", "01ai",
        "deepinfra", "huggingface", "hf", "venice", "nvidia", "nim",
        "hyperbolic",
    ]
}
```

- [ ] **Step 2: Add aliases module to mod.rs**

Add `pub mod aliases;` to `src/llm/mod.rs`.

- [ ] **Step 3: Add tests for alias resolution**

Add to `src/llm/aliases.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_aliases_resolve() {
        for name in all_aliases() {
            assert!(
                resolve(name).is_some(),
                "Alias '{name}' should resolve"
            );
        }
    }

    #[test]
    fn test_groq_resolves_to_compatible() {
        let alias = resolve("groq").unwrap();
        assert_eq!(alias.backend, "openai_compatible");
        assert_eq!(alias.base_url, "https://api.groq.com/openai");
        assert_eq!(alias.api_key_env, "GROQ_API_KEY");
        assert_eq!(alias.auth_style, AuthStyle::Bearer);
    }

    #[test]
    fn test_gemini_resolves_to_native() {
        let alias = resolve("gemini").unwrap();
        assert_eq!(alias.backend, "gemini");
    }

    #[test]
    fn test_ollama_has_no_auth() {
        let alias = resolve("ollama").unwrap();
        assert_eq!(alias.auth_style, AuthStyle::None);
    }

    #[test]
    fn test_openrouter_has_extra_headers() {
        let alias = resolve("openrouter").unwrap();
        assert!(!alias.extra_headers.is_empty());
    }

    #[test]
    fn test_unknown_returns_none() {
        assert!(resolve("nonexistent_provider_xyz").is_none());
    }

    #[test]
    fn test_multi_aliases_same_backend() {
        let a = resolve("together").unwrap();
        let b = resolve("together-ai").unwrap();
        assert_eq!(a.base_url, b.base_url);
    }
}
```

- [ ] **Step 4: Run tests**

Run: `cargo test -- aliases`
Expected: All alias tests pass.

- [ ] **Step 5: Commit**

```
git add src/llm/aliases.rs src/llm/mod.rs
git commit -m "Add provider alias resolution for 40+ LLM providers"
```

---

### Task 3: Enhance OpenAI-compatible provider with auth styles and extra headers

**Files:**
- Modify: `src/llm/openai.rs`

- [ ] **Step 1: Add auth_style and extra_headers fields to OpenAiProvider**

Extend the struct:

```rust
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
```

Update `new()` to accept the alias info. If a `ProviderAlias` is available, use its `auth_style` and `extra_headers`. Otherwise default to `Bearer` and no extra headers.

```rust
impl OpenAiProvider {
    pub fn new(config: &LlmConfig) -> Result<Self> {
        let alias = crate::llm::aliases::resolve(&config.provider);

        let auth_style = alias
            .as_ref()
            .map(|a| a.auth_style)
            .unwrap_or(crate::llm::aliases::AuthStyle::Bearer);

        let extra_headers: Vec<(String, String)> = alias
            .as_ref()
            .map(|a| {
                a.extra_headers
                    .iter()
                    .map(|(k, v)| (k.to_string(), v.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let base_url = if config.base_url.is_empty() || config.base_url == "https://api.anthropic.com" {
            alias
                .as_ref()
                .map(|a| a.base_url.to_string())
                .unwrap_or_else(|| "https://api.openai.com".to_string())
        } else {
            config.base_url.trim_end_matches('/').to_string()
        };

        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()?;
        Ok(Self {
            client,
            api_key: config.api_key()?,
            base_url,
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            temperature: config.temperature,
            auth_style,
            extra_headers,
            provider_name: config.provider.clone(),
        })
    }
}
```

- [ ] **Step 2: Apply auth_style in the chat() method**

In the `LlmProvider::chat` implementation, replace the hardcoded bearer auth:

```rust
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
    AuthStyle::None => {}
    AuthStyle::QueryParam => {
        // Not used for OpenAI-compatible, but handle gracefully
    }
}

for (key, value) in &self.extra_headers {
    request = request.header(key, value);
}
```

- [ ] **Step 3: Implement name() and trait methods**

```rust
fn name(&self) -> &str {
    &self.provider_name
}
```

- [ ] **Step 4: Update existing tests to still pass**

The existing `test_provider()` helper in the test module creates an OpenAiProvider directly. Update it to set the new fields with defaults. Alternatively, keep the direct construction working by defaulting `auth_style` to Bearer and `extra_headers` to empty.

- [ ] **Step 5: Run tests**

Run: `cargo test`
Expected: All existing OpenAI provider tests pass.

- [ ] **Step 6: Commit**

```
git add src/llm/openai.rs
git commit -m "Enhance OpenAI-compatible provider with auth styles and extra headers"
```

---

### Task 4: Update Anthropic provider with trait methods

**Files:**
- Modify: `src/llm/anthropic.rs`

- [ ] **Step 1: Add name() to AnthropicProvider**

Add to the `LlmProvider` impl:

```rust
fn name(&self) -> &str {
    "anthropic"
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -- anthropic`
Expected: All pass.

- [ ] **Step 3: Commit**

```
git add src/llm/anthropic.rs
git commit -m "Add name() and capabilities to Anthropic provider"
```

---

### Task 5: Google Gemini native provider

**Files:**
- Create: `src/llm/gemini.rs`
- Modify: `src/llm/mod.rs`

- [ ] **Step 1: Implement GeminiProvider**

Create `src/llm/gemini.rs`:

```rust
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

impl GeminiProvider {
    pub fn new(config: &LlmConfig) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()?;

        let base_url = if config.base_url.is_empty()
            || config.base_url == "https://api.anthropic.com"
            || config.base_url == "https://api.openai.com"
        {
            "https://generativelanguage.googleapis.com".to_string()
        } else {
            config.base_url.trim_end_matches('/').to_string()
        };

        Ok(Self {
            client,
            api_key: config.api_key()?,
            base_url,
            model: config.model.clone(),
            max_tokens: config.max_tokens,
            temperature: config.temperature,
        })
    }

    fn serialize_request(&self, context: &Context) -> Value {
        let contents = self.serialize_messages(&context.messages);
        let tools = self.serialize_tools(&context.tool_schemas);

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

        if !tools.is_empty() {
            body["tools"] = json!([{"functionDeclarations": tools}]);
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
                        Role::Tool => continue, // handled below
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
                    result.push(json!({"role": "model", "parts": parts}));
                }
                MessageContent::ToolResult { tool_use_id: _, content } => {
                    result.push(json!({
                        "role": "user",
                        "parts": [{
                            "functionResponse": {
                                "name": "tool",
                                "response": {"result": content}
                            }
                        }]
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
                    "parameters": s.parameters,
                })
            })
            .collect()
    }

    fn parse_response(&self, body: &Value) -> Result<ChatResponse> {
        let candidate = body["candidates"]
            .get(0)
            .ok_or_else(|| anyhow!("No candidates in Gemini response"))?;

        let parts = candidate["content"]["parts"]
            .as_array()
            .ok_or_else(|| anyhow!("No parts in Gemini response"))?;

        let mut text_parts = Vec::new();
        let mut tool_calls = Vec::new();

        for part in parts {
            if let Some(t) = part["text"].as_str() {
                text_parts.push(t.to_string());
            }
            if let Some(fc) = part.get("functionCall") {
                tool_calls.push(ToolCall {
                    id: format!("gemini_{}", tool_calls.len()),
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

    fn name(&self) -> &str {
        "gemini"
    }

    fn supports_vision(&self) -> bool {
        true
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
            system: "You are helpful.".into(),
            messages: vec![Message::user("Hello")],
            tool_schemas: vec![],
        };
        let body = provider.serialize_request(&ctx);
        assert!(body["systemInstruction"]["parts"][0]["text"]
            .as_str()
            .unwrap()
            .contains("helpful"));
        assert_eq!(body["contents"][0]["role"], "user");
        assert_eq!(body["contents"][0]["parts"][0]["text"], "Hello");
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
    }

    #[test]
    fn test_parse_tool_call_response() {
        let provider = test_provider();
        let body = json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "functionCall": {
                            "name": "get_time",
                            "args": {}
                        }
                    }],
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
        assert_eq!(resp.tool_calls.len(), 1);
        assert_eq!(resp.tool_calls[0].name, "get_time");
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
                    id: "call_1".into(),
                    name: "get_time".into(),
                    arguments: json!({}),
                }],
            ),
            Message::tool_result("call_1", "3:42 PM"),
        ];
        let serialized = provider.serialize_messages(&messages);
        assert_eq!(serialized.len(), 3);
        assert_eq!(serialized[0]["role"], "user");
        assert_eq!(serialized[1]["role"], "model");
        assert!(serialized[1]["parts"][1]["functionCall"].is_object());
        assert_eq!(serialized[2]["role"], "user");
        assert!(serialized[2]["parts"][0]["functionResponse"].is_object());
    }
}
```

- [ ] **Step 2: Register gemini module in mod.rs**

Add `pub mod gemini;` to `src/llm/mod.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test -- gemini`
Expected: All Gemini tests pass.

- [ ] **Step 4: Commit**

```
git add src/llm/gemini.rs src/llm/mod.rs
git commit -m "Add native Google Gemini provider"
```

---

### Task 6: ReliableProvider with retry, backoff, and failover

**Files:**
- Create: `src/llm/reliable.rs`
- Modify: `src/llm/mod.rs`

- [ ] **Step 1: Implement ReliableProvider**

Create `src/llm/reliable.rs`:

```rust
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::time::Duration;

use super::types::{ChatResponse, Context, LlmErrorKind};
use super::LlmProvider;

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

    async fn try_provider(
        &self,
        provider: &dyn LlmProvider,
        context: &Context,
    ) -> Result<ChatResponse> {
        let mut last_error = None;

        for attempt in 0..=self.max_retries {
            match provider.chat(context).await {
                Ok(response) => return Ok(response),
                Err(e) => {
                    let error_str = e.to_string();
                    let kind = classify_anyhow_error(&e);

                    if !kind.is_retryable() {
                        tracing::debug!(
                            "Provider '{}' returned non-retryable error: {error_str}",
                            provider.name()
                        );
                        return Err(e);
                    }

                    if attempt < self.max_retries {
                        let backoff_ms = self
                            .base_backoff_ms
                            .saturating_mul(1 << attempt)
                            .min(10_000);
                        tracing::info!(
                            "Provider '{}' attempt {}/{} failed ({}), retrying in {backoff_ms}ms",
                            provider.name(),
                            attempt + 1,
                            self.max_retries + 1,
                            kind_label(&kind),
                        );
                        tokio::time::sleep(Duration::from_millis(backoff_ms)).await;
                    }

                    last_error = Some(e);
                }
            }
        }

        Err(last_error.unwrap_or_else(|| anyhow!("Provider '{}' failed", provider.name())))
    }
}

#[async_trait]
impl LlmProvider for ReliableProvider {
    async fn chat(&self, context: &Context) -> Result<ChatResponse> {
        // Try primary
        match self.try_provider(&*self.primary, context).await {
            Ok(response) => return Ok(response),
            Err(e) => {
                if self.fallbacks.is_empty() {
                    return Err(e);
                }
                tracing::warn!(
                    "Primary provider '{}' failed: {e}, trying fallbacks",
                    self.primary.name()
                );
            }
        }

        // Try fallbacks in order
        let mut errors = vec![format!("Primary ({}): exhausted", self.primary.name())];
        for fallback in &self.fallbacks {
            match self.try_provider(&**fallback, context).await {
                Ok(response) => {
                    tracing::info!(
                        "Fallback provider '{}' succeeded",
                        fallback.name()
                    );
                    return Ok(response);
                }
                Err(e) => {
                    errors.push(format!("{}: {e}", fallback.name()));
                }
            }
        }

        Err(anyhow!(
            "All providers failed:\n  {}",
            errors.join("\n  ")
        ))
    }

    fn name(&self) -> &str {
        "reliable"
    }

    fn supports_native_tools(&self) -> bool {
        self.primary.supports_native_tools()
    }

    fn supports_vision(&self) -> bool {
        self.primary.supports_vision()
    }
}

/// Classify an anyhow::Error by inspecting its Display text for HTTP status codes and patterns.
fn classify_anyhow_error(err: &anyhow::Error) -> LlmErrorKind {
    let msg = err.to_string();

    // Try to extract HTTP status code from error message
    let status = extract_status_code(&msg);
    LlmErrorKind::classify(status, &msg)
}

/// Extract HTTP status code from error message like "API error (429): ..."
fn extract_status_code(msg: &str) -> Option<u16> {
    // Pattern: "(NNN)" where NNN is a 3-digit number
    for window in msg.as_bytes().windows(5) {
        if window[0] == b'(' && window[4] == b')' {
            if let Ok(code) = std::str::from_utf8(&window[1..4])
                .ok()
                .and_then(|s| s.parse::<u16>().ok())
            {
                if (100..=599).contains(&code) {
                    return Some(code);
                }
            }
        }
    }
    None
}

fn kind_label(kind: &LlmErrorKind) -> &'static str {
    match kind {
        LlmErrorKind::RateLimited => "rate-limited",
        LlmErrorKind::ContextTooLong => "context-too-long",
        LlmErrorKind::AuthFailed => "auth-failed",
        LlmErrorKind::ModelNotFound => "model-not-found",
        LlmErrorKind::ServerError => "server-error",
        LlmErrorKind::Timeout => "timeout",
        LlmErrorKind::Other => "other",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    struct MockProvider {
        name: String,
        call_count: Arc<AtomicU32>,
        fail_times: u32,
        error_msg: String,
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        async fn chat(&self, _context: &Context) -> Result<ChatResponse> {
            let count = self.call_count.fetch_add(1, Ordering::SeqCst);
            if count < self.fail_times {
                Err(anyhow!("{}", self.error_msg))
            } else {
                Ok(ChatResponse {
                    text: Some(format!("Response from {}", self.name)),
                    tool_calls: vec![],
                    stop_reason: StopReason::EndTurn,
                    usage: Usage::default(),
                })
            }
        }
        fn name(&self) -> &str {
            &self.name
        }
    }

    #[tokio::test]
    async fn test_reliable_success_first_try() {
        let call_count = Arc::new(AtomicU32::new(0));
        let provider = ReliableProvider::new(
            Box::new(MockProvider {
                name: "primary".into(),
                call_count: call_count.clone(),
                fail_times: 0,
                error_msg: String::new(),
            }),
            vec![],
            2,
            10,
        );

        let ctx = Context::simple_query("test");
        let result = provider.chat(&ctx).await.unwrap();
        assert!(result.text.unwrap().contains("primary"));
        assert_eq!(call_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_reliable_retry_on_server_error() {
        let call_count = Arc::new(AtomicU32::new(0));
        let provider = ReliableProvider::new(
            Box::new(MockProvider {
                name: "primary".into(),
                call_count: call_count.clone(),
                fail_times: 2,
                error_msg: "API error (500): Internal server error".into(),
            }),
            vec![],
            3,
            10, // 10ms backoff for fast tests
        );

        let ctx = Context::simple_query("test");
        let result = provider.chat(&ctx).await.unwrap();
        assert!(result.text.unwrap().contains("primary"));
        assert_eq!(call_count.load(Ordering::SeqCst), 3); // 2 fails + 1 success
    }

    #[tokio::test]
    async fn test_reliable_fallback_on_auth_error() {
        let primary_count = Arc::new(AtomicU32::new(0));
        let fallback_count = Arc::new(AtomicU32::new(0));
        let provider = ReliableProvider::new(
            Box::new(MockProvider {
                name: "primary".into(),
                call_count: primary_count.clone(),
                fail_times: 999,
                error_msg: "API error (401): Invalid API key".into(),
            }),
            vec![Box::new(MockProvider {
                name: "fallback".into(),
                call_count: fallback_count.clone(),
                fail_times: 0,
                error_msg: String::new(),
            })],
            2,
            10,
        );

        let ctx = Context::simple_query("test");
        let result = provider.chat(&ctx).await.unwrap();
        assert!(result.text.unwrap().contains("fallback"));
        // Auth error is non-retryable, so primary called only once
        assert_eq!(primary_count.load(Ordering::SeqCst), 1);
        assert_eq!(fallback_count.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_reliable_all_fail() {
        let provider = ReliableProvider::new(
            Box::new(MockProvider {
                name: "primary".into(),
                call_count: Arc::new(AtomicU32::new(0)),
                fail_times: 999,
                error_msg: "API error (500): Down".into(),
            }),
            vec![Box::new(MockProvider {
                name: "fallback".into(),
                call_count: Arc::new(AtomicU32::new(0)),
                fail_times: 999,
                error_msg: "API error (500): Also down".into(),
            })],
            1,
            10,
        );

        let ctx = Context::simple_query("test");
        let result = provider.chat(&ctx).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("All providers failed"));
    }

    #[test]
    fn test_extract_status_code() {
        assert_eq!(extract_status_code("API error (429): rate limited"), Some(429));
        assert_eq!(extract_status_code("Gemini API error (500): Internal"), Some(500));
        assert_eq!(extract_status_code("no status here"), None);
        assert_eq!(extract_status_code("(abc)"), None);
    }
}
```

- [ ] **Step 2: Register module in mod.rs**

Add `pub mod reliable;` to `src/llm/mod.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test -- reliable`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```
git add src/llm/reliable.rs src/llm/mod.rs
git commit -m "Add ReliableProvider with retry, exponential backoff, and error classification"
```

---

### Task 7: RouterProvider with hint-based routing

**Files:**
- Create: `src/llm/router.rs`
- Modify: `src/llm/mod.rs`

- [ ] **Step 1: Implement RouterProvider**

Create `src/llm/router.rs`:

```rust
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::collections::HashMap;

use super::types::{ChatResponse, Context, Message, MessageContent};
use super::LlmProvider;

/// Routes requests to different providers based on model hint prefixes.
/// Model string "hint:fast" → resolves to configured fast provider + model.
/// Non-hint models pass through to the default provider.
pub struct RouterProvider {
    providers: HashMap<String, Box<dyn LlmProvider>>,
    routes: HashMap<String, (String, String)>, // hint → (provider_name, model)
    default_name: String,
}

impl RouterProvider {
    pub fn new(
        providers: HashMap<String, Box<dyn LlmProvider>>,
        routes: HashMap<String, (String, String)>,
        default_name: String,
    ) -> Self {
        Self {
            providers,
            routes,
            default_name,
        }
    }

    fn resolve(&self, model: &str) -> (&str, &str) {
        if let Some(hint) = model.strip_prefix("hint:") {
            if let Some((provider_name, resolved_model)) = self.routes.get(hint) {
                return (provider_name, resolved_model);
            }
            tracing::warn!("Unknown routing hint '{hint}', using default provider");
        }
        (&self.default_name, model)
    }
}

#[async_trait]
impl LlmProvider for RouterProvider {
    async fn chat(&self, context: &Context) -> Result<ChatResponse> {
        // Extract model from the context (last user message or context metadata)
        // For now, route based on default — the agent loop doesn't set model hints yet.
        // The router is useful when the system is extended with model selection.
        let (provider_name, _model) = self.resolve(&self.default_name);

        let provider = self
            .providers
            .get(provider_name)
            .ok_or_else(|| anyhow!("Router: provider '{provider_name}' not found"))?;

        provider.chat(context).await
    }

    fn name(&self) -> &str {
        "router"
    }

    fn supports_native_tools(&self) -> bool {
        self.providers
            .get(&self.default_name)
            .map(|p| p.supports_native_tools())
            .unwrap_or(true)
    }

    fn supports_vision(&self) -> bool {
        self.providers
            .get(&self.default_name)
            .map(|p| p.supports_vision())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::*;

    struct StubProvider {
        label: String,
    }

    #[async_trait]
    impl LlmProvider for StubProvider {
        async fn chat(&self, _ctx: &Context) -> Result<ChatResponse> {
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
    }

    #[test]
    fn test_resolve_hint() {
        let mut providers: HashMap<String, Box<dyn LlmProvider>> = HashMap::new();
        providers.insert("fast".into(), Box::new(StubProvider { label: "fast".into() }));
        providers.insert("smart".into(), Box::new(StubProvider { label: "smart".into() }));

        let mut routes = HashMap::new();
        routes.insert("fast".to_string(), ("fast".to_string(), "llama-3".to_string()));
        routes.insert("reasoning".to_string(), ("smart".to_string(), "claude-4".to_string()));

        let router = RouterProvider::new(providers, routes, "smart".into());

        let (name, model) = router.resolve("hint:fast");
        assert_eq!(name, "fast");
        assert_eq!(model, "llama-3");

        let (name, model) = router.resolve("hint:reasoning");
        assert_eq!(name, "smart");
        assert_eq!(model, "claude-4");

        // Unknown hint falls through to default
        let (name, model) = router.resolve("hint:unknown");
        assert_eq!(name, "smart");
        assert_eq!(model, "hint:unknown");
    }

    #[test]
    fn test_resolve_no_hint() {
        let mut providers: HashMap<String, Box<dyn LlmProvider>> = HashMap::new();
        providers.insert("default".into(), Box::new(StubProvider { label: "default".into() }));

        let router = RouterProvider::new(providers, HashMap::new(), "default".into());
        let (name, model) = router.resolve("gpt-4o");
        assert_eq!(name, "default");
        assert_eq!(model, "gpt-4o");
    }
}
```

- [ ] **Step 2: Register module in mod.rs**

Add `pub mod router;` to `src/llm/mod.rs`.

- [ ] **Step 3: Run tests**

Run: `cargo test -- router`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```
git add src/llm/router.rs src/llm/mod.rs
git commit -m "Add RouterProvider for hint-based multi-provider model routing"
```

---

### Task 8: Update config, factory, and agent loop

**Files:**
- Modify: `src/config.rs`
- Modify: `src/llm/mod.rs`
- Modify: `src/agent/loop.rs`
- Modify: `src/main.rs`

- [ ] **Step 1: Extend config with new fields**

In `src/config.rs`, add to `LlmConfig`:

```rust
pub struct LlmConfig {
    // ... existing fields ...
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    #[serde(default = "default_backoff")]
    pub base_backoff_ms: u64,
}
```

Add to `Config`:

```rust
pub struct Config {
    // ... existing fields ...
    #[serde(default)]
    pub extra_providers: Vec<NamedProviderConfig>,
    #[serde(default)]
    pub routes: Vec<RouteConfig>,
}
```

Add new structs:

```rust
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct NamedProviderConfig {
    pub name: String,
    #[serde(flatten)]
    pub llm: LlmConfig,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct RouteConfig {
    pub hint: String,
    pub use_provider: String,
}
```

Add default functions:

```rust
fn default_max_retries() -> u32 { 2 }
fn default_backoff() -> u64 { 200 }
```

- [ ] **Step 2: Update the provider factory in mod.rs**

Replace `create_provider`:

```rust
pub fn create_provider(config: &LlmConfig) -> Result<Box<dyn LlmProvider>> {
    let alias = aliases::resolve(&config.provider);
    let backend = alias
        .as_ref()
        .map(|a| a.backend)
        .unwrap_or(&config.provider);

    match backend {
        "anthropic" => Ok(Box::new(anthropic::AnthropicProvider::new(config)?)),
        "gemini" => Ok(Box::new(gemini::GeminiProvider::new(config)?)),
        "openai_compatible" | "openai" => Ok(Box::new(openai::OpenAiProvider::new(config)?)),
        other => {
            // Unknown backend — try as openai_compatible (many providers work this way)
            tracing::info!("Unknown provider '{other}', trying as OpenAI-compatible");
            Ok(Box::new(openai::OpenAiProvider::new(config)?))
        }
    }
}
```

- [ ] **Step 3: Simplify Agent by removing manual failover**

In `src/agent/loop.rs`, remove the `fallback_llm` field and `call_llm` method. The `Agent` struct becomes:

```rust
pub struct Agent {
    llm: Box<dyn LlmProvider>,  // ReliableProvider handles failover
    pub tool_registry: ToolRegistry,
    pub memory: MemoryManager,
    pub session_store: SessionStore,
    context_builder: ContextBuilder,
    config: AgentConfig,
    data_dir: PathBuf,
    full_config: Arc<Config>,
}
```

`Agent::new` no longer takes `fallback_llm`. In `process_inner`, replace `self.call_llm(&context)` with `self.llm.chat(&context)`.

Remove the `call_llm` method entirely.

- [ ] **Step 4: Update main.rs to build ReliableProvider**

In `create_agent()` in `src/main.rs`, replace the current provider construction with:

```rust
// Build primary provider
let primary = create_provider(&config.llm)?;

// Build fallback chain
let mut fallbacks: Vec<Box<dyn LlmProvider>> = Vec::new();
if let Some(ref fallback_config) = config.llm.fallback {
    fallbacks.push(create_provider(fallback_config)?);
}

// Wrap in ReliableProvider
let llm: Box<dyn LlmProvider> = Box::new(ReliableProvider::new(
    primary,
    fallbacks,
    config.llm.max_retries,
    config.llm.base_backoff_ms,
));

let agent = Agent::new(llm, tool_registry, &config, data_dir).await;
```

Update `Agent::new` signature to remove `fallback_llm` parameter.

- [ ] **Step 5: Update tests that construct Agent**

In `tests/agent_test.rs` (and anywhere else `Agent::new` is called), remove the `fallback_llm` parameter. The mock LLM is passed directly — no reliable wrapping needed for tests.

- [ ] **Step 6: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 7: Commit**

```
git add src/config.rs src/llm/mod.rs src/agent/loop.rs src/main.rs tests/
git commit -m "Integrate provider aliases, reliable failover, and router into config and agent"
```

---

### Task 9: Final verification

**Files:** None (verification only)

- [ ] **Step 1: Run full test suite**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 2: Run with telegram feature**

Run: `cargo test --features telegram`
Expected: All tests pass.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings.

- [ ] **Step 4: Run fmt**

Run: `cargo fmt`

- [ ] **Step 5: Verify config backward compatibility**

Verify the existing `config/config.toml` still loads correctly:

Run: `cargo build && ./target/debug/uniclaw --help`
Expected: Builds and runs without error.

- [ ] **Step 6: Fix any issues and commit**

```
git add -A
git commit -m "Fix clippy warnings and formatting for provider extension"
```

---

## Summary

| Task | New/Modified Files | What it does |
|------|-------------------|--------------|
| 1 | types.rs, mod.rs | Extend trait + error classification |
| 2 | aliases.rs, mod.rs | 40+ provider alias resolution |
| 3 | openai.rs | Auth styles, extra headers, quirks |
| 4 | anthropic.rs | Add name() method |
| 5 | gemini.rs, mod.rs | Native Google Gemini provider |
| 6 | reliable.rs, mod.rs | Retry, backoff, failover chain |
| 7 | router.rs, mod.rs | Hint-based multi-provider routing |
| 8 | config.rs, mod.rs, loop.rs, main.rs | Wire everything together |
| 9 | — | Final verification |

**Total estimated new code**: ~1,200 LOC
**New provider count**: 40+ (3 native backends + aliases)
**Commits**: 9
