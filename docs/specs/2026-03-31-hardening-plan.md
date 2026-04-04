# UniClaw Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Fix 18 verified bugs, security gaps, reliability and performance issues across the UniClaw codebase.

**Architecture:** Each task is a self-contained commit touching 1-3 files. Tasks are ordered to group changes by file, minimizing merge conflicts. Every fix includes a test proving the issue is resolved.

**Tech Stack:** Rust, Tokio, Axum, teloxide, serde, TOML config

---

### Task 1: Fix Telegram reply-to-bot matching any bot

**Files:**
- Modify: `src/channels/telegram.rs:88-98`

- [ ] **Step 1: Fix the `is_reply_to_bot` check**

In `src/channels/telegram.rs`, replace lines 92-96:

```rust
                            let is_reply_to_bot = msg
                                .reply_to_message()
                                .and_then(|r| r.from.as_ref())
                                .map(|u| u.is_bot)
                                .unwrap_or(false);
```

With:

```rust
                            let is_reply_to_bot = msg
                                .reply_to_message()
                                .and_then(|r| r.from.as_ref())
                                .map(|u| {
                                    u.is_bot
                                        && u.username.as_deref()
                                            == Some(bot_username.as_str())
                                })
                                .unwrap_or(false);
```

- [ ] **Step 2: Remove MarkdownV2 and use plain text**

Replace lines 150-160:

```rust
                // Chunk and send (Telegram max 4096 chars)
                for chunk in chunk_message(&response, 4096) {
                    // Try Markdown first, fall back to plain text
                    let result = bot
                        .send_message(msg.chat.id, &chunk)
                        .parse_mode(ParseMode::MarkdownV2)
                        .await;

                    if result.is_err() {
                        bot.send_message(msg.chat.id, &chunk).await.ok();
                    }
                }
```

With:

```rust
                // Chunk and send (Telegram max 4096 chars)
                let chunks = chunk_message(&response, 4096);
                for (i, chunk) in chunks.iter().enumerate() {
                    if i > 0 {
                        // Rate-limit: 100ms between chunks to avoid Telegram throttling
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                    bot.send_message(msg.chat.id, chunk).await.ok();
                }
```

Also remove the unused import `ParseMode` from line 4:

```rust
use teloxide::types::ChatAction;
```

(Remove `ParseMode` from the `use teloxide::types::{ChatAction, ParseMode};` import.)

- [ ] **Step 3: Wrap `allowed_users`, `respond_in_groups`, `bot_username` in Arc**

Replace lines 58-66:

```rust
        let allowed_users = self.allowed_users.clone();
        let respond_in_groups = self.respond_in_groups.clone();
        let bot_username = me.username.clone().unwrap_or_default();

        teloxide::repl(bot, move |bot: Bot, msg: Message| {
            let agent_tx = agent_tx.clone();
            let allowed_users = allowed_users.clone();
            let respond_in_groups = respond_in_groups.clone();
            let bot_username = bot_username.clone();
```

With:

```rust
        let allowed_users = std::sync::Arc::new(self.allowed_users.clone());
        let respond_in_groups = std::sync::Arc::new(self.respond_in_groups.clone());
        let bot_username = std::sync::Arc::new(me.username.clone().unwrap_or_default());

        teloxide::repl(bot, move |bot: Bot, msg: Message| {
            let agent_tx = agent_tx.clone();
            let allowed_users = std::sync::Arc::clone(&allowed_users);
            let respond_in_groups = std::sync::Arc::clone(&respond_in_groups);
            let bot_username = std::sync::Arc::clone(&bot_username);
```

- [ ] **Step 4: Add reconnection loop around `repl`**

Replace the `teloxide::repl(...)` call and surrounding code. The `run` method body after the `let bot_username = ...` line should become:

```rust
        loop {
            let agent_tx = agent_tx.clone();
            let allowed_users = std::sync::Arc::clone(&allowed_users);
            let respond_in_groups = std::sync::Arc::clone(&respond_in_groups);
            let bot_username = std::sync::Arc::clone(&bot_username);
            let bot = bot.clone();

            teloxide::repl(bot, move |bot: Bot, msg: Message| {
                let agent_tx = agent_tx.clone();
                let allowed_users = std::sync::Arc::clone(&allowed_users);
                let respond_in_groups = std::sync::Arc::clone(&respond_in_groups);
                let bot_username = std::sync::Arc::clone(&bot_username);

                async move {
                    // ... (all existing handler code unchanged except the fixes above)
                }
            })
            .await;

            tracing::warn!("Telegram repl exited unexpectedly, restarting in 5s");
            tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        }
```

