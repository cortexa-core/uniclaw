# Hardening + Streaming Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 7 hardening issues, add integration tests, then implement real SSE streaming from providers through to HTTP clients.

**Architecture:** Part A hardens existing code (bugs, security, config, tests). Part B adds `chat_streaming()` to provider trait with SSE parsing for Anthropic/OpenAI/Gemini, wired through agent loop to HTTP SSE endpoint.

**Tech Stack:** Rust, Tokio, Axum, reqwest, serde_json, tokio::sync::mpsc

---

## Part A: Hardening

### Task 1: Wire router provider into agent initialization

**Files:**
- Modify: `src/main.rs:123-158`
- Modify: `src/config.rs` (add `routes`, `extra_providers` fields)
- Modify: `src/llm/router.rs` (remove `#[allow(dead_code)]`)

- [ ] **Step 1: Add config structs for extra providers and routes**

In `src/config.rs`, add after the existing structs:

```rust
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct NamedProviderConfig {
    pub name: String,
    pub provider: String,
    #[serde(default)]
    pub api_key_env: String,
    pub model: String,
    #[serde(default)]
    pub base_url: String,
    #[serde(default = "default_max_tokens")]
    pub max_tokens: u32,
    #[serde(default = "default_temperature")]
    pub temperature: f32,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct RouteConfig {
    pub hint: String,
    pub use_provider: String,
}
```

Add to `Config` struct:

```rust
    #[serde(default)]
    pub extra_providers: Vec<NamedProviderConfig>,
    #[serde(default)]
    pub routes: Vec<RouteConfig>,
```

- [ ] **Step 2: Add helper to convert NamedProviderConfig → LlmConfig**

In `src/config.rs`:

```rust
impl NamedProviderConfig {
    pub fn to_llm_config(&self) -> LlmConfig {
        LlmConfig {
            provider: self.provider.clone(),
            api_key_env: self.api_key_env.clone(),
            model: self.model.clone(),
            base_url: self.base_url.clone(),
            max_tokens: self.max_tokens,
            temperature: self.temperature,
            timeout_secs: self.timeout_secs,
            fallback: None,
            max_retries: 2,
            base_backoff_ms: 200,
        }
    }
}
```

- [ ] **Step 3: Wire router into create_agent() in main.rs**

In `src/main.rs`, replace the provider construction in `create_agent()`:

```rust
    let primary = llm::create_provider(&config.llm)?;

    let mut fallbacks: Vec<Box<dyn llm::LlmProvider>> = Vec::new();
    if let Some(ref fallback_config) = config.llm.fallback {
        fallbacks.push(llm::create_provider(fallback_config)?);
    }

    let reliable: Box<dyn llm::LlmProvider> = Box::new(llm::reliable::ReliableProvider::new(
        primary,
        fallbacks,
        config.llm.max_retries,
        config.llm.base_backoff_ms,
    ));

    // Wrap in router if routes are configured
    let llm: Box<dyn llm::LlmProvider> = if !config.routes.is_empty() {
        let mut providers: std::collections::HashMap<String, Box<dyn llm::LlmProvider>> =
            std::collections::HashMap::new();
        providers.insert("default".to_string(), reliable);

        for named in &config.extra_providers {
            let provider = llm::create_provider(&named.to_llm_config())?;
            let wrapped = Box::new(llm::reliable::ReliableProvider::new(
                provider,
                vec![],
                config.llm.max_retries,
                config.llm.base_backoff_ms,
            ));
            providers.insert(named.name.clone(), wrapped);
        }

        let mut routes = std::collections::HashMap::new();
        for route in &config.routes {
            let provider_name = &route.use_provider;
            if !providers.contains_key(provider_name) && provider_name != "default" {
                tracing::warn!("Route hint '{}' references unknown provider '{}'", route.hint, provider_name);
                continue;
            }
            let target = if provider_name == "default" { "default" } else { provider_name };
            routes.insert(
                route.hint.clone(),
                (target.to_string(), String::new()), // model from provider config
            );
        }

        Box::new(llm::router::RouterProvider::new(providers, routes, "default".to_string())?)
    } else {
        reliable
    };
```

- [ ] **Step 4: Remove #[allow(dead_code)] from router.rs**

Remove the two `#[allow(dead_code)]` annotations on `RouterProvider` struct and `impl RouterProvider`.

