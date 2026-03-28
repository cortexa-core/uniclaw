use anyhow::Result;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::llm::types::{ChatResponse, Context, Message, MessageContent, Role};
use crate::llm::LlmProvider;
use crate::tools::registry::ToolResult;

// --- Session ---

#[derive(Debug, Serialize, Deserialize)]
pub struct Session {
    pub id: String,
    pub messages: Vec<Message>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub needs_consolidation: bool,
}

impl Session {
    pub fn new(id: &str) -> Self {
        Self {
            id: id.to_string(),
            messages: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
            needs_consolidation: false,
        }
    }

    pub fn add_message(&mut self, role: Role, content: &str) {
        self.messages.push(Message {
            role,
            content: MessageContent::Text { text: content.to_string() },
        });
        self.updated_at = Utc::now();
    }

    pub fn add_tool_use_message(&mut self, response: &ChatResponse) {
        self.messages.push(Message::assistant_tool_use(
            response.text.clone(),
            response.tool_calls.clone(),
        ));
        self.updated_at = Utc::now();
    }

    pub fn add_tool_result(&mut self, tool_use_id: &str, result: ToolResult) {
        let content = match &result {
            ToolResult::Success(s) => s.clone(),
            ToolResult::Error(e) => format!("Error: {e}"),
        };
        self.messages.push(Message::tool_result(tool_use_id, &content));
        self.updated_at = Utc::now();
    }

    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Return messages formatted for LLM context
    pub fn messages_for_context(&self) -> Vec<Message> {
        self.messages.clone()
    }
}

// --- SessionStore ---

pub struct SessionStore {
    sessions: HashMap<String, Session>,
    data_dir: PathBuf,
}

impl SessionStore {
    pub fn new(data_dir: PathBuf) -> Self {
        Self {
            sessions: HashMap::new(),
            data_dir,
        }
    }

    pub async fn get_or_load(&mut self, id: &str) -> &mut Session {
        if !self.sessions.contains_key(id) {
            let session = self.load_from_disk(id).await.unwrap_or_else(|_| Session::new(id));
            self.sessions.insert(id.to_string(), session);
        }
        self.sessions
            .get_mut(id)
            .expect("session was just inserted; this is a bug if it fails")
    }

    pub async fn persist(&self, id: &str) -> Result<()> {
        if let Some(session) = self.sessions.get(id) {
            let sessions_dir = self.data_dir.join("sessions");
            tokio::fs::create_dir_all(&sessions_dir).await?;
            let path = sessions_dir.join(format!("{id}.jsonl"));
            let content: String = session
                .messages
                .iter()
                .filter_map(|m| {
                    match serde_json::to_string(m) {
                        Ok(json) => Some(json),
                        Err(e) => {
                            tracing::error!("Failed to serialize message in session {id}: {e}");
                            None
                        }
                    }
                })
                .collect::<Vec<_>>()
                .join("\n");
            tokio::fs::write(&path, content).await?;
            tracing::debug!("Persisted session {id} ({} messages)", session.messages.len());
        }
        Ok(())
    }

    pub async fn persist_all(&self) -> Result<()> {
        for id in self.sessions.keys() {
            self.persist(id).await?;
        }
        Ok(())
    }

    async fn load_from_disk(&self, id: &str) -> Result<Session> {
        let path = self.data_dir.join(format!("sessions/{id}.jsonl"));
        let content = tokio::fs::read_to_string(&path).await?;
        let messages: Vec<Message> = content
            .lines()
            .filter(|line| !line.trim().is_empty())
            .filter_map(|line| {
                match serde_json::from_str(line) {
                    Ok(msg) => Some(msg),
                    Err(e) => {
                        tracing::warn!("Skipping malformed message in session {id}: {e}");
                        None
                    }
                }
            })
            .collect();

        Ok(Session {
            id: id.to_string(),
            messages,
            created_at: Utc::now(), // approximate — could parse from file metadata
            updated_at: Utc::now(),
            needs_consolidation: false,
        })
    }
}

// --- MemoryManager ---

pub struct MemoryManager {
    data_dir: PathBuf,
}

