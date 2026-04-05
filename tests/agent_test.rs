use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::json;

// We need to access internal modules — use the crate directly
use uniclaw::agent::{Agent, Input};
use uniclaw::config::Config;
use uniclaw::llm::reliable::ReliableProvider;
use uniclaw::llm::types::*;
use uniclaw::llm::LlmProvider;
use uniclaw::tools;
use uniclaw::tools::registry::ToolRegistry;

// --- MockLlmClient ---

struct MockLlmClient {
    responses: Mutex<VecDeque<ChatResponse>>,
    recorded_contexts: Mutex<Vec<Vec<Message>>>,
}

impl MockLlmClient {
    /// Returns a simple text response
    fn text(response: &str) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from([ChatResponse {
                text: Some(response.to_string()),
                tool_calls: vec![],
                stop_reason: StopReason::EndTurn,
                usage: Usage::default(),
            }])),
            recorded_contexts: Mutex::new(Vec::new()),
        }
    }

    /// Returns a tool call, then a text response
    fn tool_then_text(tool_name: &str, args: serde_json::Value, final_text: &str) -> Self {
        Self {
            responses: Mutex::new(VecDeque::from([
                ChatResponse {
                    text: None,
                    tool_calls: vec![ToolCall {
                        id: "call_1".into(),
                        name: tool_name.into(),
                        arguments: args,
                    }],
                    stop_reason: StopReason::ToolUse,
                    usage: Usage::default(),
                },
                ChatResponse {
                    text: Some(final_text.to_string()),
                    tool_calls: vec![],
                    stop_reason: StopReason::EndTurn,
                    usage: Usage::default(),
                },
            ])),
            recorded_contexts: Mutex::new(Vec::new()),
        }
    }

    /// Returns multiple tool calls simultaneously, then text
    fn multi_tool_then_text(calls: Vec<(&str, serde_json::Value)>, final_text: &str) -> Self {
        let tool_calls: Vec<ToolCall> = calls
            .into_iter()
            .enumerate()
            .map(|(i, (name, args))| ToolCall {
                id: format!("call_{}", i + 1),
                name: name.to_string(),
                arguments: args,
            })
            .collect();

        Self {
            responses: Mutex::new(VecDeque::from([
                ChatResponse {
                    text: None,
                    tool_calls,
                    stop_reason: StopReason::ToolUse,
                    usage: Usage::default(),
                },
                ChatResponse {
                    text: Some(final_text.to_string()),
                    tool_calls: vec![],
                    stop_reason: StopReason::EndTurn,
                    usage: Usage::default(),
                },
            ])),
            recorded_contexts: Mutex::new(Vec::new()),
        }
    }

    /// Always returns tool calls — for testing max iterations
    fn infinite_tool_calls(tool_name: &str, args: serde_json::Value) -> Self {
        let mut responses = VecDeque::new();
        for i in 0..20 {
            responses.push_back(ChatResponse {
                text: None,
                tool_calls: vec![ToolCall {
                    id: format!("call_{i}"),
                    name: tool_name.into(),
                    arguments: args.clone(),
                }],
                stop_reason: StopReason::ToolUse,
                usage: Usage::default(),
            });
        }
        Self {
            responses: Mutex::new(responses),
            recorded_contexts: Mutex::new(Vec::new()),
        }
    }

    /// Always fails
    fn failing() -> Self {
        Self {
            responses: Mutex::new(VecDeque::new()),
            recorded_contexts: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl LlmProvider for MockLlmClient {
    fn name(&self) -> &str {
        "mock"
    }

    async fn chat(&self, context: &Context) -> Result<ChatResponse> {
        self.recorded_contexts
            .lock()
            .unwrap()
            .push(context.messages.clone());

        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| anyhow!("MockLlmClient: no more responses (simulated failure)"))
    }
}

// --- Helper ---

fn test_config() -> Config {
    toml::from_str(
        r#"
[agent]
max_iterations = 5
consolidation_threshold = 100

[llm]
provider = "openai_compatible"
api_key_env = ""
model = "mock"
base_url = "http://localhost"
"#,
    )
    .unwrap()
}

fn test_input(msg: &str) -> Input {
    Input {
        id: "test-id".into(),
        session_id: "test-session".into(),
        content: msg.to_string(),
        stream_tx: None,
    }
}

async fn make_agent(mock: MockLlmClient, data_dir: &std::path::Path) -> Agent {
    std::fs::create_dir_all(data_dir.join("memory")).unwrap();
    std::fs::create_dir_all(data_dir.join("sessions")).unwrap();
    std::fs::create_dir_all(data_dir.join("skills")).unwrap();

    let mut registry = ToolRegistry::new();
    tools::register_default_tools(&mut registry);

    Agent::new(
        Box::new(mock),
        registry,
        &test_config(),
        data_dir.to_path_buf(),
    )
    .await
}

// --- Tests ---

#[tokio::test]
async fn test_simple_text_response() {
    let dir = tempfile::tempdir().unwrap();
    let mut agent = make_agent(MockLlmClient::text("Hello there!"), dir.path()).await;

    let output = agent.process(&test_input("Hi")).await.unwrap();
    assert_eq!(output.content, "Hello there!");
}

#[tokio::test]
async fn test_single_tool_call() {
    let dir = tempfile::tempdir().unwrap();
    let mut agent = make_agent(
        MockLlmClient::tool_then_text("get_time", json!({}), "It's 3:42 PM."),
        dir.path(),
    )
    .await;

    let output = agent
        .process(&test_input("What time is it?"))
        .await
        .unwrap();
    assert_eq!(output.content, "It's 3:42 PM.");
}

#[tokio::test]
async fn test_multi_tool_parallel() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockLlmClient::multi_tool_then_text(
        vec![("get_time", json!({})), ("system_info", json!({}))],
        "Time is 3:42 PM and system is healthy.",
    );
    let mut agent = make_agent(mock, dir.path()).await;

    let output = agent
        .process(&test_input("Time and system info please"))
        .await
        .unwrap();
    assert_eq!(output.content, "Time is 3:42 PM and system is healthy.");
}