- [ ] **Step 5: Run tests and commit**

Run: `cargo test && cargo clippy -- -D warnings`

```
git commit -m "Wire router provider into agent initialization when routes configured"
```

---

### Task 2: Fix skill frontmatter parser for block lists

**Files:**
- Modify: `src/agent/skills.rs:166-199`

- [ ] **Step 1: Add block list parsing to parse_yaml_frontmatter()**

In `src/agent/skills.rs`, enhance the `parse_yaml_frontmatter` function. After the current loop that handles `key: value` pairs, add handling for `- item` lines under array keys:

```rust
fn parse_yaml_frontmatter(frontmatter: &str) -> anyhow::Result<SkillFrontmatter> {
    let mut toml_lines = Vec::new();
    let mut in_requires = false;
    let mut current_array_key: Option<String> = None;
    let mut array_items: Vec<String> = Vec::new();

    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if trimmed == "requires:" {
            in_requires = true;
            toml_lines.push("[requires]".to_string());
            continue;
        }

        let is_indented = line.starts_with("  ") || line.starts_with('\t');
        if !is_indented && in_requires {
            in_requires = false;
        }

        // Handle block list items: "    - item"
        if let Some(item) = trimmed.strip_prefix("- ") {
            let item = item.trim().trim_matches('"').trim_matches('\'');
            array_items.push(format!("\"{}\"", item));
            continue;
        }

        // Flush pending array items when we hit a new key
        if let Some(ref key) = current_array_key {
            if !array_items.is_empty() {
                toml_lines.push(format!("{} = [{}]", key, array_items.join(", ")));
                array_items.clear();
            }
            current_array_key = None;
        }

        if let Some((key, value)) = trimmed.split_once(':') {
            let key = key.trim();
            let value = value.trim();
            if value.is_empty() {
                // Could be start of a block list or a section
                if is_indented {
                    current_array_key = Some(key.to_string());
                }
                continue;
            }
            toml_lines.push(format!("{key} = {}", yaml_value_to_toml(value)));
        }
    }

    // Flush any remaining array items
    if let Some(ref key) = current_array_key {
        if !array_items.is_empty() {
            toml_lines.push(format!("{} = [{}]", key, array_items.join(", ")));
        }
    }

    let toml_str = toml_lines.join("\n");
    toml::from_str(&toml_str)
        .map_err(|e| anyhow::anyhow!("Failed to parse skill frontmatter: {e}"))
}
```

- [ ] **Step 2: Add test for block list format**

```rust
    #[test]
    fn test_parse_yaml_with_block_list_requires() {
        let fm = "name: multi-tool\ndescription: Needs multiple tools\nrequires:\n  tools:\n    - shell_exec\n    - file_ops";
        let parsed = parse_yaml_frontmatter(fm).unwrap();
        assert_eq!(parsed.requires.tools, vec!["shell_exec", "file_ops"]);
    }
```

- [ ] **Step 3: Run tests and commit**

Run: `cargo test -- skills`

```
git commit -m "Fix skill frontmatter parser to handle YAML block list syntax"
```

---

### Task 3: Add consolidation attempt limit

**Files:**
- Modify: `src/agent/memory.rs` (Session struct + consolidate method)

- [ ] **Step 1: Add consolidation_failures field to Session**

In `src/agent/memory.rs`, add to `Session`:

```rust
pub struct Session {
    pub id: String,
    pub messages: Vec<Message>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub needs_consolidation: bool,
    #[serde(default)]
    pub consolidation_failures: u32,
}
```

Initialize to 0 in `Session::new()` and `load_from_disk()`.

- [ ] **Step 2: Enforce limit in consolidate()**

At the start of `consolidate()`, add:

```rust
    if session.consolidation_failures >= 3 {
        tracing::warn!(
            "Session {} has failed consolidation {} times, skipping",
            session.id, session.consolidation_failures
        );
        session.needs_consolidation = false;
        return Ok(());
    }
```

In the error branch (consolidation LLM call failed), increment:

```rust
    Err(e) => {
        tracing::warn!("Consolidation LLM call failed: {e}. Keeping messages.");
        session.consolidation_failures += 1;
    }
```

On success, reset: `session.consolidation_failures = 0;`

- [ ] **Step 3: Add test**