Note: The `Ok(())` after the loop is now unreachable. Change the return type handling — the loop runs forever, but keep `Ok(())` after it for type compatibility (the compiler will warn it's unreachable; that's fine, or add `#[allow(unreachable_code)]`).

- [ ] **Step 5: Build and verify**

Run: `cargo build --features telegram`
Expected: Compiles with no errors.

- [ ] **Step 6: Commit**

```bash
git add src/channels/telegram.rs
git commit -m "Fix Telegram: reply-to-bot check, remove MarkdownV2, add reconnection and rate limiting"
```

---

### Task 2: Add `GroupResponseMode` enum to config

**Files:**
- Modify: `src/config.rs:127-137, 237-239`
- Modify: `src/channels/telegram.rs:14, 85-98`

- [ ] **Step 1: Define the enum in config.rs**

Replace the `respond_in_groups` field in `TelegramConfig` (lines 127-137):

```rust
#[derive(Debug, Clone, Deserialize, serde::Serialize)]
pub struct TelegramConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub bot_token_env: String,
    #[serde(default)]
    pub allowed_users: Vec<i64>,
    #[serde(default)]
    pub respond_in_groups: GroupResponseMode,
}

#[derive(Debug, Clone, Deserialize, serde::Serialize, Default, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum GroupResponseMode {
    Always,
    Never,
    #[default]
    Mention,
}
```

Remove the `default_respond_in_groups` function (lines 237-239).

- [ ] **Step 2: Update telegram.rs to use the enum**

In `src/channels/telegram.rs`, change the struct field type (line 14):

```rust
pub struct TelegramChannel {
    bot_token: String,
    allowed_users: Vec<i64>,
    respond_in_groups: crate::config::GroupResponseMode,
}
```

Update the group policy check (around lines 85-98) — replace the string match with:

```rust
                if is_group {
                    let should_respond = match respond_in_groups.as_ref() {
                        crate::config::GroupResponseMode::Always => true,
                        crate::config::GroupResponseMode::Never => return Ok(()),
                        crate::config::GroupResponseMode::Mention => {
                            let mentioned = !bot_username.is_empty()
                                && text.contains(&format!("@{bot_username}"));
                            let is_reply_to_bot = msg
                                .reply_to_message()
                                .and_then(|r| r.from.as_ref())
                                .map(|u| {
                                    u.is_bot
                                        && u.username.as_deref()
                                            == Some(bot_username.as_str())
                                })
                                .unwrap_or(false);
                            mentioned || is_reply_to_bot
                        }
                    };
                    if !should_respond {
                        return Ok(());
                    }
                }
```

Since `respond_in_groups` is now `Arc<GroupResponseMode>`, the `.as_ref()` call returns `&GroupResponseMode`.

- [ ] **Step 3: Add a config parsing test**

Add to `src/config.rs` tests:

```rust
    #[test]
    fn test_group_response_mode_parsing() {
        let toml = r#"
[agent]
[llm]
provider = "test"
model = "test"
[channels.telegram]
bot_token_env = "TEST"
respond_in_groups = "always"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(
            config.channels.telegram.unwrap().respond_in_groups,
            GroupResponseMode::Always
        );
    }

    #[test]
    fn test_group_response_mode_default() {
        let toml = r#"
[agent]
[llm]
provider = "test"
model = "test"
[channels.telegram]
bot_token_env = "TEST"
"#;
        let config: Config = toml::from_str(toml).unwrap();
        assert_eq!(
            config.channels.telegram.unwrap().respond_in_groups,
            GroupResponseMode::Mention
        );
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test --features telegram`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/config.rs src/channels/telegram.rs
git commit -m "Replace respond_in_groups String with GroupResponseMode enum"
```

---

### Task 3: Fix shell empty whitelist and add argument path restriction

**Files:**
- Modify: `src/tools/shell.rs:34-80`

- [ ] **Step 1: Add empty-whitelist denial and absolute path check**

In `src/tools/shell.rs`, replace the whitelist checking block (lines 58-80) with:

```rust
        // Check each pipeline segment's program against whitelist
        let allowed: HashSet<&str> = ctx
            .config
            .tools
            .shell_allowed_commands
            .iter()
            .map(|s| s.as_str())
            .collect();

        // Empty whitelist = deny all (not allow all)
        if allowed.is_empty() {
            return ToolResult::Error(
                "No commands are allowed: shell_allowed_commands is empty in config.".into(),
            );
        }

        let data_dir_str = ctx.data_dir.to_string_lossy();

        for segment in &segments {
            let parts: Vec<&str> = segment.split_whitespace().collect();
            let program = match parts.first() {
                Some(p) => *p,
                None => return ToolResult::Error("Empty command segment in pipeline".into()),
            };

            if !allowed.contains(program) {
                return ToolResult::Error(format!(
                    "Command '{program}' is not in the allowed list. Allowed: {}",
                    ctx.config.tools.shell_allowed_commands.join(", ")
                ));
            }

            // Reject absolute path arguments that escape the data directory
            for arg in &parts[1..] {
                if arg.starts_with('/') && !arg.starts_with(data_dir_str.as_ref()) {
                    return ToolResult::Error(format!(
                        "Argument '{arg}' references an absolute path outside the data directory. \
                         Use relative paths or paths within the data directory."
                    ));
                }
            }
        }
```

- [ ] **Step 2: Add tests for the new restrictions**

Add these tests to the `mod tests` block in `src/tools/shell.rs`:

```rust
    #[tokio::test]
    async fn test_shell_exec_empty_whitelist_denies() {
        let dir = tempfile::tempdir().unwrap();
        let config: Config = toml::from_str(
            r#"
[agent]
[llm]
provider = "test"
model = "test"
[tools]
shell_allowed_commands = []
"#,
        )
        .unwrap();
        let ctx = ToolContext {
            data_dir: dir.path().to_path_buf(),
            session_id: "test".into(),
            config: Arc::new(config),
        };
        let result = ShellExecTool
            .execute(json!({"command": "echo hello"}), &ctx)
            .await;
        assert!(result.is_error());
        assert!(result.content().contains("shell_allowed_commands is empty"));
    }

    #[tokio::test]
    async fn test_shell_exec_rejects_absolute_path_args() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());
        let result = ShellExecTool
            .execute(json!({"command": "cat /etc/passwd"}), &ctx)
            .await;
        assert!(result.is_error());
        assert!(result.content().contains("absolute path outside"));
    }

    #[tokio::test]
    async fn test_shell_exec_allows_relative_path_args() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("test.txt"), "hello").unwrap();
        let ctx = test_ctx(dir.path());
        let result = ShellExecTool
            .execute(json!({"command": "cat test.txt"}), &ctx)
            .await;
        assert!(!result.is_error());
        assert!(result.content().contains("hello"));
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -- shell`
Expected: All shell tests pass (including existing ones).

- [ ] **Step 4: Commit**

```bash
git add src/tools/shell.rs
git commit -m "Shell: deny-all on empty whitelist, reject absolute paths outside data_dir"
```

---

### Task 4: Atomic config write

**Files:**
- Modify: `src/server/api_config.rs:40-45`

- [ ] **Step 1: Replace direct write with write-tmp-then-rename**

In `src/server/api_config.rs`, replace lines 40-45:

```rust
    if let Err(e) = tokio::fs::write(&state.config_path, &toml_str).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to write config: {e}")})),
        );
    }
```

With:

```rust
    // Atomic write: write to temp file, then rename (POSIX rename is atomic)
    let tmp_path = state.config_path.with_extension("tmp");
    if let Err(e) = tokio::fs::write(&tmp_path, &toml_str).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to write config: {e}")})),
        );
    }
    if let Err(e) = tokio::fs::rename(&tmp_path, &state.config_path).await {
        // Clean up temp file on rename failure
        tokio::fs::remove_file(&tmp_path).await.ok();
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": format!("Failed to apply config: {e}")})),
        );
    }
```

- [ ] **Step 2: Build and verify**

Run: `cargo build`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add src/server/api_config.rs
git commit -m "Atomic config writes via temp file + rename"
```

---

### Task 5: Minimal `/api/status` for unauthenticated requests

