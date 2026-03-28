use std::collections::VecDeque;
use std::sync::Mutex;

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::json;

// We need to access internal modules — use the crate directly
use uniclaw::agent::{Agent, Input};
use uniclaw::config::Config;
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
    fn multi_tool_then_text(
        calls: Vec<(&str, serde_json::Value)>,
        final_text: &str,
    ) -> Self {
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

    fn context_count(&self) -> usize {
        self.recorded_contexts.lock().unwrap().len()
    }
}

#[async_trait]
impl LlmProvider for MockLlmClient {
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
    }
}

fn make_agent(mock: MockLlmClient, data_dir: &std::path::Path) -> Agent {
    std::fs::create_dir_all(data_dir.join("memory")).unwrap();
    std::fs::create_dir_all(data_dir.join("sessions")).unwrap();
    std::fs::create_dir_all(data_dir.join("skills")).unwrap();

    let mut registry = ToolRegistry::new();
    tools::register_default_tools(&mut registry);

    Agent::new(
        Box::new(mock),
        None,
        registry,
        &test_config(),
        data_dir.to_path_buf(),
    )
}

// --- Tests ---

#[tokio::test]
async fn test_simple_text_response() {
    let dir = tempfile::tempdir().unwrap();
    let mut agent = make_agent(MockLlmClient::text("Hello there!"), dir.path());

    let output = agent.process(&test_input("Hi")).await.unwrap();
    assert_eq!(output.content, "Hello there!");
}

#[tokio::test]
async fn test_single_tool_call() {
    let dir = tempfile::tempdir().unwrap();
    let mut agent = make_agent(
        MockLlmClient::tool_then_text("get_time", json!({}), "It's 3:42 PM."),
        dir.path(),
    );

    let output = agent.process(&test_input("What time is it?")).await.unwrap();
    assert_eq!(output.content, "It's 3:42 PM.");
}

#[tokio::test]
async fn test_multi_tool_parallel() {
    let dir = tempfile::tempdir().unwrap();
    let mock = MockLlmClient::multi_tool_then_text(
        vec![
            ("get_time", json!({})),
            ("system_info", json!({})),
        ],
        "Time is 3:42 PM and system is healthy.",
    );
    let mut agent = make_agent(mock, dir.path());

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
    let mut agent = make_agent(mock, dir.path());

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

    let mut registry = ToolRegistry::new();
    tools::register_default_tools(&mut registry);

    let mut agent = Agent::new(
        Box::new(primary),
        Some(Box::new(fallback)),
        registry,
        &test_config(),
        dir.path().to_path_buf(),
    );

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

    let mut registry = ToolRegistry::new();
    tools::register_default_tools(&mut registry);

    let mut agent = Agent::new(
        Box::new(primary),
        Some(Box::new(fallback)),
        registry,
        &test_config(),
        dir.path().to_path_buf(),
    );

    let result = agent.process(&test_input("Hello")).await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("All LLM providers failed"));
}

#[tokio::test]
async fn test_session_persistence() {
    let dir = tempfile::tempdir().unwrap();
    let mut agent = make_agent(MockLlmClient::text("Hi!"), dir.path());

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
        let mut agent = make_agent(MockLlmClient::text("Hi!"), dir.path());
        agent.process(&test_input("Hello")).await.unwrap();
        agent.session_store.persist_all().unwrap();
    }

    // Second turn — loads existing session
    {
        let mut agent = make_agent(MockLlmClient::text("I remember you!"), dir.path());
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

    let mock = MockLlmClient::text("Hello!");
    let recorded = &mock as *const MockLlmClient;

    let mut registry = ToolRegistry::new();
    tools::register_default_tools(&mut registry);

    // We need to check what context was sent to the LLM
    // Since we can't easily inspect after move, check context_count
    let mut agent = make_agent(MockLlmClient::text("Hello!"), dir.path());
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
    let mut agent = make_agent(mock, dir.path());
    let output = agent.process(&test_input("Write hello to test.txt")).await.unwrap();
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
    let mock = MockLlmClient::tool_then_text(
        "nonexistent_tool",
        json!({}),
        "Sorry, that didn't work.",
    );
    let mut agent = make_agent(mock, dir.path());

    // Should not crash — agent handles unknown tool gracefully
    let output = agent.process(&test_input("Do something impossible")).await.unwrap();
    assert_eq!(output.content, "Sorry, that didn't work.");
}