```rust
    #[tokio::test]
    async fn test_consolidation_gives_up_after_failures() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("memory")).unwrap();
        let mgr = MemoryManager::new(dir.path().to_path_buf());

        struct FailingLlm;
        #[async_trait::async_trait]
        impl LlmProvider for FailingLlm {
            async fn chat(&self, _: &Context) -> anyhow::Result<ChatResponse> {
                Err(anyhow::anyhow!("LLM unavailable"))
            }
            fn name(&self) -> &str { "failing" }
        }

        let mut session = Session::new("test");
        for i in 0..10 {
            session.add_message(Role::User, &format!("Msg {i}"));
            session.add_message(Role::Assistant, &format!("Reply {i}"));
        }
        session.needs_consolidation = true;

        // Fail 3 times
        for _ in 0..3 {
            session.needs_consolidation = true;
            mgr.consolidate(&mut session, &FailingLlm, 8192).await.unwrap();
        }

        // 4th attempt should be skipped (failures >= 3)
        session.needs_consolidation = true;
        mgr.consolidate(&mut session, &FailingLlm, 8192).await.unwrap();
        assert!(!session.needs_consolidation); // Gave up
        assert_eq!(session.message_count(), 20); // Messages untouched
    }
```

- [ ] **Step 4: Run tests and commit**

```
git commit -m "Add consolidation attempt limit to prevent infinite retry loops"
```

---

### Task 4: Move context budgets to config

**Files:**
- Modify: `src/config.rs` (add budget fields to AgentConfig)
- Modify: `src/agent/context.rs` (read budgets from config)
- Modify: `src/agent/loop.rs` (pass budgets through)

- [ ] **Step 1: Add budget fields to config AgentConfig**

In `src/config.rs`, add to `AgentConfig`:

```rust
    #[serde(default = "default_soul_max")]
    pub context_soul_max: usize,
    #[serde(default = "default_user_max")]
    pub context_user_max: usize,
    #[serde(default = "default_memory_max_context")]
    pub context_memory_max: usize,
    #[serde(default = "default_daily_notes_max")]
    pub context_daily_notes_max: usize,
```

Add defaults:
```rust
fn default_soul_max() -> usize { 4096 }
fn default_user_max() -> usize { 2048 }
fn default_memory_max_context() -> usize { 4096 }
fn default_daily_notes_max() -> usize { 3072 }
```

- [ ] **Step 2: Pass budgets to ContextBuilder**

Update `ContextBuilder::new()` to accept budgets. In `loop.rs`, pass them from config.

- [ ] **Step 3: Run tests and commit**

```
git commit -m "Make context budgets configurable via agent config"
```

---

### Task 5: Add rate limiting to HTTP API

**Files:**
- Modify: `src/server/http.rs`
- Modify: `src/config.rs` (add rate_limit field)

- [ ] **Step 1: Add rate_limit_per_minute to ServerConfig**

In `src/config.rs`, add to `ServerConfig`:
```rust
    #[serde(default = "default_rate_limit")]
    pub rate_limit_per_minute: u32,
```
```rust
fn default_rate_limit() -> u32 { 60 }
```

- [ ] **Step 2: Add rate limiter state to HttpState**

In `src/server/http.rs`, add to `HttpState`:
```rust
    pub rate_limiter: std::sync::Mutex<std::collections::HashMap<std::net::IpAddr, (std::time::Instant, u32)>>,
    pub rate_limit_per_minute: u32,
```

- [ ] **Step 3: Add rate limit check in auth middleware**

In the middleware function, before the auth check, add IP-based rate limiting:

```rust
    // Rate limiting (before auth)
    if state.rate_limit_per_minute > 0 {
        let ip = req.extensions().get::<axum::extract::ConnectInfo<std::net::SocketAddr>>()
            .map(|ci| ci.0.ip())
            .unwrap_or(std::net::IpAddr::V4(std::net::Ipv4Addr::LOCALHOST));

        let mut limiter = state.rate_limiter.lock().unwrap();
        let entry = limiter.entry(ip).or_insert((std::time::Instant::now(), 0));

        if entry.0.elapsed() > std::time::Duration::from_secs(60) {
            *entry = (std::time::Instant::now(), 1);
        } else {
            entry.1 += 1;
            if entry.1 > state.rate_limit_per_minute {
                return Err(StatusCode::TOO_MANY_REQUESTS);
            }
        }
    }
```