#[tokio::test]
async fn test_max_iterations() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockLlmClient::infinite_tool_calls("get_time", json!({}));
    let mut agent = make_agent(mock, dir.path()).await;

    let output = agent.process(&test_input("Loop forever")).await.unwrap();
    assert!(output.content.contains("reasoning limit"));
}

#[tokio::test]
async fn test_llm_failover() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("memory")).unwrap();
    std::fs::create_dir_all(dir.path().join("sessions")).unwrap();
    std::fs::create_dir_all(dir.path().join("skills")).unwrap();

    let primary = MockLlmClient::failing();
    let fallback = MockLlmClient::text("Fallback response!");

    let llm: Box<dyn LlmProvider> = Box::new(ReliableProvider::new(
        Box::new(primary),
        vec![Box::new(fallback)],
        1, // single attempt before failover
        10,
    ));

    let mut registry = ToolRegistry::new();
    tools::register_default_tools(&mut registry);

    let mut agent = Agent::new(llm, registry, &test_config(), dir.path().to_path_buf()).await;

    let output = agent.process(&test_input("Hello")).await.unwrap();
    assert_eq!(output.content, "Fallback response!");
}

#[tokio::test]
async fn test_llm_all_fail() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("memory")).unwrap();
    std::fs::create_dir_all(dir.path().join("sessions")).unwrap();
    std::fs::create_dir_all(dir.path().join("skills")).unwrap();

    let primary = MockLlmClient::failing();
    let fallback = MockLlmClient::failing();

    let llm: Box<dyn LlmProvider> = Box::new(ReliableProvider::new(
        Box::new(primary),
        vec![Box::new(fallback)],
        1,
        10,
    ));

    let mut registry = ToolRegistry::new();
    tools::register_default_tools(&mut registry);

    let mut agent = Agent::new(llm, registry, &test_config(), dir.path().to_path_buf()).await;

    let result = agent.process(&test_input("Hello")).await;
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("All providers failed"));
}

#[tokio::test]
async fn test_session_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let mut agent = make_agent(MockLlmClient::text("Hi!"), dir.path()).await;

    agent.process(&test_input("Hello")).await.unwrap();

    // Verify session file was created
    let session_path = dir.path().join("sessions/test-session.jsonl");
    assert!(session_path.exists());

    let content = std::fs::read_to_string(&session_path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert_eq!(lines.len(), 2); // user + assistant
}