**Files:**
- Modify: `src/server/http.rs:27-68, 137-144`

- [ ] **Step 1: Pass `api_token` into status handler and return minimal response when unauthed**

Replace the status handler (lines 137-144):

```rust
async fn status_handler(State(state): State<Arc<HttpState>>) -> Json<StatusResponse> {
    Json(StatusResponse {
        version: state.version.clone(),
        model: state.model.clone(),
        uptime_secs: state.start_time.elapsed().as_secs(),
        status: "running".into(),
    })
}
```

With:

```rust
async fn status_handler(
    State(state): State<Arc<HttpState>>,
    req: axum::extract::Request,
) -> Json<serde_json::Value> {
    // If auth is configured, check if this request has a valid token
    let is_authenticated = if state.api_token.is_empty() {
        true
    } else {
        req.headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .and_then(|h| h.strip_prefix("Bearer "))
            == Some(&state.api_token)
    };

    if is_authenticated {
        Json(serde_json::json!({
            "status": "running",
            "version": state.version,
            "model": state.model,
            "uptime_secs": state.start_time.elapsed().as_secs(),
        }))
    } else {
        Json(serde_json::json!({"status": "ok"}))
    }
}
```

Remove the `StatusResponse` struct (lines 81-87) since we now return dynamic JSON.

- [ ] **Step 2: Build and verify**

Run: `cargo build`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add src/server/http.rs
git commit -m "Return minimal /api/status to unauthenticated requests"
```

---

### Task 6: Dynamic credential redaction

**Files:**
- Modify: `src/tools/http_fetch.rs:107-123`

- [ ] **Step 1: Build redaction list from config**

Replace the `redact_known_secrets` function (lines 107-123):

```rust
/// Scan response body for any known secrets and redact them.
/// Builds the secret list dynamically from all *_env config fields.
fn redact_known_secrets(text: &str, ctx: &ToolContext) -> String {
    let mut result = text.to_string();

    // Collect all env var names from config that might contain secrets
    let mut env_names: Vec<&str> = vec![];

    // LLM API keys
    if !ctx.config.llm.api_key_env.is_empty() {
        env_names.push(&ctx.config.llm.api_key_env);
    }
    if let Some(ref fallback) = ctx.config.llm.fallback {
        if !fallback.api_key_env.is_empty() {
            env_names.push(&fallback.api_key_env);
        }
    }

    // Server API token
    if let Some(ref server) = ctx.config.server {
        if !server.api_token_env.is_empty() {
            env_names.push(&server.api_token_env);
        }
    }

    // Telegram bot token
    if let Some(ref tg) = ctx.config.channels.telegram {
        if !tg.bot_token_env.is_empty() {
            env_names.push(&tg.bot_token_env);
        }
    }

    // Also always check common API key env vars
    env_names.push("ANTHROPIC_API_KEY");
    env_names.push("OPENAI_API_KEY");

    // Deduplicate
    env_names.sort_unstable();
    env_names.dedup();

    for env_name in &env_names {
        if let Ok(key) = std::env::var(env_name) {
            if !key.is_empty() && result.contains(&key) {
                result = result.replace(&key, "[REDACTED]");
                tracing::warn!("Redacted leaked credential from HTTP response");
            }
        }
    }

    result
}
```

- [ ] **Step 2: Update existing test**

The existing test in `http_fetch.rs` (`test_redact_known_secrets`) should still pass since `ANTHROPIC_API_KEY` is still in the list. No change needed.

- [ ] **Step 3: Run tests**

Run: `cargo test -- redact`
Expected: `test_redact_known_secrets` passes.

- [ ] **Step 4: Commit**

```bash
git add src/tools/http_fetch.rs
git commit -m "Build credential redaction list dynamically from all config env fields"
```

---

### Task 7: Log silent persist errors in agent loop

**Files:**
- Modify: `src/agent/loop.rs:153, 175`

- [ ] **Step 1: Replace `.ok()` with logged warnings**

In `src/agent/loop.rs`, replace line 153:

```rust
                self.session_store.persist(&input.session_id).await.ok();
```

With:

```rust
                if let Err(e) = self.session_store.persist(&input.session_id).await {
                    tracing::warn!("Failed to persist session {} after timeout: {e}", input.session_id);
                }
```

Replace line 175:

```rust
                self.session_store.persist(&input.session_id).await.ok();