- [ ] **Step 4: Initialize in main.rs**

Update `HttpState` construction in `run_serve()` to include rate limiter fields.

- [ ] **Step 5: Run tests and commit**

```
git commit -m "Add per-IP rate limiting to HTTP API"
```

---

### Task 6: Add request logging

**Files:**
- Modify: `src/main.rs` (agent_worker logging)
- Modify: `src/agent/loop.rs` (process_inner logging)

- [ ] **Step 1: Add structured logging to agent worker**

In `src/main.rs`, in `spawn_agent_worker`, enhance the processing:

```rust
while let Some((input, reply_tx)) = inbound_rx.recv().await {
    tracing::info!(
        session = %input.session_id,
        "Processing request"
    );
    let start = std::time::Instant::now();
    let result = agent.process(&input).await;
    let elapsed = start.elapsed();
    match result {
        Ok(ref output) => {
            tracing::info!(
                session = %input.session_id,
                elapsed_ms = elapsed.as_millis() as u64,
                tokens_in = output.usage.as_ref().map(|u| u.input_tokens).unwrap_or(0),
                tokens_out = output.usage.as_ref().map(|u| u.output_tokens).unwrap_or(0),
                "Request completed"
            );
            reply_tx.send(output).ok(); // need to handle the move
        }
        // ...
    }
}
```

Note: Since `result` is moved into `reply_tx.send()`, extract the logging info before sending. Adjust the code to log before sending.

- [ ] **Step 2: Run tests and commit**

```
git commit -m "Add structured request logging with session, timing, and token usage"
```

---

### Task 7: Tighten default shell whitelist

**Files:**
- Modify: `config/config.toml`
- Modify: `config/default_config.toml`

- [ ] **Step 1: Update both config files**

Change `shell_allowed_commands` in both config files:

```toml
# Security: only safe read-only commands by default.
# Add "curl", "ping", "ifconfig" only if your use case requires network access.
shell_allowed_commands = ["ls", "cat", "date", "df", "free", "uptime", "wc", "du", "sort", "head", "tail", "grep", "whoami", "hostname", "uname"]
```

- [ ] **Step 2: Commit**

```
git commit -m "Remove curl/ping/ifconfig from default shell whitelist for security"
```

---

### Task 8: Integration tests

**Files:**
- Create: `tests/integration_test.rs`

- [ ] **Step 1: Create integration test file**

Create `tests/integration_test.rs` with tests that exercise the full stack using mock LLM providers:

```rust
//! Integration tests for the full agent pipeline.

use anyhow::Result;
use uniclaw::agent::{Agent, Input, Output};
use uniclaw::config::Config;
use uniclaw::llm::types::*;
use uniclaw::llm::LlmProvider;
use uniclaw::tools;

// ... mock provider, helper functions ...

#[tokio::test]
async fn test_agent_processes_text_response() { ... }

#[tokio::test]
async fn test_agent_executes_tool_and_returns_result() { ... }

#[tokio::test]
async fn test_session_persists_across_calls() { ... }

#[tokio::test]
async fn test_consolidation_triggers_at_threshold() { ... }
```

These tests should verify:
1. A simple text response flows through correctly
2. Tool calls are executed and results fed back to LLM
3. Session messages persist between calls to same session_id
4. Consolidation triggers when message count exceeds threshold

Note: These mostly exist in `tests/agent_test.rs` already. Read that file first — only add tests for gaps (consolidation trigger, session persistence across restarts).

- [ ] **Step 2: Run tests and commit**

```
git commit -m "Add integration tests for session persistence and consolidation"
```

---

## Part B: Streaming

### Task 9: Add streaming types and trait method

**Files:**
- Modify: `src/llm/mod.rs`
- Modify: `src/llm/types.rs`
- Modify: `src/agent/loop.rs` (add stream_tx to Input)

- [ ] **Step 1: Add stream_tx to Input**

In `src/agent/loop.rs`, add to `Input`:

```rust
pub struct Input {
    #[allow(dead_code)]
    pub id: String,
    pub session_id: String,
    pub content: String,
    /// Optional channel for streaming text chunks to the client.
    /// When Some, the agent uses streaming LLM calls.
    pub stream_tx: Option<tokio::sync::mpsc::Sender<String>>,
}
```

