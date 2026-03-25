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
        };

        Self {
            llm,
            fallback_llm,
            tool_registry,
            memory: MemoryManager::new(data_dir.clone()),
            session_store: SessionStore::new(data_dir.clone()),
            context_builder: ContextBuilder::new(
                data_dir.clone(),
                config.agent.context_cache_ttl_secs,
            ),
            config: agent_config,
            full_config: Arc::new(config.clone()),
            data_dir,
        }
    }

    /// Process one input. Called only by agent_worker task (sole owner).
    pub async fn process(&mut self, input: &Input) -> Result<Output> {
        // Consolidation deferred from previous turn — runs before new input
        let needs_consolidation = {
            let session = self.session_store.get_or_load(&input.session_id);
            session.needs_consolidation
        };
        if needs_consolidation {
            let session = self.session_store.get_or_load(&input.session_id);
            self.memory
                .consolidate(session, &*self.llm, self.config.memory_max_bytes)
                .await
                .ok(); // Non-fatal — logged inside
        }

        // Add user message
        let session = self.session_store.get_or_load(&input.session_id);
        session.add_message(Role::User, &input.content);

        // ReAct loop
        let mut total_usage = Usage::default();
        for iteration in 0..self.config.max_iterations {
            tracing::debug!("Agent iteration {}/{}", iteration + 1, self.config.max_iterations);

            // Build context
            let tool_schemas = self.tool_registry.schemas();
            let context = self.context_builder.build(
                self.session_store.get_or_load(&input.session_id),
                &tool_schemas,
            )?;

            // Call LLM with failover
            let response = self.call_llm(&context).await?;
            total_usage.input_tokens += response.usage.input_tokens;
            total_usage.output_tokens += response.usage.output_tokens;

            match response.stop_reason {
                StopReason::EndTurn | StopReason::MaxTokens => {
                    let text = response.text.unwrap_or_default();

                    {
                        let session = self.session_store.get_or_load(&input.session_id);
                        session.add_message(Role::Assistant, &text);
                        // Flag consolidation for next turn if over threshold
                        if session.message_count() > self.config.consolidation_threshold {
                            session.needs_consolidation = true;
                        }
                    }
                    self.session_store.persist(&input.session_id)?;

                    return Ok(Output::with_usage(text, total_usage));
                }
                StopReason::ToolUse => {
                    // Record assistant message with tool calls
                    let session = self.session_store.get_or_load(&input.session_id);
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
                    let session = self.session_store.get_or_load(&input.session_id);
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
        let session = self.session_store.get_or_load(&input.session_id);
        let msg = "I've reached my reasoning limit for this turn.".to_string();
        session.add_message(Role::Assistant, &msg);
        self.session_store.persist(&input.session_id)?;
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
