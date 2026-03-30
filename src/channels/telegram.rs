use anyhow::{anyhow, Result};
use async_trait::async_trait;
use teloxide::prelude::*;
use teloxide::types::{ChatAction, ParseMode};
use tokio::sync::oneshot;

use super::{AgentSender, Channel};
use crate::agent::{Input, Output};
use crate::config::TelegramConfig;

pub struct TelegramChannel {
    bot_token: String,
    allowed_users: Vec<i64>,
    respond_in_groups: String,
}

impl TelegramChannel {
    pub fn new(config: &TelegramConfig) -> Result<Self> {
        let bot_token = if config.bot_token_env.is_empty() {
            return Err(anyhow!("Telegram bot_token_env not configured"));
        } else {
            std::env::var(&config.bot_token_env)
                .map_err(|_| anyhow!("Environment variable {} is not set", config.bot_token_env))?
        };

        if bot_token.is_empty() {
            return Err(anyhow!("Telegram bot token is empty"));
        }

        Ok(Self {
            bot_token,
            allowed_users: config.allowed_users.clone(),
            respond_in_groups: config.respond_in_groups.clone(),
        })
    }
}

#[async_trait]
impl Channel for TelegramChannel {
    fn name(&self) -> &str {
        "telegram"
    }

    async fn run(&self, agent_tx: AgentSender) -> Result<()> {
        let bot = Bot::new(&self.bot_token);

        // Verify bot token
        let me = bot.get_me().await
            .map_err(|e| anyhow!("Failed to connect to Telegram: {e}"))?;
        tracing::info!(
            "Telegram bot connected: @{} ({})",
            me.username.as_deref().unwrap_or("unknown"),
            me.first_name
        );

        let allowed_users = self.allowed_users.clone();
        let respond_in_groups = self.respond_in_groups.clone();
        let bot_username = me.username.clone().unwrap_or_default();

        teloxide::repl(bot, move |bot: Bot, msg: Message| {
            let agent_tx = agent_tx.clone();
            let allowed_users = allowed_users.clone();
            let respond_in_groups = respond_in_groups.clone();
            let bot_username = bot_username.clone();

            async move {
                // Only handle text messages
                let text = match msg.text() {
                    Some(t) => t.to_string(),
                    None => return Ok(()),
                };

                let user_id = msg.from.as_ref().map(|u| u.id.0 as i64).unwrap_or(0);

                // Check authorization
                if !allowed_users.is_empty() && !allowed_users.contains(&user_id) {
                    return Ok(());
                }

                // Check group policy
                let is_group = msg.chat.is_group() || msg.chat.is_supergroup();
                if is_group {
                    let should_respond = match respond_in_groups.as_str() {
                        "always" => true,
                        "never" => return Ok(()),
                        _ => {
                            // "mention" mode
                            let mentioned = !bot_username.is_empty()
                                && text.contains(&format!("@{bot_username}"));
                            let is_reply_to_bot = msg.reply_to_message()
                                .and_then(|r| r.from.as_ref())
                                .map(|u| u.is_bot)
                                .unwrap_or(false);
                            mentioned || is_reply_to_bot
                        }
                    };
                    if !should_respond {
                        return Ok(());
                    }
                }

                // Strip @bot_username from the text if present
                let clean_text = if !bot_username.is_empty() {
                    text.replace(&format!("@{bot_username}"), "").trim().to_string()
                } else {
                    text
                };

                if clean_text.is_empty() {
                    return Ok(());
                }

                // Send typing indicator
                bot.send_chat_action(msg.chat.id, ChatAction::Typing).await.ok();

                // Session per user
                let session_id = format!("telegram:{user_id}");

                let input = Input {
                    id: uuid::Uuid::new_v4().to_string(),
                    session_id,
                    content: clean_text,
                };

                let (reply_tx, reply_rx) = oneshot::channel::<Output>();

                if agent_tx.send((input, reply_tx)).await.is_err() {
                    bot.send_message(msg.chat.id, "Agent is unavailable. Please try again later.")
                        .await.ok();
                    return Ok(());
                }

                let response = match tokio::time::timeout(
                    std::time::Duration::from_secs(120),
                    reply_rx,
                ).await {
                    Ok(Ok(output)) => output.content,
                    Ok(Err(_)) => "Something went wrong. Please try again.".to_string(),
                    Err(_) => "Request timed out. Please try again.".to_string(),
                };

                // Chunk and send (Telegram max 4096 chars)
                for chunk in chunk_message(&response, 4096) {
                    // Try Markdown first, fall back to plain text
                    let result = bot.send_message(msg.chat.id, &chunk)
                        .parse_mode(ParseMode::MarkdownV2)
                        .await;

                    if result.is_err() {
                        bot.send_message(msg.chat.id, &chunk).await.ok();
                    }
                }

                Ok(())
            }
        })
        .await;

        Ok(())
    }
}

fn chunk_message(text: &str, max_len: usize) -> Vec<String> {
    if text.len() <= max_len {
        return vec![text.to_string()];
    }

    let mut chunks = Vec::new();
    let mut remaining = text;

    while !remaining.is_empty() {
        if remaining.len() <= max_len {
            chunks.push(remaining.to_string());
            break;
        }

        let search = &remaining[..max_len];
        let split_at = search.rfind("\n\n")
            .or_else(|| search.rfind('\n'))
            .or_else(|| search.rfind(' '))
            .unwrap_or(max_len);

        let split_at = if split_at == 0 { max_len } else { split_at };

        chunks.push(remaining[..split_at].to_string());
        remaining = remaining[split_at..].trim_start();
    }

    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chunk_short_message() {
        let chunks = chunk_message("Hello world", 4096);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "Hello world");
    }

    #[test]
    fn test_chunk_long_message() {
        let text = "a".repeat(5000);
        let chunks = chunk_message(&text, 4096);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].len() <= 4096);
    }

    #[test]
    fn test_chunk_at_paragraph() {
        let text = format!("{}\n\n{}", "a".repeat(2000), "b".repeat(3000));
        let chunks = chunk_message(&text, 4096);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].ends_with('a'));
        assert!(chunks[1].starts_with('b'));
    }
}