- [ ] **Step 2: Add chat_streaming() default to trait**

In `src/llm/mod.rs`:

```rust
#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat(&self, context: &Context) -> Result<ChatResponse>;

    /// Streaming variant: sends text chunks through tx as they arrive,
    /// then returns the complete ChatResponse for tool parsing.
    /// Default: falls back to chat() and sends the full text at once.
    async fn chat_streaming(
        &self,
        context: &Context,
        tx: tokio::sync::mpsc::Sender<String>,
    ) -> Result<ChatResponse> {
        let response = self.chat(context).await?;
        if let Some(ref text) = response.text {
            let _ = tx.send(text.clone()).await;
        }
        Ok(response)
    }

    fn name(&self) -> &str;

    fn supports_streaming(&self) -> bool {
        false
    }

    #[allow(dead_code)]
    fn supports_native_tools(&self) -> bool {
        true
    }
    #[allow(dead_code)]
    fn supports_vision(&self) -> bool {
        false
    }
}
```

- [ ] **Step 3: Use streaming in agent loop**

In `src/agent/loop.rs`, in `process_inner()`, replace the LLM call:

```rust
            let response = if let Some(ref tx) = input.stream_tx {
                self.llm.chat_streaming(&context, tx.clone()).await?
            } else {
                self.llm.chat(&context).await?
            };
```

- [ ] **Step 4: Update all Input construction sites**

Every place that creates an `Input` needs to add `stream_tx: None`:
- `src/main.rs` (send_and_wait)
- `src/server/http.rs` (chat_handler)
- `src/server/api_stream.rs` (stream_chat)
- `src/server/mqtt.rs`
- `src/server/cron.rs`
- `src/server/heartbeat.rs`
- `src/channels/telegram.rs`
- `tests/agent_test.rs`

- [ ] **Step 5: Run tests and commit**

```
git commit -m "Add chat_streaming() to LlmProvider trait with default fallback"
```

---

### Task 10: Implement SSE streaming for OpenAI-compatible provider

**Files:**
- Modify: `src/llm/openai.rs`

- [ ] **Step 1: Implement chat_streaming() with reqwest streaming**

Override `chat_streaming()` in the OpenAI provider:

```rust
async fn chat_streaming(
    &self,
    context: &Context,
    tx: tokio::sync::mpsc::Sender<String>,
) -> Result<ChatResponse> {
    let mut body = self.serialize_request(context);
    body["stream"] = json!(true);

    let url = format!("{}/v1/chat/completions", self.base_url);
    let mut request = self.client.post(&url).header("content-type", "application/json");

    // Auth (same as chat())
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
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        return Err(anyhow!("OpenAI API error ({}): {}", status.as_u16(), error_body));
    }

    // Parse SSE stream
    let mut text_parts = Vec::new();
    let mut tool_calls: Vec<ToolCall> = Vec::new();
    let mut usage = Usage::default();

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    use futures::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        // Process complete lines
        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if line.is_empty() || line.starts_with(':') {
                continue;
            }

            if let Some(data) = line.strip_prefix("data: ") {
                if data.trim() == "[DONE]" {
                    continue;
                }

                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                    // Extract text delta
                    if let Some(delta) = json["choices"][0]["delta"]["content"].as_str() {
                        if !delta.is_empty() {
                            text_parts.push(delta.to_string());
                            let _ = tx.send(delta.to_string()).await;
                        }
                    }

                    // Extract tool call deltas (accumulated)
                    if let Some(tc_deltas) = json["choices"][0]["delta"]["tool_calls"].as_array() {
                        for tc in tc_deltas {
                            let idx = tc["index"].as_u64().unwrap_or(0) as usize;
                            while tool_calls.len() <= idx {
                                tool_calls.push(ToolCall {
                                    id: String::new(),
                                    name: String::new(),
                                    arguments: serde_json::Value::Null,
                                });
                            }
                            if let Some(id) = tc["id"].as_str() {
                                tool_calls[idx].id = id.to_string();
                            }
                            if let Some(name) = tc["function"]["name"].as_str() {
                                tool_calls[idx].name = name.to_string();
                            }
                            if let Some(args) = tc["function"]["arguments"].as_str() {
                                match &mut tool_calls[idx].arguments {
                                    serde_json::Value::Null => {
                                        tool_calls[idx].arguments = serde_json::Value::String(args.to_string());
                                    }
                                    serde_json::Value::String(ref mut s) => {
                                        s.push_str(args);
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }

                    // Extract usage from final chunk
                    if let Some(u) = json.get("usage") {
                        usage.input_tokens = u["prompt_tokens"].as_u64().unwrap_or(0) as u32;
                        usage.output_tokens = u["completion_tokens"].as_u64().unwrap_or(0) as u32;
                    }
                }
            }
        }
    }

    // Finalize tool call arguments (parse accumulated JSON strings)
    for tc in &mut tool_calls {
        if let serde_json::Value::String(ref s) = tc.arguments {
            tc.arguments = serde_json::from_str(s).unwrap_or(json!({}));
        }
    }

    let stop_reason = if !tool_calls.is_empty() {
        StopReason::ToolUse
    } else {
        StopReason::EndTurn
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

fn supports_streaming(&self) -> bool {
    true
}
```