```

With:

```rust
                if let Err(e) = self.session_store.persist(&input.session_id).await {
                    tracing::warn!("Failed to persist session {} after consolidation: {e}", input.session_id);
                }
```

- [ ] **Step 2: Build and verify**

Run: `cargo build`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add src/agent/loop.rs
git commit -m "Log session persist errors instead of silently swallowing"
```

---

### Task 8: Return borrowed messages from session + fix session timestamps

**Files:**
- Modify: `src/agent/memory.rs:67-70, 232-238`
- Modify: `src/agent/context.rs:82-114`

- [ ] **Step 1: Change `messages_for_context` to return a reference**

In `src/agent/memory.rs`, replace lines 67-70:

```rust
    /// Return messages formatted for LLM context
    pub fn messages_for_context(&self) -> Vec<Message> {
        self.messages.clone()
    }
```

With:

```rust
    /// Return messages for LLM context (borrowed to avoid cloning)
    pub fn messages_for_context(&self) -> &[Message] {
        &self.messages
    }
```

- [ ] **Step 2: Update context builder to clone at the boundary**

In `src/agent/context.rs`, replace lines 108-114:

```rust
        let messages = session.messages_for_context();

        Ok(Context {
            system,
            messages,
            tool_schemas: tool_schemas.to_vec(),
        })
```

With:

```rust
        let messages = session.messages_for_context().to_vec();

        Ok(Context {
            system,
            messages,
            tool_schemas: tool_schemas.to_vec(),
        })
```

This still clones once, but moves the clone to the explicit boundary and allows future optimizations (e.g., only cloning the tail).

- [ ] **Step 3: Fix session timestamps on disk reload**

In `src/agent/memory.rs`, replace lines 232-238:

```rust
        Ok(Session {
            id: id.to_string(),
            messages,
            created_at: Utc::now(), // approximate — could parse from file metadata
            updated_at: Utc::now(),
            needs_consolidation: false,
        })
```

With:

```rust
        // Use file modification time for timestamps instead of current time
        let file_time = tokio::fs::metadata(&path)
            .await
            .ok()
            .and_then(|m| m.modified().ok())
            .map(|st| DateTime::<Utc>::from(st))
            .unwrap_or_else(Utc::now);

        Ok(Session {
            id: id.to_string(),
            messages,
            created_at: file_time,
            updated_at: file_time,
            needs_consolidation: false,
        })
```

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/agent/memory.rs src/agent/context.rs
git commit -m "Return borrowed messages from session, fix timestamps on disk reload"
```

---

### Task 9: Session cache eviction

**Files:**
- Modify: `src/agent/memory.rs:75-99`

- [ ] **Step 1: Add access tracking and eviction**

In `src/agent/memory.rs`, add an access counter to `SessionStore`:

```rust
pub struct SessionStore {
    sessions: HashMap<String, Session>,
    access_order: HashMap<String, u64>,
    access_counter: u64,
    max_cached: usize,
    data_dir: PathBuf,
}
```

Update `new`:

```rust
    pub fn new(data_dir: PathBuf, max_cached: usize) -> Self {
        Self {
            sessions: HashMap::new(),
            access_order: HashMap::new(),
            access_counter: 0,
            max_cached,
            data_dir,
        }
    }
```

Update `get_or_load` to track access and evict:

```rust
    pub async fn get_or_load(&mut self, id: &str) -> &mut Session {
        // Evict least-recently-used if at capacity and this is a new session
        if !self.sessions.contains_key(id) && self.sessions.len() >= self.max_cached {
            self.evict_lru().await;
        }

        if !self.sessions.contains_key(id) {
            let session = self
                .load_from_disk(id)
                .await
                .unwrap_or_else(|_| Session::new(id));
            self.sessions.insert(id.to_string(), session);
        }

        self.access_counter += 1;
        self.access_order
            .insert(id.to_string(), self.access_counter);

        self.sessions
            .get_mut(id)
            .expect("session was just inserted; this is a bug if it fails")
    }