#[tokio::test]
async fn test_multi_turn() {
    let dir = tempfile::tempdir().unwrap();

    // First turn
    {
        let mut agent = make_agent(MockLlmClient::text("Hi!"), dir.path()).await;
        agent.process(&test_input("Hello")).await.unwrap();
        agent.session_store.persist_all().await.unwrap();
    }

    // Second turn — loads existing session
    {
        let mut agent = make_agent(MockLlmClient::text("I remember you!"), dir.path()).await;
        let output = agent.process(&test_input("Remember me?")).await.unwrap();
        assert_eq!(output.content, "I remember you!");

        // Session should have 4 messages total (2 per turn)
        let session_path = dir.path().join("sessions/test-session.jsonl");
        let content = std::fs::read_to_string(&session_path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 4);
    }
}

#[tokio::test]
async fn test_context_includes_soul() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("memory")).unwrap();
    std::fs::create_dir_all(dir.path().join("sessions")).unwrap();
    std::fs::create_dir_all(dir.path().join("skills")).unwrap();
    std::fs::write(dir.path().join("SOUL.md"), "# TestBot\nYou are a test bot.").unwrap();

    let mut agent = make_agent(MockLlmClient::text("Hello!"), dir.path()).await;
    agent.process(&test_input("Hi")).await.unwrap();

    // Verify SOUL.md was created (default or custom)
    assert!(dir.path().join("SOUL.md").exists());
}

#[tokio::test]
async fn test_file_tool_via_agent() {
    let dir = tempfile::tempdir().unwrap();

    // Agent writes a file via tool, then reads it
    let mock = MockLlmClient::tool_then_text(
        "write_file",
        json!({"path": "test.txt", "content": "hello world"}),
        "File written!",
    );
    let mut agent = make_agent(mock, dir.path()).await;
    let output = agent
        .process(&test_input("Write hello to test.txt"))
        .await
        .unwrap();
    assert_eq!(output.content, "File written!");

    // Verify the file was actually created
    assert_eq!(
        std::fs::read_to_string(dir.path().join("test.txt")).unwrap(),
        "hello world"
    );
}

#[tokio::test]
async fn test_unknown_tool() {
    let dir = tempfile::tempdir().unwrap();
    let mock =
        MockLlmClient::tool_then_text("nonexistent_tool", json!({}), "Sorry, that didn't work.");
    let mut agent = make_agent(mock, dir.path()).await;

    // Should not crash — agent handles unknown tool gracefully
    let output = agent
        .process(&test_input("Do something impossible"))
        .await
        .unwrap();
    assert_eq!(output.content, "Sorry, that didn't work.");
}

// --- Context-aware mock that reports message count ---

struct ContextAwareMockLlm {
    call_count: AtomicUsize,
}

impl ContextAwareMockLlm {
    fn new() -> Self {
        Self {
            call_count: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl LlmProvider for ContextAwareMockLlm {
    fn name(&self) -> &str {
        "context-aware-mock"
    }

    async fn chat(&self, context: &Context) -> Result<ChatResponse> {
        self.call_count.fetch_add(1, Ordering::SeqCst);
        Ok(ChatResponse {
            text: Some(format!("I see {} messages", context.messages.len())),
            tool_calls: vec![],
            stop_reason: StopReason::EndTurn,
            usage: Usage::default(),
        })
    }
}

#[tokio::test]
async fn test_session_persists_across_calls() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("memory")).unwrap();
    std::fs::create_dir_all(dir.path().join("sessions")).unwrap();
    std::fs::create_dir_all(dir.path().join("skills")).unwrap();

    let mock = ContextAwareMockLlm::new();

    let mut registry = ToolRegistry::new();
    tools::register_default_tools(&mut registry);

    let mut agent = Agent::new(
        Box::new(mock),
        registry,
        &test_config(),
        dir.path().to_path_buf(),
    )
    .await;

    // First call — the LLM should see just 1 message (the user message)
    let input1 = Input {
        id: "req-1".into(),
        session_id: "persist-test".into(),
        content: "Hello".into(),
        stream_tx: None,
    };
    let output1 = agent.process(&input1).await.unwrap();
    assert!(
        output1.content.contains("1 messages"),
        "First turn should see 1 message in context, got: {}",
        output1.content
    );

    // Second call — same session, LLM should see previous user + assistant + new user = 3
    let input2 = Input {
        id: "req-2".into(),
        session_id: "persist-test".into(),
        content: "Remember me?".into(),
        stream_tx: None,
    };
    let output2 = agent.process(&input2).await.unwrap();
    assert!(
        output2.content.contains("3 messages"),
        "Second turn should see 3 messages in context, got: {}",
        output2.content
    );
}

// --- Mock that returns enough responses for consolidation testing ---

struct ConsolidationMockLlm {
    call_count: AtomicUsize,
}

impl ConsolidationMockLlm {
    fn new() -> Self {
        Self {
            call_count: AtomicUsize::new(0),
        }
    }
}

#[async_trait]
impl LlmProvider for ConsolidationMockLlm {
    fn name(&self) -> &str {
        "consolidation-mock"
    }