- [ ] **Step 2: Add `futures` to dependencies if not present**

Check `Cargo.toml` — `futures` should already be there. If not, add `futures = "0.3"`.

- [ ] **Step 3: Run tests and commit**

```
git commit -m "Implement real SSE streaming for OpenAI-compatible provider"
```

---

### Task 11: Implement SSE streaming for Anthropic provider

**Files:**
- Modify: `src/llm/anthropic.rs`

- [ ] **Step 1: Implement chat_streaming()**

Override `chat_streaming()` in AnthropicProvider. Anthropic's streaming format uses `event:` + `data:` pairs:

```rust
async fn chat_streaming(
    &self,
    context: &Context,
    tx: tokio::sync::mpsc::Sender<String>,
) -> Result<ChatResponse> {
    let mut body = self.serialize_request(context);
    body["stream"] = json!(true);

    let url = format!("{}/v1/messages", self.base_url);
    let response = self.client
        .post(&url)
        .header("x-api-key", &self.api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        return Err(anyhow!("Anthropic API error ({}): {}", status.as_u16(), error_body));
    }

    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();
    let mut usage = Usage::default();
    let mut current_event = String::new();

    // Track tool call building state
    let mut current_tool_id = String::new();
    let mut current_tool_name = String::new();
    let mut current_tool_args = String::new();

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    use futures::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if line.is_empty() {
                continue;
            }

            if let Some(event) = line.strip_prefix("event: ") {
                current_event = event.to_string();
                continue;
            }

            if let Some(data) = line.strip_prefix("data: ") {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                    match current_event.as_str() {
                        "content_block_start" => {
                            if json["content_block"]["type"].as_str() == Some("tool_use") {
                                current_tool_id = json["content_block"]["id"]
                                    .as_str().unwrap_or("").to_string();
                                current_tool_name = json["content_block"]["name"]
                                    .as_str().unwrap_or("").to_string();
                                current_tool_args.clear();
                            }
                        }
                        "content_block_delta" => {
                            if let Some(text) = json["delta"]["text"].as_str() {
                                text_parts.push(text.to_string());
                                let _ = tx.send(text.to_string()).await;
                            }
                            if let Some(args) = json["delta"]["partial_json"].as_str() {
                                current_tool_args.push_str(args);
                            }
                        }
                        "content_block_stop" => {
                            if !current_tool_name.is_empty() {
                                tool_calls.push(ToolCall {
                                    id: current_tool_id.clone(),
                                    name: current_tool_name.clone(),
                                    arguments: serde_json::from_str(&current_tool_args)
                                        .unwrap_or(json!({})),
                                });
                                current_tool_name.clear();
                            }
                        }
                        "message_delta" => {
                            if let Some(out) = json["usage"]["output_tokens"].as_u64() {
                                usage.output_tokens = out as u32;
                            }
                        }
                        "message_start" => {
                            if let Some(inp) = json["message"]["usage"]["input_tokens"].as_u64() {
                                usage.input_tokens = inp as u32;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    let stop_reason = if !tool_calls.is_empty() {
        StopReason::ToolUse
    } else {
        StopReason::EndTurn
    };

    let text = if text_parts.is_empty() { None } else { Some(text_parts.join("")) };

    Ok(ChatResponse { text, tool_calls, stop_reason, usage })
}

fn supports_streaming(&self) -> bool {
    true
}
```

