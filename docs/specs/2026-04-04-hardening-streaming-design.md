# Hardening + Streaming Design

**Date**: 2026-04-04
**Scope**: Part A — fix bugs, add rate limiting, logging, tests. Part B — real SSE streaming from providers through to clients.

---

## Part A: Hardening

### A1. Wire Router Provider (or remove dead code)

`router.rs` is 222 LOC with `#[allow(dead_code)]`. Wire it into `main.rs`: when `config.routes` is non-empty, wrap the ReliableProvider in a RouterProvider. When empty, skip it (current behavior).

In `create_agent()`:
```rust
let llm: Box<dyn LlmProvider> = if !config.routes.is_empty() {
    // Build named providers + router
    ...
} else {
    // Simple: reliable wrapping primary + fallback (current behavior)
    Box::new(ReliableProvider::new(primary, fallbacks, ...))
};
```

### A2. Fix Skill Frontmatter Parser

The hand-rolled YAML→TOML converter in `skills.rs:166-199` can't handle YAML block lists:
```yaml
requires:
  tools:
    - shell_exec
    - file_ops
```

Fix: Add block list parsing to `parse_yaml_frontmatter()`. When we see `  - item` lines under a key, collect them into `["item1", "item2"]` TOML array format. This is ~20 LOC.

### A3. Consolidation Attempt Limit

Add `consolidation_failures: u32` to `Session`. Increment on consolidation failure, reset on success. If failures > 3, skip consolidation and log a warning. Prevents infinite retry loops.

### A4. Move Context Budgets to Config

Add to `AgentConfig`:
```toml
[agent]
context_soul_max = 4096
context_user_max = 2048
context_memory_max = 4096
context_daily_notes_max = 3072
```

Pass through to `ContextBuilder`.

### A5. Rate Limiting on HTTP API

Add a simple per-IP rate limiter using a `HashMap<IpAddr, (Instant, u32)>` in `HttpState`. Default: 60 requests/minute. Configurable via `[server] rate_limit_per_minute = 60`.

Middleware checks before auth: if count > limit within 60s window, return 429.

### A6. Request Logging

Add structured logging in `agent_worker` when processing each request:
```rust
tracing::info!(
    session_id = %input.session_id,
    tool_count = tool_calls.len(),
    "Processing request"
);
```

Also log on completion with token usage.

### A7. Tighten Default Shell Whitelist

Remove `curl`, `ping`, `ifconfig` from default config. Keep: `ls`, `cat`, `date`, `df`, `free`, `uptime`, `wc`, `du`, `sort`, `head`, `tail`, `grep`, `whoami`, `hostname`, `uname`. Add a comment explaining why `curl` is excluded.

### A8. Integration Tests

Add `tests/integration_test.rs` with:
- HTTP chat request → agent → tool → response
- Session persistence across agent restarts
- Rate limit enforcement
- Concurrent request handling

---

## Part B: Streaming

### Architecture

Current (fake streaming):
```
Client → HTTP → Agent Worker → LLM.chat() → full response → Output
                                                                 ↓
Client ← SSE ← chunk 20 chars with 15ms delay ←←←←←←←←←←←←←←←←←
```

After (real streaming):
```
Client → HTTP → Agent Worker → LLM.chat_streaming(tx) → tokens flow → tx.send()
                                                            ↓               ↓
                                         full ChatResponse  ↓     Client ← SSE ← real tokens
                                         (for tool parsing)  ↓
                                         loop continues       ↓
```

### Key Design Decision

The agent loop still needs a complete `ChatResponse` for tool call parsing. So `chat_streaming()` returns `Result<ChatResponse>` (same as `chat()`) BUT also sends text chunks through a channel as they arrive from the provider's SSE stream.

```rust
async fn chat_streaming(
    &self,
    context: &Context,
    tx: mpsc::Sender<String>,
) -> Result<ChatResponse> {
    // Default: call chat() and send full text at once
    let response = self.chat(context).await?;
    if let Some(ref text) = response.text {
        let _ = tx.send(text.clone()).await;
    }
    Ok(response)
}
```

Providers that support streaming override this with real SSE parsing.

### SSE Parsing Per Provider

**Anthropic** (`event: content_block_delta` → `delta.text`):
- Parse `event:` line to determine event type
- On `content_block_delta`: extract `delta.text`, send through channel
- On `message_delta`: extract `usage.output_tokens`
- On `message_stop`: done
- Accumulate text + tool_calls into final `ChatResponse`

**OpenAI-compatible** (`data: {JSON}` → `choices[0].delta.content`):
- Parse `data: [DONE]` as completion
- Parse `data: {JSON}` for `choices[0].delta.content` (text) and `choices[0].delta.tool_calls` (tools)
- Send text deltas through channel
- Accumulate into final `ChatResponse`

**Gemini** (JSON chunks → `candidates[0].content.parts[0].text`):
- Gemini streaming returns partial JSON responses
- Parse each chunk for text parts, send through channel
- Accumulate into final `ChatResponse`

### Agent Loop Changes

`Input` gets an optional streaming channel:
```rust
pub struct Input {
    pub id: String,
    pub session_id: String,
    pub content: String,
    pub stream_tx: Option<mpsc::Sender<String>>,  // NEW
}
```

In `process_inner()`, when calling LLM:
```rust
let response = if let Some(ref tx) = input.stream_tx {
    self.llm.chat_streaming(&context, tx.clone()).await?
} else {
    self.llm.chat(&context).await?
};
```

### HTTP SSE Endpoint Changes

`api_stream.rs` currently fakes streaming. Change to:
1. Create `mpsc::channel(64)` for streaming
2. Include `stream_tx` in the `Input`
3. Forward chunks from the channel as SSE `text_delta` events
4. When agent completes, send `done` event

### ReliableProvider Streaming

`ReliableProvider` wraps `chat_streaming()` the same way it wraps `chat()` — retry logic applies to the streaming call. On retry, a new channel is NOT created (the same tx is reused — previous partial chunks were already sent to the client, which is fine for streaming UX).

---

## Testing Strategy

- A1-A7: Unit tests per fix
- A8: Integration tests in `tests/`
- B: Mock SSE server responses for provider streaming tests
- Streaming end-to-end: test that chunks arrive before full response completes
