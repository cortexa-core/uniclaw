use anyhow::Result;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::llm::types::{Context, ToolSchema};
use super::memory::Session;

pub struct ContextBuilder {
    data_dir: PathBuf,
    cached_system: Option<CachedSystem>,
    ttl: Duration,
    budgets: ContextBudgets,
}

struct CachedSystem {
    prompt: String,
    loaded_at: Instant,
}

struct ContextBudgets {
    soul_max: usize,
    user_max: usize,
    memory_max: usize,
    daily_notes_max: usize,
    skills_max: usize,
}

impl Default for ContextBudgets {
    fn default() -> Self {
        Self {
            soul_max: 4096,
            user_max: 2048,
            memory_max: 4096,
            daily_notes_max: 3072,
            skills_max: 2048,
        }
    }
}

pub const DEFAULT_SOUL: &str = r#"# MiniClaw

You are MiniClaw, a helpful AI assistant running on a local device.

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
    pub fn new(data_dir: PathBuf, ttl_secs: u64) -> Self {
        Self {
            data_dir,
            cached_system: None,
            ttl: Duration::from_secs(ttl_secs),
            budgets: ContextBudgets::default(),
        }
    }

    pub fn build(
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
            let system = self.build_system_prompt()?;
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
        let messages = session.messages_for_context();

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

    fn build_system_prompt(&self) -> Result<String> {
        let mut parts = Vec::new();

        // 1. SOUL.md
        let soul = self.read_budgeted("SOUL.md", self.budgets.soul_max);
        if soul.is_empty() {
            // Auto-create default SOUL.md
            let soul_path = self.data_dir.join("SOUL.md");
            if !soul_path.exists() {
                std::fs::write(&soul_path, DEFAULT_SOUL).ok();
            }
            parts.push(DEFAULT_SOUL.to_string());
        } else {
            parts.push(soul);
        }

        // 2. USER.md
        let user = self.read_budgeted("USER.md", self.budgets.user_max);
        if !user.is_empty() {
            parts.push(format!("## User Context\n\n{user}"));
        }

        // 3. Device context
        parts.push(self.device_context());

        // 4. MEMORY.md
        let memory = self.read_budgeted("memory/MEMORY.md", self.budgets.memory_max);
        if !memory.is_empty() {
            parts.push(format!("## Long-term Memory\n\n{memory}"));
        }

        // 5. Recent daily notes (last 3)
        let notes = self.load_recent_daily_notes(3);
        if !notes.is_empty() {
            let truncated = truncate_at_boundary(&notes, self.budgets.daily_notes_max);
            parts.push(format!("## Recent Notes\n\n{truncated}"));
        }

        // 6. Skills
        let skills = self.load_skills();
        if !skills.is_empty() {
            let truncated = truncate_at_boundary(&skills, self.budgets.skills_max);
            parts.push(format!("## Available Skills\n\n{truncated}"));
        }

        Ok(parts.join("\n\n---\n\n"))
    }

    fn device_context(&self) -> String {
        let now = chrono::Local::now();
        format!(
            "## Device Context\n\n\
             - Device: MiniClaw v{}\n\
             - Platform: {} {}\n\
             - Current time: {}",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            now.format("%Y-%m-%d %H:%M:%S %Z"),
        )
    }

    fn read_budgeted(&self, relative_path: &str, max_bytes: usize) -> String {
        let path = self.data_dir.join(relative_path);
        match std::fs::read_to_string(&path) {
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

    fn load_recent_daily_notes(&self, count: usize) -> String {
        let notes_dir = self.data_dir.join("memory");
        let mut entries: Vec<_> = std::fs::read_dir(&notes_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                // Match YYYY-MM-DD.md pattern
                name.len() == 13 && name.ends_with(".md") && name.chars().nth(4) == Some('-')
            })
            .collect();

        // Sort by name descending (most recent first)
        entries.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
        entries.truncate(count);

        entries
            .iter()
            .filter_map(|e| std::fs::read_to_string(e.path()).ok())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn load_skills(&self) -> String {
        let skills_dir = self.data_dir.join("skills");
        let entries = match std::fs::read_dir(&skills_dir) {
            Ok(e) => e,
            Err(_) => return String::new(),
        };

        entries
            .flatten()
            .filter(|e| {
                e.file_name().to_string_lossy().ends_with(".md")
            })
            .filter_map(|e| {
                let content = std::fs::read_to_string(e.path()).ok()?;
                Some(content)
            })
            .collect::<Vec<_>>()
            .join("\n\n---\n\n")
    }
}

/// Truncate a string at a paragraph boundary (double newline) near max_bytes.
fn truncate_at_boundary(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.to_string();
    }
    // Find the last paragraph break before max_bytes
    let search_region = &text[..max_bytes];
    if let Some(pos) = search_region.rfind("\n\n") {
        format!("{}...", &text[..pos])
    } else if let Some(pos) = search_region.rfind('\n') {
        format!("{}...", &text[..pos])
    } else {
        format!("{}...", &text[..max_bytes])
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
    fn test_build_with_soul_only() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("SOUL.md"), "# Test Agent\nYou are a test.").unwrap();
        std::fs::create_dir_all(dir.path().join("memory")).unwrap();
        std::fs::create_dir_all(dir.path().join("skills")).unwrap();

        let mut builder = ContextBuilder::new(dir.path().to_path_buf(), 60);
        let session = Session::new("test");
        let ctx = builder.build(&session, &[]).unwrap();
        assert!(ctx.system.contains("Test Agent"));
        assert!(ctx.system.contains("Device Context"));
    }

    #[test]
    fn test_missing_soul_creates_default() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("memory")).unwrap();
        std::fs::create_dir_all(dir.path().join("skills")).unwrap();

        let mut builder = ContextBuilder::new(dir.path().to_path_buf(), 60);
        let session = Session::new("test");
        let ctx = builder.build(&session, &[]).unwrap();
        assert!(ctx.system.contains("MiniClaw"));
        // Verify default SOUL.md was created
        assert!(dir.path().join("SOUL.md").exists());
    }

    #[test]
    fn test_cache_reuse() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("SOUL.md"), "# V1").unwrap();
        std::fs::create_dir_all(dir.path().join("memory")).unwrap();
        std::fs::create_dir_all(dir.path().join("skills")).unwrap();

        let mut builder = ContextBuilder::new(dir.path().to_path_buf(), 60);
        let session = Session::new("test");

        let ctx1 = builder.build(&session, &[]).unwrap();
        // Modify file — cache should still return V1
        std::fs::write(dir.path().join("SOUL.md"), "# V2").unwrap();
        let ctx2 = builder.build(&session, &[]).unwrap();
        assert_eq!(ctx1.system, ctx2.system); // cached
    }
}