- [ ] **Step 2: Run tests and commit**

```
git commit -m "Implement real SSE streaming for Anthropic provider"
```

---

### Task 12: Implement streaming for Gemini provider

**Files:**
- Modify: `src/llm/gemini.rs`

- [ ] **Step 1: Implement chat_streaming()**

Gemini streaming uses `streamGenerateContent` endpoint returning JSON chunks:

```rust
async fn chat_streaming(
    &self,
    context: &Context,
    tx: tokio::sync::mpsc::Sender<String>,
) -> Result<ChatResponse> {
    let body = self.serialize_request(context);
    let url = format!(
        "{}/v1beta/models/{}:streamGenerateContent?key={}&alt=sse",
        self.base_url, self.model, self.api_key
    );

    let response = self.client
        .post(&url)
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .await?;

    let status = response.status();
    if !status.is_success() {
        let error_body = response.text().await.unwrap_or_default();
        return Err(anyhow!("Gemini API error ({}): {}", status.as_u16(), error_body));
    }

    let mut text_parts = Vec::new();
    let mut tool_calls = Vec::new();
    let mut usage = Usage::default();

    let mut stream = response.bytes_stream();
    let mut buffer = String::new();

    use futures::StreamExt;
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        buffer.push_str(&String::from_utf8_lossy(&chunk));

        while let Some(line_end) = buffer.find('\n') {
            let line = buffer[..line_end].trim().to_string();
            buffer = buffer[line_end + 1..].to_string();

            if let Some(data) = line.strip_prefix("data: ") {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(data) {
                    if let Some(parts) = json["candidates"][0]["content"]["parts"].as_array() {
                        for part in parts {
                            if let Some(text) = part["text"].as_str() {
                                text_parts.push(text.to_string());
                                let _ = tx.send(text.to_string()).await;
                            }
                            if let Some(fc) = part.get("functionCall") {
                                tool_calls.push(ToolCall {
                                    id: format!("gemini_{}", tool_calls.len()),
                                    name: fc["name"].as_str().unwrap_or("").to_string(),
                                    arguments: fc["args"].clone(),
                                });
                            }
                        }
                    }
                    if let Some(u) = json.get("usageMetadata") {
                        usage.input_tokens = u["promptTokenCount"].as_u64().unwrap_or(0) as u32;
                        usage.output_tokens = u["candidatesTokenCount"].as_u64().unwrap_or(0) as u32;
                    }
                }
            }
        }
    }

    let stop_reason = if !tool_calls.is_empty() {
        StopReason::ToolUse
    } else {
        StopReason::EndTurn
    };

    let text = if text_parts.is_empty() { None } else { Some(text_parts.join("")) };

    Ok(ChatResponse { text, tool_calls, stop_reason, usage })
}

fn supports_streaming(&self) -> bool {
    true
}
```

- [ ] **Step 2: Run tests and commit**

```
git commit -m "Implement real SSE streaming for Gemini provider"
```

---

### Task 13: Wire streaming through ReliableProvider

**Files:**
- Modify: `src/llm/reliable.rs`

- [ ] **Step 1: Add chat_streaming() to ReliableProvider**

The reliable provider wraps `chat_streaming()` with the same retry logic:

```rust
async fn chat_streaming(
    &self,
    context: &Context,
    tx: tokio::sync::mpsc::Sender<String>,
) -> Result<ChatResponse> {
    // Try primary with retry
    match self.try_provider_streaming(&*self.primary, context, tx.clone()).await {
        Ok(response) => return Ok(response),
        Err(e) => {
            if self.fallbacks.is_empty() {
                return Err(e);
            }
            tracing::warn!(
                "Primary provider '{}' streaming failed: {e}, trying fallbacks",
                self.primary.name()
            );
        }
    }

    // Try fallbacks
    let mut errors = vec![format!("Primary ({}): exhausted", self.primary.name())];
    for fallback in &self.fallbacks {
        match self.try_provider_streaming(&**fallback, context, tx.clone()).await {
            Ok(response) => return Ok(response),
            Err(e) => errors.push(format!("{}: {e}", fallback.name())),
        }
    }

    Err(anyhow!("All providers failed:\n  {}", errors.join("\n  ")))
}
```

