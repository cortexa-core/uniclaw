use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::sync::Arc;

use crate::config::Config;
use crate::llm::types::*;
use crate::llm::LlmProvider;
use crate::tools::registry::{ToolContext, ToolRegistry, ToolResult};

use super::context::ContextBuilder;
use super::memory::{MemoryManager, SessionStore};

pub struct Agent {
    llm: Box<dyn LlmProvider>,
    fallback_llm: Option<Box<dyn LlmProvider>>,
    pub tool_registry: ToolRegistry,
    pub memory: MemoryManager,
    pub session_store: SessionStore,
    context_builder: ContextBuilder,
    config: AgentConfig,
    data_dir: PathBuf,
    full_config: Arc<Config>,
}

pub struct AgentConfig {
    pub max_iterations: usize,
    pub max_tool_calls_per_iteration: usize,
    pub consolidation_threshold: usize,
    pub memory_max_bytes: usize,
    pub request_timeout_secs: u64,
}

/// Input to the agent — all sources normalize to this.
pub struct Input {
    #[allow(dead_code)] // used for logging and request tracking
    pub id: String,
    pub session_id: String,
    pub content: String,
}

/// Output from the agent.
#[derive(Debug)]
pub struct Output {
    pub content: String,
    pub usage: Option<Usage>,
}

impl Output {
    pub fn text(content: String) -> Self {
        Self { content, usage: None }
    }

    pub fn with_usage(content: String, usage: Usage) -> Self {
        Self { content, usage: Some(usage) }
    }
}

/// Validate a session ID for safe use in file paths.
/// Rejects path traversal characters, path separators, and overly long IDs.
fn validate_session_id(id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(anyhow!("Session ID cannot be empty"));
    }
    if id.len() > 128 {
        return Err(anyhow!("Session ID too long (max 128 characters)"));
    }
    // Allow only alphanumeric, hyphens, underscores, and dots (no path separators or ..)
    if !id.chars().all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.') {
        return Err(anyhow!(
            "Session ID contains invalid characters (only alphanumeric, hyphens, underscores, dots allowed)"
        ));
    }
    if id.contains("..") {
        return Err(anyhow!("Session ID cannot contain '..'"));
    }
    Ok(())
}

impl Agent {
    pub fn new(
        llm: Box<dyn LlmProvider>,
        fallback_llm: Option<Box<dyn LlmProvider>>,
        tool_registry: ToolRegistry,
        config: &Config,
        data_dir: PathBuf,
    ) -> Self {
        let agent_config = AgentConfig {
            max_iterations: config.agent.max_iterations,
            max_tool_calls_per_iteration: config.agent.max_tool_calls_per_iteration,
            consolidation_threshold: config.agent.consolidation_threshold,
            memory_max_bytes: config.agent.memory_max_bytes,
            request_timeout_secs: config.agent.request_timeout_secs,
        };

        // Initialize context builder with skill manager
        let mut context_builder = ContextBuilder::new(
            data_dir.clone(),
            config.agent.context_cache_ttl_secs,
        );
        let tool_names: Vec<String> = tool_registry.tool_names().iter().map(|s| s.to_string()).collect();
        context_builder.set_available_tools(tool_names);

        Self {
            llm,
            fallback_llm,
            tool_registry,
            memory: MemoryManager::new(data_dir.clone()),
            session_store: SessionStore::new(data_dir.clone()),
            context_builder,
            config: agent_config,
            full_config: Arc::new(config.clone()),
            data_dir,
        }
    }

    /// Process one input with a timeout guard.
    /// Called only by agent_worker task (sole owner).
    pub async fn process(&mut self, input: &Input) -> Result<Output> {
        validate_session_id(&input.session_id)?;

        let timeout = std::time::Duration::from_secs(self.config.request_timeout_secs);
        match tokio::time::timeout(timeout, self.process_inner(input)).await {
            Ok(result) => result,
            Err(_) => {
                tracing::warn!(
                    "Request timed out after {}s for session {}",
                    self.config.request_timeout_secs,
                    input.session_id
                );
                // Best-effort persist before returning timeout
                self.session_store.persist(&input.session_id).await.ok();
                Ok(Output::text("Request timed out.".to_string()))
            }
        }
    }