impl MemoryManager {
    pub fn new(data_dir: PathBuf) -> Self {
        Self { data_dir }
    }

    #[allow(dead_code)] // used by future memory tools and consolidation
    pub async fn read_memory(&self) -> Result<String> {
        let path = self.data_dir.join("memory/MEMORY.md");
        Ok(tokio::fs::read_to_string(&path).await.unwrap_or_default())
    }

    #[allow(dead_code)]
    pub async fn append_memory(&self, key: &str, value: &str) -> Result<()> {
        let path = self.data_dir.join("memory/MEMORY.md");
        let mut content = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        let timestamp = chrono::Local::now().format("%Y-%m-%d %H:%M");
        content.push_str(&format!("\n- [{timestamp}] {key}: {value}"));
        tokio::fs::write(&path, content).await?;
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn append_daily_note(&self, note: &str) -> Result<()> {
        let date = chrono::Local::now().format("%Y-%m-%d");
        let path = self.data_dir.join(format!("memory/{date}.md"));

        let mut content = match tokio::fs::read_to_string(&path).await {
            Ok(c) => c,
            Err(_) => format!("## {date}\n"),
        };

        content.push_str(&format!("\n- {note}"));
        tokio::fs::write(&path, content).await?;
        Ok(())
    }

    /// Consolidate old messages from a session into MEMORY.md.
    /// Takes the older half of messages, asks the LLM to summarize them,
    /// appends the summary to MEMORY.md, and removes the old messages from the session.
    pub async fn consolidate(
        &self,
        session: &mut Session,
        llm: &dyn LlmProvider,
        memory_max_bytes: usize,
    ) -> Result<()> {
        if session.messages.len() < 4 {
            // Too few messages to consolidate
            return Ok(());
        }

        let split_point = session.messages.len() / 2;
        let old_messages = &session.messages[..split_point];

        // Build a text representation of old messages for summarization
        let conversation_text: String = old_messages
            .iter()
            .filter_map(|m| {
                let role = match m.role {
                    Role::User => "User",
                    Role::Assistant => "Assistant",
                    Role::Tool => return None, // skip tool results for summary
                };
                Some(format!("{}: {}", role, m.content_text()))
            })
            .collect::<Vec<_>>()
            .join("\n");

        if conversation_text.trim().is_empty() {
            // Nothing meaningful to consolidate
            session.messages = session.messages[split_point..].to_vec();
            return Ok(());
        }

        tracing::info!(
            "Consolidating session {} — summarizing {} messages",
            session.id,
            split_point
        );

        // Ask LLM to summarize
        let summary_prompt = format!(
            "Summarize the key facts, decisions, and user preferences from this conversation \
             in concise bullet points. Only include information worth remembering long-term. \
             Do NOT include greetings or trivial exchanges.\n\n{}",
            conversation_text
        );

        let context = Context::simple_query(&summary_prompt);
        let summary_response = llm.chat(&context).await;

        // Only discard old messages if the summary was actually persisted to disk.
        // This prevents data loss when the LLM fails or returns empty results.
        let mut summary_saved = false;

        match summary_response {
            Ok(response) => {
                if let Some(summary_text) = response.text {
                    if !summary_text.trim().is_empty() {
                        // Append summary to MEMORY.md
                        let memory_path = self.data_dir.join("memory/MEMORY.md");
                        let mut memory = tokio::fs::read_to_string(&memory_path).await.unwrap_or_default();
                        let date = chrono::Local::now().format("%Y-%m-%d %H:%M");
                        memory.push_str(&format!("\n\n### Consolidated {date}\n\n{summary_text}"));

                        // Check if MEMORY.md exceeds max size
                        if memory.len() > memory_max_bytes {
                            tracing::info!(
                                "MEMORY.md exceeds max size ({}B > {}B), reconsolidating",
                                memory.len(),
                                memory_max_bytes
                            );
                            memory = self.reconsolidate_memory(&memory, llm, memory_max_bytes).await;
                        }

                        tokio::fs::write(&memory_path, memory).await?;
                        tracing::info!("Consolidation summary written to MEMORY.md");
                        summary_saved = true;
                    } else {
                        tracing::warn!("Consolidation returned empty summary. Keeping messages.");
                    }
                } else {
                    tracing::warn!("Consolidation returned no text. Keeping messages.");
                }
            }
            Err(e) => {
                tracing::warn!("Consolidation LLM call failed: {e}. Keeping messages.");
            }
        }

        if summary_saved {
            // Remove old messages. Find a clean split point that doesn't orphan
            // tool results (which must follow their assistant tool_use message).
            let clean_split = find_clean_split_point(&session.messages, split_point);
            session.messages = session.messages[clean_split..].to_vec();

            // Strip any remaining orphaned tool messages at the start
            strip_orphaned_tool_messages(&mut session.messages);
        }

        session.needs_consolidation = false;

        Ok(())
    }

    /// When MEMORY.md exceeds max size, ask the LLM to condense it.
    async fn reconsolidate_memory(
        &self,
        memory: &str,
        llm: &dyn LlmProvider,
        max_bytes: usize,
    ) -> String {
        let prompt = format!(
            "Condense these notes into the most important facts only. \
             Remove redundant, outdated, or trivial information. \
             Keep it under {} characters:\n\n{}",
            max_bytes / 2,
            memory
        );

        let context = Context::simple_query(&prompt);
        match llm.chat(&context).await {
            Ok(response) => response.text.unwrap_or_else(|| memory.to_string()),
            Err(e) => {
                tracing::warn!("Memory reconsolidation failed: {e}. Keeping existing memory.");
                memory.to_string()
            }
        }
    }
}

/// Find a split point that doesn't break tool call/result pairs.
/// Walks backward from the desired split point to find a User message boundary.
fn find_clean_split_point(messages: &[Message], desired: usize) -> usize {
    // Walk backward from desired split to find a User message.
    // User messages are always safe split points — they don't depend on prior tool_use.
    let mut point = desired;
    while point > 0 {
        if matches!(messages[point].role, Role::User) {
            return point;
        }
        point -= 1;
    }
    // Fallback: if no user message found, use the desired point
    desired
}

/// Remove orphaned tool result messages at the start of the message list.
/// A tool result is orphaned if there's no preceding assistant ToolUse message.
fn strip_orphaned_tool_messages(messages: &mut Vec<Message>) {
    // Remove leading messages that are Tool role (orphaned results)
    // or Assistant ToolUse without their tool results completing
    while !messages.is_empty() {
        match &messages[0].role {
            Role::Tool => {
                tracing::debug!("Stripping orphaned tool result message");
                messages.remove(0);
            }
            Role::Assistant => {
                // If it's a ToolUse message with no following Tool results, strip it too
                if matches!(&messages[0].content, MessageContent::ToolUse { .. }) {
                    tracing::debug!("Stripping orphaned assistant tool_use message");
                    messages.remove(0);
                } else {
                    break;
                }
            }
            Role::User => break,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::types::{ChatResponse, StopReason, ToolCall, Usage};

    // --- Mock LLM for consolidation tests ---

    struct MockConsolidationLlm {
        response_text: String,
    }

    #[async_trait::async_trait]
    impl LlmProvider for MockConsolidationLlm {
        async fn chat(&self, _context: &Context) -> Result<ChatResponse> {
            Ok(ChatResponse {
                text: Some(self.response_text.clone()),
                tool_calls: vec![],
                stop_reason: StopReason::EndTurn,
                usage: Usage::default(),
            })
        }
    }

    #[tokio::test]
    async fn test_consolidation_basic() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("memory")).unwrap();
        let mgr = MemoryManager::new(dir.path().to_path_buf());
        let mock_llm = MockConsolidationLlm {
            response_text: "- User name is Jiekai\n- Prefers Celsius".into(),
        };

        let mut session = Session::new("test");
        // Add 10 messages
        for i in 0..5 {
            session.add_message(Role::User, &format!("Message {i}"));
            session.add_message(Role::Assistant, &format!("Response {i}"));
        }
        assert_eq!(session.message_count(), 10);

        mgr.consolidate(&mut session, &mock_llm, 8192).await.unwrap();

        // Session should have only the recent half
        // Session should have roughly half the messages (exact count depends on
        // clean split point finding a User message boundary)
        assert!(session.message_count() >= 4 && session.message_count() <= 7,
            "Expected 4-7 messages after consolidation, got {}", session.message_count());
        // First remaining message should be a User message (clean split)
        assert!(matches!(session.messages[0].role, Role::User));
        // MEMORY.md should contain the summary
        let memory = std::fs::read_to_string(dir.path().join("memory/MEMORY.md")).unwrap();
        assert!(memory.contains("User name is Jiekai"));
        assert!(memory.contains("Prefers Celsius"));
        assert!(memory.contains("Consolidated"));
    }

    #[tokio::test]
    async fn test_consolidation_too_few_messages() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("memory")).unwrap();
        let mgr = MemoryManager::new(dir.path().to_path_buf());
        let mock_llm = MockConsolidationLlm {
            response_text: "should not be called".into(),
        };