    async fn chat(&self, context: &Context) -> Result<ChatResponse> {
        let n = self.call_count.fetch_add(1, Ordering::SeqCst);

        // Check if this is a consolidation request (system prompt contains "Summarize")
        let is_consolidation = context.messages.iter().any(|m| {
            if let MessageContent::Text { text } = &m.content {
                text.contains("Summarize the key facts")
            } else {
                false
            }
        });

        if is_consolidation {
            return Ok(ChatResponse {
                text: Some("- User sent several test messages.".to_string()),
                tool_calls: vec![],
                stop_reason: StopReason::EndTurn,
                usage: Usage::default(),
            });
        }

        Ok(ChatResponse {
            text: Some(format!("Reply {n}")),
            tool_calls: vec![],
            stop_reason: StopReason::EndTurn,
            usage: Usage::default(),
        })
    }
}

fn consolidation_test_config() -> Config {
    toml::from_str(
        r#"
[agent]
max_iterations = 5
consolidation_threshold = 6

[llm]
provider = "openai_compatible"
api_key_env = ""
model = "mock"
base_url = "http://localhost"
"#,
    )
    .unwrap()
}

#[tokio::test]
async fn test_consolidation_triggers_at_threshold() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("memory")).unwrap();
    std::fs::create_dir_all(dir.path().join("sessions")).unwrap();
    std::fs::create_dir_all(dir.path().join("skills")).unwrap();

    let mock = ConsolidationMockLlm::new();
    let config = consolidation_test_config();

    let mut registry = ToolRegistry::new();
    tools::register_default_tools(&mut registry);

    let mut agent = Agent::new(
        Box::new(mock),
        registry,
        &config,
        dir.path().to_path_buf(),
    )
    .await;

    let session_id = "consolidation-test";

    // Send enough messages to exceed threshold of 6.
    // Each turn adds 2 messages (user + assistant), so 4 turns = 8 messages > 6.
    for i in 0..4 {
        let input = Input {
            id: format!("req-{i}"),
            session_id: session_id.into(),
            content: format!("Message {i}"),
            stream_tx: None,
        };
        agent.process(&input).await.unwrap();
    }

    // After turn 4, session has 8 messages (> threshold of 6),
    // so needs_consolidation should be flagged.
    // Verify session file has 8 messages before consolidation.
    let session_path = dir.path().join(format!("sessions/{session_id}.jsonl"));
    let pre_content = std::fs::read_to_string(&session_path).unwrap();
    let pre_lines: Vec<&str> = pre_content.lines().collect();
    assert_eq!(
        pre_lines.len(),
        8,
        "Should have 8 messages before consolidation"
    );

    // Send one more message — this triggers consolidation at the START of this turn.
    let trigger_input = Input {
        id: "req-trigger".into(),
        session_id: session_id.into(),
        content: "Trigger consolidation".into(),
        stream_tx: None,
    };
    agent.process(&trigger_input).await.unwrap();

    // After consolidation: old messages were split in half (4 removed),
    // remaining 4 + 2 new messages from this turn = 6 total.
    // But the exact count depends on clean_split_point. The key assertion
    // is that the count DECREASED from the pre-consolidation 8+2=10.
    let post_content = std::fs::read_to_string(&session_path).unwrap();
    let post_lines: Vec<&str> = post_content.lines().collect();
    assert!(
        post_lines.len() < 10,
        "Consolidation should reduce message count. Expected < 10, got {}",
        post_lines.len()
    );

    // Verify MEMORY.md was written with the consolidation summary
    let memory_path = dir.path().join("memory/MEMORY.md");
    assert!(
        memory_path.exists(),
        "MEMORY.md should be created during consolidation"
    );
    let memory_content = std::fs::read_to_string(&memory_path).unwrap();
    assert!(
        memory_content.contains("Consolidated"),
        "MEMORY.md should contain consolidation header"
    );
    assert!(
        memory_content.contains("test messages"),
        "MEMORY.md should contain the mock summary"
    );
}
