pub mod aliases;
pub mod anthropic;
pub mod gemini;
pub mod openai;
pub mod reliable;
pub mod router;
pub mod types;

use anyhow::Result;
use async_trait::async_trait;

use crate::config::LlmConfig;
use types::{ChatResponse, Context};

#[async_trait]
pub trait LlmProvider: Send + Sync {
    async fn chat(&self, context: &Context) -> Result<ChatResponse>;
    fn name(&self) -> &str;
    #[allow(dead_code)]
    fn supports_native_tools(&self) -> bool {
        true
    }
    #[allow(dead_code)]
    fn supports_vision(&self) -> bool {
        false
    }

    /// Streaming variant: sends text chunks through tx as they arrive,
    /// then returns the complete ChatResponse for tool parsing.
    /// Default implementation calls chat() and sends the full text at once.
    async fn chat_streaming(
        &self,
        context: &Context,
        tx: tokio::sync::mpsc::Sender<String>,
    ) -> Result<ChatResponse> {
        let response = self.chat(context).await?;
        if let Some(ref text) = response.text {
            let _ = tx.send(text.clone()).await;
        }
        Ok(response)
    }

    #[allow(dead_code)]
    fn supports_streaming(&self) -> bool {
        false
    }
}

pub fn create_provider(config: &LlmConfig) -> Result<Box<dyn LlmProvider>> {
    let alias = aliases::resolve(&config.provider);
    let backend = alias
        .as_ref()
        .map(|a| a.backend)
        .unwrap_or(config.provider.as_str());

    match backend {
        "anthropic" => Ok(Box::new(anthropic::AnthropicProvider::new(config)?)),
        "gemini" => Ok(Box::new(gemini::GeminiProvider::new(config)?)),
        "openai_compatible" | "openai" => Ok(Box::new(openai::OpenAiProvider::new(config)?)),
        _ => {
            // Unknown backend — try as OpenAI-compatible
            tracing::info!(
                "Unknown provider '{}', trying as OpenAI-compatible",
                config.provider
            );
            Ok(Box::new(openai::OpenAiProvider::new(config)?))
        }
    }
}