        let mut session = Session::new("test");
        session.add_message(Role::User, "Hi");
        session.add_message(Role::Assistant, "Hello");

        mgr.consolidate(&mut session, &mock_llm, 8192).await.unwrap();

        // Should not consolidate — too few messages
        assert_eq!(session.message_count(), 2);
    }

    #[tokio::test]
    async fn test_consolidation_memory_bounds() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("memory")).unwrap();

        // Pre-fill MEMORY.md with existing content near the limit
        let existing = "x".repeat(500);
        std::fs::write(dir.path().join("memory/MEMORY.md"), &existing).unwrap();

        let mgr = MemoryManager::new(dir.path().to_path_buf());
        let mock_llm = MockConsolidationLlm {
            response_text: "- Condensed facts here".into(),
        };

        let mut session = Session::new("test");
        for i in 0..6 {
            session.add_message(Role::User, &format!("Msg {i}"));
            session.add_message(Role::Assistant, &format!("Reply {i}"));
        }

        // Set a very small max to trigger reconsolidation
        mgr.consolidate(&mut session, &mock_llm, 100).await.unwrap();

        // Memory was reconsolidated (the mock returns "Condensed facts here" for both calls)
        let memory = std::fs::read_to_string(dir.path().join("memory/MEMORY.md")).unwrap();
        assert!(memory.contains("Condensed facts"));
        assert_eq!(session.message_count(), 6); // kept recent half
    }

    #[tokio::test]
    async fn test_consolidation_clears_flag() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("memory")).unwrap();
        let mgr = MemoryManager::new(dir.path().to_path_buf());
        let mock_llm = MockConsolidationLlm {
            response_text: "- Summary".into(),
        };

        let mut session = Session::new("test");
        session.needs_consolidation = true;
        for i in 0..6 {
            session.add_message(Role::User, &format!("Msg {i}"));
            session.add_message(Role::Assistant, &format!("Reply {i}"));
        }

        mgr.consolidate(&mut session, &mock_llm, 8192).await.unwrap();
        assert!(!session.needs_consolidation);
    }

    #[tokio::test]
    async fn test_consolidation_no_orphaned_tools() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("memory")).unwrap();
        let mgr = MemoryManager::new(dir.path().to_path_buf());
        let mock_llm = MockConsolidationLlm {
            response_text: "- User asked about time".into(),
        };

        let mut session = Session::new("test");
        // Simulate a conversation with tool calls
        session.add_message(Role::User, "What time is it?");
        session.messages.push(Message::assistant_tool_use(
            Some("Let me check.".into()),
            vec![ToolCall {
                id: "call_1".into(),
                name: "get_time".into(),
                arguments: serde_json::json!({}),
            }],
        ));
        session.messages.push(Message::tool_result("call_1", "3:42 PM"));
        session.add_message(Role::Assistant, "It's 3:42 PM.");
        // Add more messages to trigger consolidation
        for i in 0..8 {
            session.add_message(Role::User, &format!("Question {i}"));
            session.add_message(Role::Assistant, &format!("Answer {i}"));
        }

        mgr.consolidate(&mut session, &mock_llm, 8192).await.unwrap();

        // Verify no orphaned tool messages at the start
        if !session.messages.is_empty() {
            assert!(
                !matches!(session.messages[0].role, Role::Tool),
                "First message should not be an orphaned tool result"
            );
            // Verify the first message is a User message (clean split)
            assert!(
                matches!(session.messages[0].role, Role::User),
                "First message after consolidation should be a User message"
            );
        }
    }

    #[test]
    fn test_strip_orphaned_tool_messages() {
        let mut messages = vec![
            Message::tool_result("call_1", "some result"),
            Message::tool_result("call_2", "another result"),
            Message::user("Hello"),
            Message::assistant("Hi!"),
        ];
        strip_orphaned_tool_messages(&mut messages);
        assert_eq!(messages.len(), 2);
        assert!(matches!(messages[0].role, Role::User));
    }

    #[test]
    fn test_find_clean_split_point() {
        let messages = vec![
            Message::user("Q1"),                    // 0
            Message::assistant("A1"),               // 1
            Message::user("Q2"),                    // 2
            Message::assistant_tool_use(None, vec![ToolCall {
                id: "c1".into(), name: "t".into(), arguments: serde_json::json!({}),
            }]),                                     // 3
            Message::tool_result("c1", "result"),   // 4
            Message::assistant("A2"),               // 5
            Message::user("Q3"),                    // 6
            Message::assistant("A3"),               // 7
        ];

        // If desired split is at index 4 (tool result), should back up to index 2 (user)
        assert_eq!(find_clean_split_point(&messages, 4), 2);
        // If desired split is at index 6 (user), stays at 6
        assert_eq!(find_clean_split_point(&messages, 6), 6);
    }

    // --- Original tests ---

    #[test]
    fn test_session_add_messages() {
        let mut session = Session::new("test");
        session.add_message(Role::User, "Hello");
        session.add_message(Role::Assistant, "Hi there!");
        assert_eq!(session.message_count(), 2);
        assert_eq!(session.messages[0].content_text(), "Hello");
        assert_eq!(session.messages[1].content_text(), "Hi there!");
    }

    #[tokio::test]
    async fn test_session_roundtrip_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sessions")).unwrap();
        let mut store = SessionStore::new(dir.path().to_path_buf());

        // Create and populate session
        {
            let session = store.get_or_load("abc").await;
            session.add_message(Role::User, "Hello");
            session.add_message(Role::Assistant, "Hi!");
        }
        store.persist("abc").await.unwrap();

        // Load from fresh store
        let mut store2 = SessionStore::new(dir.path().to_path_buf());
        let session2 = store2.get_or_load("abc").await;
        assert_eq!(session2.message_count(), 2);
        assert_eq!(session2.messages[0].content_text(), "Hello");
    }

    #[tokio::test]
    async fn test_session_store_creates_new() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = SessionStore::new(dir.path().to_path_buf());
        let session = store.get_or_load("new-session").await;
        assert_eq!(session.id, "new-session");
        assert_eq!(session.message_count(), 0);
    }

    #[tokio::test]
    async fn test_memory_manager_append_and_read() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("memory")).unwrap();
        let mgr = MemoryManager::new(dir.path().to_path_buf());

        mgr.append_memory("name", "Jiekai").await.unwrap();
        mgr.append_memory("color", "blue").await.unwrap();

        let memory = mgr.read_memory().await.unwrap();
        assert!(memory.contains("name: Jiekai"));
        assert!(memory.contains("color: blue"));
    }

    #[tokio::test]
    async fn test_daily_note() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("memory")).unwrap();
        let mgr = MemoryManager::new(dir.path().to_path_buf());

        mgr.append_daily_note("User prefers Celsius").await.unwrap();
        mgr.append_daily_note("Created morning cron job").await.unwrap();

        let date = chrono::Local::now().format("%Y-%m-%d");
        let path = dir.path().join(format!("memory/{date}.md"));
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains("Celsius"));
        assert!(content.contains("morning cron"));
    }
}
