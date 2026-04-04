# Extended LLM Provider System Design

**Date**: 2026-04-04
**Goal**: Match ZeroClaw/PicoClaw/IronClaw provider breadth with UniClaw's lean architecture

## Current State

UniClaw has 2 providers (~600 LOC):
- `AnthropicProvider` — native Anthropic Messages API
- `OpenAiProvider` — OpenAI Chat Completions API (also handles any compatible endpoint)

Simple failover: primary → one fallback, hardcoded in `Agent::call_llm()`.

## Design Principles

1. **90% of providers are OpenAI-compatible** — different base_url + api_key. Don't write per-provider code when config suffices.
2. **Only write native providers for truly different wire formats** — Anthropic (have), Gemini (new). Everything else routes through the enhanced OpenAI-compatible wrapper.
3. **Infrastructure over quantity** — Router, Reliable, Error Classification matter more than 50 provider files with copy-paste code.
4. **Backward compatible** — existing `[llm]` config works unchanged.

## Architecture

```
                    ┌─────────────┐
                    │   Router    │ ← optional, hint-based (fast/reasoning/vision)
                    │  provider   │
                    └──────┬──────┘
                           │
                    ┌──────▼──────┐
                    │  Reliable   │ ← retry, backoff, error classification
                    │  provider   │    wraps primary + fallback chain
                    └──────┬──────┘
                           │
              ┌────────────┼────────────┐
              ▼            ▼            ▼
        ┌──────────┐ ┌──────────┐ ┌──────────┐
        │Anthropic │ │ Gemini   │ │OpenAI-   │
        │ (native) │ │ (native) │ │compatible│
        └──────────┘ └──────────┘ └──────────┘
                                       │
                          ┌────────────┼─── ... ───┐
                          ▼            ▼            ▼
                       OpenAI      Groq       40+ aliases
```

## File Structure

```
src/llm/
  mod.rs              — LlmProvider trait, create_provider factory (MODIFY)
  types.rs            — Add LlmError enum (MODIFY)
  anthropic.rs        — Add name(), supports_* methods (MODIFY)
  openai.rs           — Add ProviderQuirks, extra_headers, auth styles (MODIFY)
  gemini.rs           — Google Gemini generateContent API (NEW ~250 LOC)
  reliable.rs         — ReliableProvider: retry, backoff, error classify, failover (NEW ~300 LOC)
  router.rs           — RouterProvider: hint-based model routing (NEW ~150 LOC)
  aliases.rs          — Provider alias resolution + default URLs/env vars (NEW ~120 LOC)
src/config.rs         — Add reliability, extra_providers, routes config (MODIFY)
src/agent/loop.rs     — Remove manual failover, use ReliableProvider (MODIFY)
src/main.rs           — Updated provider construction (MODIFY)
```

**Estimated new code**: ~1,200 LOC
**Total LLM module after**: ~1,800 LOC