```

Add the eviction helper:

```rust
    async fn evict_lru(&mut self) {
        if let Some((lru_id, _)) = self
            .access_order
            .iter()
            .min_by_key(|(_, &order)| order)
            .map(|(id, order)| (id.clone(), *order))
        {
            // Persist before evicting
            if let Err(e) = self.persist(&lru_id).await {
                tracing::warn!("Failed to persist evicted session {lru_id}: {e}");
            }
            self.sessions.remove(&lru_id);
            self.access_order.remove(&lru_id);
            tracing::debug!("Evicted session {lru_id} from cache");
        }
    }
```

- [ ] **Step 2: Update all `SessionStore::new` call sites**

In `src/agent/loop.rs` line 119, update:

```rust
            session_store: SessionStore::new(data_dir.clone(), config.agent.session_max_count),
```

- [ ] **Step 3: Add a test for eviction**

Add to `src/agent/memory.rs` tests:

```rust
    #[tokio::test]
    async fn test_session_cache_eviction() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sessions")).unwrap();
        let mut store = SessionStore::new(dir.path().to_path_buf(), 2); // max 2 cached

        // Load 3 sessions — should evict the first
        {
            let s = store.get_or_load("a").await;
            s.add_message(Role::User, "hello from a");
        }
        {
            let s = store.get_or_load("b").await;
            s.add_message(Role::User, "hello from b");
        }
        assert_eq!(store.sessions.len(), 2);

        // Loading "c" should evict "a" (least recently used)
        {
            let s = store.get_or_load("c").await;
            s.add_message(Role::User, "hello from c");
        }
        assert_eq!(store.sessions.len(), 2);
        assert!(!store.sessions.contains_key("a"));
        assert!(store.sessions.contains_key("b"));
        assert!(store.sessions.contains_key("c"));

        // "a" was persisted to disk before eviction
        assert!(dir.path().join("sessions/a.jsonl").exists());
    }
```

- [ ] **Step 4: Run tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/agent/memory.rs src/agent/loop.rs
git commit -m "Add LRU session cache eviction to prevent unbounded memory growth"
```

---

### Task 10: Async skill loading

**Files:**
- Modify: `src/agent/skills.rs:42-76, 115-147`
- Modify: `src/agent/context.rs:77-79`
- Modify: `src/agent/loop.rs:107-112`

- [ ] **Step 1: Convert `SkillManager::load` to async**

In `src/agent/skills.rs`, replace the `load` function (lines 44-76):

```rust
    /// Load skills from directory, filter by tool/env requirements
    pub async fn load(skills_dir: &Path, available_tools: &[String]) -> Self {
        let mut skills = Vec::new();

        let mut entries = match tokio::fs::read_dir(skills_dir).await {
            Ok(e) => e,
            Err(_) => return Self { skills },
        };

        while let Ok(Some(entry)) = entries.next_entry().await {
            let path = entry.path();
            if path.extension().is_none_or(|ext| ext != "md") {
                continue;
            }

            match Self::load_skill(&path, available_tools).await {
                Ok(Some(skill)) => {
                    tracing::info!("Loaded skill: {}", skill.name);
                    skills.push(skill);
                }
                Ok(None) => {} // gated out
                Err(e) => {
                    tracing::warn!("Failed to load skill {}: {e}", path.display());
                }
            }
        }

        tracing::info!(
            "Loaded {} skills from {}",
            skills.len(),
            skills_dir.display()
        );
        Self { skills }
    }
```

Replace `load_skill` (lines 115-147):

```rust
    async fn load_skill(path: &Path, available_tools: &[String]) -> anyhow::Result<Option<Skill>> {
        let raw = tokio::fs::read_to_string(path).await?;
        let (frontmatter, body) = Self::parse_frontmatter(&raw)?;
        let meta: SkillFrontmatter = parse_yaml_frontmatter(&frontmatter)?;

        // Gate: required tools
        for tool in &meta.requires.tools {
            if !available_tools.iter().any(|t| t == tool) {
                tracing::info!("Skill '{}' gated: requires tool '{tool}'", meta.name);
                return Ok(None);
            }
        }

        // Gate: required env vars
        for env_var in &meta.requires.env {
            if std::env::var(env_var).is_err() {
                tracing::info!("Skill '{}' gated: requires env var '{env_var}'", meta.name);
                return Ok(None);
            }
        }

        let content = body.trim().to_string();
        if content.is_empty() {
            tracing::warn!("Skill '{}' has empty content, skipping", meta.name);
            return Ok(None);
        }

        Ok(Some(Skill {
            name: meta.name,
            description: meta.description,
            content,
        }))
    }
```