Add `try_provider_streaming()` — same retry logic as `try_provider()` but calls `chat_streaming()`.

- [ ] **Step 2: Run tests and commit**

```
git commit -m "Wire streaming through ReliableProvider with retry/failover"
```

---

### Task 14: Update HTTP SSE endpoint for real streaming

**Files:**
- Modify: `src/server/api_stream.rs`

- [ ] **Step 1: Replace fake streaming with real streaming**

Rewrite `stream_chat()` to create a streaming channel and include it in the Input:

```rust
pub async fn stream_chat(
    State(state): State<Arc<HttpState>>,
    Json(req): Json<StreamChatRequest>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let inbound_tx = state.inbound_tx.clone();
    let session_id = req.session_id.clone();

    let stream = async_stream::stream! {
        // Create streaming channel
        let (stream_tx, mut stream_rx) = tokio::sync::mpsc::channel::<String>(64);

        let input = Input {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.clone(),
            content: req.message.clone(),
            stream_tx: Some(stream_tx),
        };

        let (reply_tx, reply_rx) = oneshot::channel::<Output>();

        yield Ok::<_, Infallible>(Event::default()
            .event("status")
            .data(r#"{"type":"thinking"}"#));

        if inbound_tx.send((input, reply_tx)).await.is_err() {
            yield Ok(Event::default()
                .event("error")
                .data(r#"{"error":"Agent worker unavailable"}"#));
            return;
        }

        // Stream text chunks as they arrive from the provider
        let reply_handle = tokio::spawn(async move { reply_rx.await });

        loop {
            tokio::select! {
                chunk = stream_rx.recv() => {
                    match chunk {
                        Some(text) => {
                            yield Ok(Event::default()
                                .event("text_delta")
                                .data(serde_json::json!({"text": text}).to_string()));
                        }
                        None => break, // Channel closed
                    }
                }
            }
        }

        // Get final result for usage info
        match reply_handle.await {
            Ok(Ok(output)) => {
                if let Some(usage) = &output.usage {
                    yield Ok(Event::default()
                        .event("usage")
                        .data(serde_json::json!({
                            "input_tokens": usage.input_tokens,
                            "output_tokens": usage.output_tokens,
                        }).to_string()));
                }
                yield Ok(Event::default()
                    .event("done")
                    .data(serde_json::json!({"session_id": session_id}).to_string()));
            }
            _ => {
                yield Ok(Event::default()
                    .event("error")
                    .data(r#"{"error":"Request failed"}"#));
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}
```

- [ ] **Step 2: Run tests and commit**

```
git commit -m "Replace fake SSE streaming with real provider streaming"
```

---

### Task 15: Final verification

- [ ] **Step 1: Run full test suite**

```bash
cargo test
cargo test --features telegram
```

- [ ] **Step 2: Clippy + fmt**

```bash
cargo clippy -- -D warnings
cargo fmt
```

- [ ] **Step 3: Manual smoke test**

```bash
cargo run -- chat -m "What is 2+2?"
```

Verify response streams character by character (if using streaming-capable provider).

- [ ] **Step 4: Commit any fixes**

```
git commit -m "Final verification: clippy, fmt, smoke test"
```

---

## Summary

| Task | Files | What |
|------|-------|------|
| **Part A** |
| 1 | config.rs, main.rs, router.rs | Wire router provider |
| 2 | skills.rs | Fix YAML block list parsing |
| 3 | memory.rs | Consolidation attempt limit |
| 4 | config.rs, context.rs, loop.rs | Configurable context budgets |
| 5 | http.rs, config.rs, main.rs | Rate limiting |
| 6 | main.rs, loop.rs | Request logging |
| 7 | config files | Tighten shell whitelist |
| 8 | tests/ | Integration tests |
| **Part B** |
| 9 | mod.rs, loop.rs | Streaming trait + Input.stream_tx |
| 10 | openai.rs | OpenAI SSE streaming |
| 11 | anthropic.rs | Anthropic SSE streaming |
| 12 | gemini.rs | Gemini SSE streaming |
| 13 | reliable.rs | Streaming through reliable wrapper |
| 14 | api_stream.rs | Real SSE endpoint |
| 15 | — | Final verification |

**Total: 15 tasks, ~1,500 LOC new code**