## 1. LlmProvider Trait Extension

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat(&self, context: &Context) -> Result<ChatResponse>;
    fn name(&self) -> &str;
    fn supports_native_tools(&self) -> bool { true }
    fn supports_vision(&self) -> bool { false }
}
```

Only `chat()` and `name()` are required. Capability methods have defaults.

## 2. Provider Aliases (40+ providers)

The alias system maps a short name to (backend, base_url, default_api_key_env):

| Alias | Backend | Base URL | Default API Key Env |
|-------|---------|----------|---------------------|
| `anthropic` | anthropic | https://api.anthropic.com | ANTHROPIC_API_KEY |
| `gemini` / `google` | gemini | https://generativelanguage.googleapis.com | GEMINI_API_KEY |
| `openai` | openai_compatible | https://api.openai.com | OPENAI_API_KEY |
| `openrouter` | openai_compatible | https://openrouter.ai/api | OPENROUTER_API_KEY |
| `groq` | openai_compatible | https://api.groq.com/openai | GROQ_API_KEY |
| `deepseek` | openai_compatible | https://api.deepseek.com | DEEPSEEK_API_KEY |
| `mistral` | openai_compatible | https://api.mistral.ai | MISTRAL_API_KEY |
| `xai` / `grok` | openai_compatible | https://api.x.ai | XAI_API_KEY |
| `together` / `together-ai` | openai_compatible | https://api.together.xyz | TOGETHER_API_KEY |
| `fireworks` / `fireworks-ai` | openai_compatible | https://api.fireworks.ai/inference | FIREWORKS_API_KEY |
| `cerebras` | openai_compatible | https://api.cerebras.ai | CEREBRAS_API_KEY |
| `perplexity` | openai_compatible | https://api.perplexity.ai | PERPLEXITY_API_KEY |
| `cohere` | openai_compatible | https://api.cohere.com/compatibility | COHERE_API_KEY |
| `ollama` | openai_compatible | http://localhost:11434 | (none) |
| `lmstudio` / `lm-studio` | openai_compatible | http://localhost:1234 | (none) |
| `vllm` | openai_compatible | http://localhost:8000 | (none) |
| `litellm` | openai_compatible | http://localhost:4000 | (none) |
| `llamacpp` / `llama.cpp` | openai_compatible | http://localhost:8080 | (none) |
| `qwen` / `dashscope` | openai_compatible | https://dashscope.aliyuncs.com/compatible-mode | DASHSCOPE_API_KEY |
| `glm` / `zhipu` / `bigmodel` | openai_compatible | https://open.bigmodel.cn/api/paas | ZHIPU_API_KEY |
| `moonshot` / `kimi` | openai_compatible | https://api.moonshot.cn | MOONSHOT_API_KEY |
| `minimax` | openai_compatible | https://api.minimax.chat | MINIMAX_API_KEY |
| `doubao` / `volcengine` | openai_compatible | https://ark.cn-beijing.volces.com/api | ARK_API_KEY |
| `stepfun` | openai_compatible | https://api.stepfun.com | STEPFUN_API_KEY |
| `baichuan` | openai_compatible | https://api.baichuan-ai.com | BAICHUAN_API_KEY |
| `yi` / `01ai` | openai_compatible | https://api.01.ai | YI_API_KEY |
| `deepinfra` | openai_compatible | https://api.deepinfra.com/v1/openai | DEEPINFRA_API_KEY |
| `huggingface` / `hf` | openai_compatible | https://api-inference.huggingface.co | HF_API_TOKEN |
| `venice` | openai_compatible | https://api.venice.ai | VENICE_API_KEY |
| `nvidia` / `nim` | openai_compatible | https://integrate.api.nvidia.com | NVIDIA_API_KEY |
| `sambanova` | openai_compatible | https://api.sambanova.ai | SAMBANOVA_API_KEY |
| `hyperbolic` | openai_compatible | https://api.hyperbolic.xyz | HYPERBOLIC_API_KEY |

User config: `provider = "groq"` → resolved to openai_compatible with correct URL/key.
User can still override `base_url` and `api_key_env` for custom endpoints.

## 3. Provider Quirks

Some OpenAI-compatible providers need small deviations. Handled via config, not per-provider files:

```rust
pub struct ProviderQuirks {
    pub auth_style: AuthStyle,
    pub extra_headers: Vec<(String, String)>,
    pub merge_system_into_user: bool,
    pub api_path: String,  // default: "/v1/chat/completions"
}

pub enum AuthStyle {
    Bearer,         // Authorization: Bearer <key> (default, most providers)
    ApiKey,         // x-api-key: <key> (Anthropic)
    NoAuth,         // No auth header (local providers like Ollama)
}
```

Built-in quirks per alias:
- `openrouter`: extra headers `HTTP-Referer`, `X-Title`
- `ollama`, `lmstudio`, `vllm`, `llamacpp`, `litellm`: `AuthStyle::NoAuth`
- `minimax`: `merge_system_into_user: true`
- All others: defaults (Bearer auth, standard path)

## 4. Gemini Native Provider

Google Gemini uses `generateContent` API with a different message format:
- Roles: `user` / `model` (not `assistant`)
- Content: `parts` array (not `content` string)
- Tools: `functionDeclarations` (not OpenAI-style)
- Auth: `?key=API_KEY` query param (not header)
- Endpoint: `https://generativelanguage.googleapis.com/v1beta/models/{model}:generateContent`

~250 LOC implementation paralleling anthropic.rs structure.

## 5. Error Classification

```rust
pub enum LlmErrorKind {
    RateLimited,       // 429, "rate limit", "too many requests"
    ContextTooLong,    // 413, "context length", "too many tokens"
    AuthFailed,        // 401, 403, "invalid api key", "unauthorized"
    ModelNotFound,     // 404, "model not found", "does not exist"
    ServerError,       // 5xx
    Timeout,           // request timeout
    Network,           // connection errors
    Other,
}

impl LlmErrorKind {
    pub fn classify(status: u16, body: &str) -> Self { ... }
    pub fn is_retryable(&self) -> bool {
        matches!(self, Self::RateLimited | Self::ServerError | Self::Timeout | Self::Network)
    }
}
```

Classification order: HTTP status first, then body text pattern matching (case-insensitive).