- [ ] **Step 2: Update `set_available_tools` to be async**

In `src/agent/context.rs`, replace lines 77-79:

```rust
    /// Set available tool names for skill gating
    pub async fn set_available_tools(&mut self, tool_names: Vec<String>) {
        let skills_dir = self.data_dir.join("skills");
        self.skill_manager = Some(SkillManager::load(&skills_dir, &tool_names).await);
    }
```

- [ ] **Step 3: Update the call site in loop.rs**

In `src/agent/loop.rs`, replace lines 107-112:

```rust
        let tool_names: Vec<String> = tool_registry
            .tool_names()
            .iter()
            .map(|s| s.to_string())
            .collect();
        context_builder.set_available_tools(tool_names);
```

Since `Agent::new` is not async, we need to make the skill loading lazy or make `new` async. The simplest approach: make `set_available_tools` synchronous and spawn the async loading in `build_system_prompt` on first call. **Actually**, the simpler fix: change `Agent::new` to an async function.

Replace `pub fn new(` with `pub async fn new(` and add `.await`:

```rust
        context_builder.set_available_tools(tool_names).await;
```

Then in `src/main.rs`, find the `create_agent` function and add `.await` to the `Agent::new(...)` call. (The `create_agent` function is already async.)

- [ ] **Step 4: Fix tests that use blocking `SkillManager::load`**

In `src/agent/skills.rs` tests, change synchronous tests that call `SkillManager::load` to `#[tokio::test]` async tests, adding `.await` to the `load` calls.

- [ ] **Step 5: Run tests**

Run: `cargo test`
Expected: All tests pass.

- [ ] **Step 6: Commit**

```bash
git add src/agent/skills.rs src/agent/context.rs src/agent/loop.rs src/main.rs
git commit -m "Convert skill loading to async tokio I/O"
```

---

### Task 11: Cron job parameter bounds

**Files:**
- Modify: `src/tools/cron_tools.rs:43-82`

- [ ] **Step 1: Add length validation**

In `src/tools/cron_tools.rs`, after the `name` extraction (line 54), add bounds checks:

```rust
        let name = args["name"].as_str().unwrap_or("Unnamed job").to_string();

        if name.len() > 256 {
            return ToolResult::Error("Job name too long (max 256 characters)".into());
        }
        if action.len() > 4096 {
            return ToolResult::Error("Job action too long (max 4096 characters)".into());
        }
```

- [ ] **Step 2: Add a test**

Add to `src/tools/cron_tools.rs` tests:

```rust
    #[tokio::test]
    async fn test_cron_add_rejects_long_action() {
        let dir = tempfile::tempdir().unwrap();
        let ctx = test_ctx(dir.path());
        let long_action = "a".repeat(5000);
        let result = CronAddTool
            .execute(
                json!({"name": "Big", "action": long_action, "interval_seconds": 60}),
                &ctx,
            )
            .await;
        assert!(result.is_error());
        assert!(result.content().contains("too long"));
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test -- cron`
Expected: All cron tests pass.

- [ ] **Step 4: Commit**

```bash
git add src/tools/cron_tools.rs
git commit -m "Enforce max length on cron job name and action fields"
```

---

### Task 12: MQTT device_id validation

**Files:**
- Modify: `src/server/mqtt.rs:28-40`

- [ ] **Step 1: Add device_id validation**

In `src/server/mqtt.rs`, after line 32 (where `device_id` is extracted), add:

```rust
    // Validate device_id — only allow safe characters for MQTT topic segments
    if !device_id
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        return Err(anyhow::anyhow!(
            "mqtt_device_id contains invalid characters (only alphanumeric, hyphens, underscores allowed): '{device_id}'"
        ));
    }
```

- [ ] **Step 2: Build and verify**

Run: `cargo build`
Expected: Compiles without errors.

- [ ] **Step 3: Commit**

```bash
git add src/server/mqtt.rs
git commit -m "Validate MQTT device_id to prevent topic injection"
```

---

### Task 13: Final verification

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

- [ ] **Step 4: Run fmt check**

Run: `cargo fmt -- --check`
Expected: No formatting issues.

- [ ] **Step 5: Fix any issues found and commit**

```bash
git add -A
git commit -m "Fix clippy warnings and formatting"
```