    async fn process_inner(&mut self, input: &Input) -> Result<Output> {
        // Consolidation deferred from previous turn — runs before new input
        let needs_consolidation = {
            let session = self.session_store.get_or_load(&input.session_id).await;
            session.needs_consolidation
        };
        if needs_consolidation {
            let session = self.session_store.get_or_load(&input.session_id).await;
            if let Err(e) = self.memory
                .consolidate(session, &*self.llm, self.config.memory_max_bytes)
                .await
            {
                tracing::warn!("Consolidation failed: {e}");
            } else {
                // Persist the consolidated session so changes survive a crash
                self.session_store.persist(&input.session_id).await.ok();
            }
        }

        // Add user message
        let session = self.session_store.get_or_load(&input.session_id).await;
        session.add_message(Role::User, &input.content);

        // ReAct loop
        let mut total_usage = Usage::default();
        for iteration in 0..self.config.max_iterations {
            tracing::debug!("Agent iteration {}/{}", iteration + 1, self.config.max_iterations);

            // Build context
            let tool_schemas = self.tool_registry.schemas();
            let context = {
                let session = self.session_store.get_or_load(&input.session_id).await;
                self.context_builder.build(session, &tool_schemas).await?
            };

            // Call LLM with failover
            let response = self.call_llm(&context).await?;
            total_usage.input_tokens += response.usage.input_tokens;
            total_usage.output_tokens += response.usage.output_tokens;

            match response.stop_reason {
                StopReason::EndTurn | StopReason::MaxTokens => {
                    let text = response.text.unwrap_or_default();

                    {
                        let session = self.session_store.get_or_load(&input.session_id).await;
                        session.add_message(Role::Assistant, &text);
                        // Flag consolidation for next turn if over threshold
                        if session.message_count() > self.config.consolidation_threshold {
                            session.needs_consolidation = true;
                        }
                    }
                    self.session_store.persist(&input.session_id).await?;

                    return Ok(Output::with_usage(text, total_usage));
                }
                StopReason::ToolUse => {
                    // Record assistant message with tool calls
                    let session = self.session_store.get_or_load(&input.session_id).await;
                    session.add_tool_use_message(&response);

                    // Execute tools in parallel
                    let max_calls = self.config.max_tool_calls_per_iteration
                        .min(response.tool_calls.len());
                    let tool_calls = &response.tool_calls[..max_calls];

                    let ctx = ToolContext {
                        data_dir: self.data_dir.clone(),
                        session_id: input.session_id.clone(),
                        config: self.full_config.clone(),
                    };

                    let results: Vec<ToolResult> =
                        futures::future::join_all(tool_calls.iter().map(|tc| {
                            self.tool_registry.execute(&tc.name, tc.arguments.clone(), &ctx)
                        }))
                        .await;

                    // Add tool results to session
                    let session = self.session_store.get_or_load(&input.session_id).await;
                    for (tc, result) in tool_calls.iter().zip(results) {
                        tracing::info!(
                            "Tool {} result: {}",
                            tc.name,
                            if result.is_error() { "error" } else { "success" }
                        );
                        session.add_tool_result(&tc.id, result);
                    }
                    // Continue loop — LLM will see tool results
                }
            }
        }

        // Max iterations exceeded
        let session = self.session_store.get_or_load(&input.session_id).await;
        let msg = "I've reached my reasoning limit for this turn.".to_string();
        session.add_message(Role::Assistant, &msg);
        self.session_store.persist(&input.session_id).await?;
        Ok(Output::with_usage(msg, total_usage))
    }

    async fn call_llm(&self, context: &Context) -> Result<ChatResponse> {
        match self.llm.chat(context).await {
            Ok(response) => Ok(response),
            Err(primary_err) => {
                tracing::warn!("Primary LLM failed: {primary_err}");
                if let Some(fallback) = &self.fallback_llm {
                    tracing::info!("Trying fallback LLM provider...");
                    fallback.chat(context).await.map_err(|fallback_err| {
                        anyhow!(
                            "All LLM providers failed.\n  Primary: {primary_err}\n  Fallback: {fallback_err}"
                        )
                    })
                } else {
                    Err(primary_err)
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_session_id_valid() {
        assert!(validate_session_id("cli").is_ok());
        assert!(validate_session_id("session-123").is_ok());
        assert!(validate_session_id("test_session.v2").is_ok());
        assert!(validate_session_id("abc123").is_ok());
        assert!(validate_session_id("a").is_ok());
    }

    #[test]
    fn test_validate_session_id_empty() {
        assert!(validate_session_id("").is_err());
    }

    #[test]
    fn test_validate_session_id_path_traversal() {
        assert!(validate_session_id("..").is_err());
        assert!(validate_session_id("../etc/passwd").is_err());
        assert!(validate_session_id("foo/../bar").is_err());
        assert!(validate_session_id("a..b").is_err());
    }

    #[test]
    fn test_validate_session_id_too_long() {
        let long_id = "a".repeat(129);
        assert!(validate_session_id(&long_id).is_err());
        // Exactly 128 should be ok
        let max_id = "a".repeat(128);
        assert!(validate_session_id(&max_id).is_ok());
    }

    #[test]
    fn test_validate_session_id_special_chars() {
        assert!(validate_session_id("foo/bar").is_err());
        assert!(validate_session_id("foo\\bar").is_err());
        assert!(validate_session_id("foo bar").is_err());
        assert!(validate_session_id("foo\nbar").is_err());
        assert!(validate_session_id("foo\0bar").is_err());
    }
}
