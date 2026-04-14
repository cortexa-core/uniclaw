use anyhow::Result;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use super::memory::Session;
use super::skills::SkillManager;
use crate::llm::types::{Context, ToolSchema};
use crate::utils::floor_char_boundary;

pub struct ContextBuilder {
    data_dir: PathBuf,
    cached_system: Option<CachedSystem>,
    skill_manager: Option<SkillManager>,
    ttl: Duration,
    budgets: ContextBudgets,
    robot_prompt: Option<String>,
    world_rx: Option<tokio::sync::watch::Receiver<crate::robot::world_state::WorldState>>,
}

struct CachedSystem {
    prompt: String,
    loaded_at: Instant,
}

pub struct ContextBudgets {
    pub soul_max: usize,
    pub user_max: usize,
    pub memory_max: usize,
    pub daily_notes_max: usize,
}

impl Default for ContextBudgets {
    fn default() -> Self {
        Self {
            soul_max: 4096,
            user_max: 2048,
            memory_max: 4096,
            daily_notes_max: 3072,
        }
    }
}

pub const DEFAULT_SOUL: &str = r#"# UniClaw

You are UniClaw, a helpful AI assistant running on a local device.

## Identity
- You are a local-first AI agent running on a Raspberry Pi
- You have direct access to the device's file system and network
- You value privacy — sensitive data stays on this device

## Behavior
- Be concise and direct
- When asked to do something, use your tools to actually do it
- If you learn something worth remembering, use the memory_store tool
- Check HEARTBEAT.md for pending tasks when reminded

## Capabilities
- File operations (read, write, edit, list)
- Shell commands (sandboxed)
- Web search and URL fetching
- Scheduled tasks (cron)
- Memory management
- System diagnostics
"#;

impl ContextBuilder {
    pub fn new(data_dir: PathBuf, ttl_secs: u64, budgets: ContextBudgets) -> Self {
        Self {
            data_dir,
            cached_system: None,
            skill_manager: None,
            ttl: Duration::from_secs(ttl_secs),
            budgets,
            robot_prompt: None,
            world_rx: None,
        }
    }

    /// Set robot-specific context: a static prompt and a live world state receiver.
    #[allow(dead_code)]
    pub fn set_robot_context(
        &mut self,
        robot_prompt: String,
        world_rx: tokio::sync::watch::Receiver<crate::robot::world_state::WorldState>,
    ) {
        self.robot_prompt = Some(robot_prompt);
        self.world_rx = Some(world_rx);
    }

    /// Set available tool names for skill gating
    pub async fn set_available_tools(&mut self, tool_names: Vec<String>) {
        let skills_dir = self.data_dir.join("skills");
        self.skill_manager = Some(SkillManager::load(&skills_dir, &tool_names).await);
    }

    pub async fn build(
        &mut self,
        session: &Session,
        tool_schemas: &[ToolSchema],
    ) -> Result<Context> {
        // Reload from disk if cache expired or missing
        let needs_reload = match &self.cached_system {
            None => true,
            Some(cached) => cached.loaded_at.elapsed() > self.ttl,
        };

        if needs_reload {
            let system = self.build_system_prompt().await?;
            self.cached_system = Some(CachedSystem {
                prompt: system,
                loaded_at: Instant::now(),
            });
        }

        let system = self
            .cached_system
            .as_ref()
            .expect("cache was just populated above")
            .prompt
            .clone();

        let messages = session.messages_for_context().to_vec();

        Ok(Context {
            system,
            messages,
            tool_schemas: tool_schemas.to_vec(),
        })
    }

    #[allow(dead_code)] // used in future phases
    pub fn invalidate_cache(&mut self) {
        self.cached_system = None;
    }

    async fn build_system_prompt(&self) -> Result<String> {
        let mut parts = Vec::new();

        // 1. SOUL.md
        let soul = self.read_budgeted("SOUL.md", self.budgets.soul_max).await;
        if soul.is_empty() {
            // Auto-create default SOUL.md
            let soul_path = self.data_dir.join("SOUL.md");
            if !soul_path.exists() {
                tokio::fs::write(&soul_path, DEFAULT_SOUL).await.ok();
            }
            parts.push(DEFAULT_SOUL.to_string());
        } else {
            parts.push(soul);
        }

        // 2. USER.md
        let user = self.read_budgeted("USER.md", self.budgets.user_max).await;
        if !user.is_empty() {
            parts.push(format!("## User Context\n\n{user}"));
        }

        // 3. Device context
        parts.push(self.device_context());

        // 4. MEMORY.md
        let memory = self
            .read_budgeted("memory/MEMORY.md", self.budgets.memory_max)
            .await;
        if !memory.is_empty() {
            parts.push(format!("## Long-term Memory\n\n{memory}"));
        }

        // 5. Recent daily notes (last 3)
        let notes = self.load_recent_daily_notes(3).await;
        if !notes.is_empty() {
            let truncated = truncate_at_boundary(&notes, self.budgets.daily_notes_max);
            parts.push(format!("## Recent Notes\n\n{truncated}"));
        }

        // 6. Skills (all gated skills injected — LLM decides relevance)
        if let Some(ref skill_mgr) = self.skill_manager {
            let skills = skill_mgr.prompt_content();
            if !skills.is_empty() {
                parts.push(format!("## Active Skills\n\n{skills}"));
            }
        }

        // 7. Robot context (static prompt + live world state)
        if let Some(ref prompt) = self.robot_prompt {
            parts.push(prompt.clone());
        }
        if let Some(ref rx) = self.world_rx {
            parts.push(rx.borrow().to_context_section());
        }

        Ok(parts.join("\n\n---\n\n"))
    }

