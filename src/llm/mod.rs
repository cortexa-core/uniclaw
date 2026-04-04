pub mod aliases;
pub mod anthropic;
pub mod gemini;
pub mod openai;
pub mod reliable;
pub mod router;
pub mod types;

use anyhow::{anyhow, Result};
use async_trait::async_trait;

use crate::config::LlmConfig;
use types::{ChatResponse, Context};

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat(&self, context: &Context) -> Result<ChatResponse>;
    fn name(&self) -> &str;
    fn supports_native_tools(&self) -> bool {
        true
    }
    fn supports_vision(&self) -> bool {
        false
    }
}

pub fn create_provider(config: &LlmConfig) -> Result<Box<dyn LlmProvider>> {
    match config.provider.as_str() {
        "anthropic" => Ok(Box::new(anthropic::AnthropicProvider::new(config)?)),
        "openai_compatible" | "openai" => Ok(Box::new(openai::OpenAiProvider::new(config)?)),
        "gemini" => Ok(Box::new(gemini::GeminiProvider::new(config)?)),
        other => Err(anyhow!(
            "Unknown LLM provider: {other}. Use 'anthropic', 'gemini', or 'openai_compatible'."
        )),
    }
}