## 6. ReliableProvider

Replaces the manual `call_llm()` in `Agent`:

```rust
pub struct ReliableProvider {
    primary: Box<dyn LlmProvider>,
    fallbacks: Vec<Box<dyn LlmProvider>>,
    max_retries: u32,       // default 2
    base_backoff_ms: u64,   // default 200
}
```

**Retry logic:**
```
for provider in [primary, ...fallbacks]:
    for attempt in 0..max_retries:
        result = provider.chat(context)
        match classify(result):
            Ok(response) → return Ok(response)
            RateLimited → sleep(backoff * 2^attempt), continue retry
            ServerError/Timeout/Network → sleep(backoff * 2^attempt), continue retry
            ContextTooLong → return Err (caller must truncate)
            AuthFailed/ModelNotFound → break to next provider
            Other → break to next provider
    // All retries exhausted for this provider → try next
return Err("All providers failed: {details}")
```

Backoff: `min(base_backoff_ms * 2^attempt, 10_000)` — capped at 10 seconds.

## 7. RouterProvider (Optional)

Only created when `[[llm.routes]]` are configured:

```rust
pub struct RouterProvider {
    providers: HashMap<String, Box<dyn LlmProvider>>,
    routes: HashMap<String, (String, String)>,  // hint → (provider_name, model)
    default_name: String,
}
```

Agent sends `model = "hint:fast"` → Router resolves to configured provider + model.
Non-hint models pass through to the default provider unchanged.

## 8. Config Changes

New fields in `LlmConfig` (all optional, backward compatible):

```rust
pub struct LlmConfig {
    // ... existing fields unchanged ...
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,           // NEW, default 2
    #[serde(default = "default_backoff")]
    pub base_backoff_ms: u64,       // NEW, default 200
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,  // NEW
}
```

New top-level config sections (optional):

```rust
pub struct Config {
    // ... existing fields ...
    #[serde(default)]
    pub extra_providers: Vec<NamedProviderConfig>,  // NEW
    #[serde(default)]
    pub routes: Vec<RouteConfig>,                    // NEW
}

pub struct NamedProviderConfig {
    pub name: String,
    pub provider: String,
    pub api_key_env: String,
    pub model: String,
    pub base_url: Option<String>,
}

pub struct RouteConfig {
    pub hint: String,
    pub use_provider: String,
}
```

### Example Configs

**Minimal (unchanged from current):**
```toml
[llm]
provider = "openai_compatible"
api_key_env = "OPENAI_API_KEY"
model = "gpt-4o-mini"
base_url = "https://api.openai.com"
```

**Simple with alias:**
```toml
[llm]
provider = "groq"
api_key_env = "GROQ_API_KEY"
model = "llama-3.3-70b-versatile"
```

**With fallback + retry:**
```toml
[llm]
provider = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"
model = "claude-sonnet-4-20250514"
max_retries = 3

[llm.fallback]
provider = "groq"
api_key_env = "GROQ_API_KEY"
model = "llama-3.3-70b-versatile"
```

**With router:**
```toml
[llm]
provider = "anthropic"
api_key_env = "ANTHROPIC_API_KEY"
model = "claude-sonnet-4-20250514"

[[extra_providers]]
name = "fast"
provider = "groq"
api_key_env = "GROQ_API_KEY"
model = "llama-3.3-70b-versatile"

[[extra_providers]]
name = "local"
provider = "ollama"
model = "qwen3:0.6b"

[[routes]]
hint = "fast"
use_provider = "fast"
```

## 9. Agent Loop Changes

Remove the manual `call_llm()` failover method from `Agent`. The `ReliableProvider` handles all retry/failover logic. The agent just calls `self.llm.chat(context)` — the reliable wrapper does the rest.

Before:
```rust
pub struct Agent {
    llm: Box<dyn LlmProvider>,
    fallback_llm: Option<Box<dyn LlmProvider>>,
    ...
}
```

After:
```rust
pub struct Agent {
    llm: Box<dyn LlmProvider>,  // ReliableProvider or RouterProvider wrapping everything
    ...
}
```

The `fallback_llm` field is removed. Failover lives in `ReliableProvider`.

## 10. Testing Strategy

- Each new file gets unit tests
- Alias resolution: test all 40+ names resolve correctly
- Error classification: test all HTTP codes + body patterns
- ReliableProvider: test retry on 429/5xx, fail-fast on 401/404, backoff timing
- Gemini: test request/response serialization (no live API calls)
- Router: test hint resolution and passthrough
- Integration: existing agent tests must pass unchanged