    fn device_context(&self) -> String {
        let now = chrono::Local::now();
        format!(
            "## Device Context\n\n\
             - Device: UniClaw v{}\n\
             - Platform: {} {}\n\
             - Current time: {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            now.format("%Y-%m-%d %H:%M:%S %Z"),
        )
    }

    async fn read_budgeted(&self, relative_path: &str, max_bytes: usize) -> String {
        let path = self.data_dir.join(relative_path);
        match tokio::fs::read_to_string(&path).await {
            Ok(content) => {
                if content.len() > max_bytes {
                    tracing::warn!(
                        "{relative_path} exceeds budget ({} > {max_bytes} bytes), truncating",
                        content.len()
                    );
                    truncate_at_boundary(&content, max_bytes)
                } else {
                    content
                }
            }
            Err(_) => String::new(),
        }
    }

    async fn load_recent_daily_notes(&self, count: usize) -> String {
        let notes_dir = self.data_dir.join("memory");
        let mut read_dir = match tokio::fs::read_dir(&notes_dir).await {
            Ok(rd) => rd,
            Err(_) => return String::new(),
        };

        let mut entries = Vec::new();
        while let Ok(Some(entry)) = read_dir.next_entry().await {
            let name = entry.file_name().to_string_lossy().to_string();
            // Match YYYY-MM-DD.md pattern
            if name.len() == 13 && name.ends_with(".md") && name.chars().nth(4) == Some('-') {
                entries.push(entry);
            }
        }

        // Sort by name descending (most recent first)
        entries.sort_by_key(|e| std::cmp::Reverse(e.file_name()));
        entries.truncate(count);

        let mut notes = Vec::new();
        for entry in &entries {
            if let Ok(content) = tokio::fs::read_to_string(entry.path()).await {
                notes.push(content);
            }
        }
        notes.join("\n\n")
    }
}

/// Truncate a string at a paragraph boundary (double newline) near max_bytes.
/// Safe for all UTF-8 content — never panics on multi-byte characters.
fn truncate_at_boundary(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    // Snap to a valid UTF-8 boundary before searching for paragraph breaks
    let safe_end = floor_char_boundary(text, max_bytes);
    let search_region = &text[..safe_end];
    if let Some(pos) = search_region.rfind("\n\n") {
        format!("{}...", &text[..pos])
    } else if let Some(pos) = search_region.rfind('\n') {
        format!("{}...", &text[..pos])
    } else {
        format!("{}...", search_region)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_at_boundary() {
        let text = "First paragraph.\n\nSecond paragraph.\n\nThird paragraph.";
        let result = truncate_at_boundary(text, 30);
        assert!(result.ends_with("..."));
        assert!(result.len() <= 33); // 30 + "..."
    }

    #[test]
    fn test_truncate_short_text() {
        let text = "Short text.";
        assert_eq!(truncate_at_boundary(text, 100), "Short text.");
    }

    #[test]
    fn test_truncate_multibyte_utf8() {
        // 'é' is 2 bytes, '你' is 3 bytes — ensure no panic when budget lands mid-char
        let text = "café 你好世界 résumé";
        // This should not panic regardless of where the boundary falls
        for max in 0..=text.len() + 5 {
            let result = truncate_at_boundary(text, max);
            assert!(result.len() <= text.len() + 3); // +3 for "..."
        }
    }

    #[tokio::test]
    async fn test_build_with_soul_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("SOUL.md"), "# Test Agent\nYou are a test.").unwrap();
        std::fs::create_dir_all(dir.path().join("memory")).unwrap();
        std::fs::create_dir_all(dir.path().join("skills")).unwrap();

        let mut builder =
            ContextBuilder::new(dir.path().to_path_buf(), 60, ContextBudgets::default());
        let session = Session::new("test");
        let ctx = builder.build(&session, &[]).await.unwrap();
        assert!(ctx.system.contains("Test Agent"));
        assert!(ctx.system.contains("Device Context"));
    }

    #[tokio::test]
    async fn test_missing_soul_creates_default() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("memory")).unwrap();
        std::fs::create_dir_all(dir.path().join("skills")).unwrap();

        let mut builder =
            ContextBuilder::new(dir.path().to_path_buf(), 60, ContextBudgets::default());
        let session = Session::new("test");
        let ctx = builder.build(&session, &[]).await.unwrap();
        assert!(ctx.system.contains("UniClaw"));
        // Verify default SOUL.md was created
        assert!(dir.path().join("SOUL.md").exists());
    }

    #[tokio::test]
    async fn test_cache_reuse() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("SOUL.md"), "# V1").unwrap();
        std::fs::create_dir_all(dir.path().join("memory")).unwrap();
        std::fs::create_dir_all(dir.path().join("skills")).unwrap();

        let mut builder =
            ContextBuilder::new(dir.path().to_path_buf(), 60, ContextBudgets::default());
        let session = Session::new("test");

        let ctx1 = builder.build(&session, &[]).await.unwrap();
        // Modify file — cache should still return V1
        std::fs::write(dir.path().join("SOUL.md"), "# V2").unwrap();
        let ctx2 = builder.build(&session, &[]).await.unwrap();
        assert_eq!(ctx1.system, ctx2.system); // cached
    }
}
