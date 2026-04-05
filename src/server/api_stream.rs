use axum::{
    extract::State,
    response::sse::{Event, KeepAlive, Sse},
    Json,
};
use serde::Deserialize;
use std::convert::Infallible;
use std::sync::Arc;
use tokio::sync::oneshot;

use super::http::HttpState;
use crate::agent::{Input, Output};

#[derive(Deserialize)]
pub struct StreamChatRequest {
    message: String,
    #[serde(default = "default_session")]
    session_id: String,
}

fn default_session() -> String {
    uuid::Uuid::new_v4().to_string()
}

pub async fn stream_chat(
    State(state): State<Arc<HttpState>>,
    Json(req): Json<StreamChatRequest>,
) -> Sse<impl futures::Stream<Item = Result<Event, Infallible>>> {
    let inbound_tx = state.inbound_tx.clone();
    let session_id = req.session_id.clone();

    let stream = async_stream::stream! {
        let input = Input {
            id: uuid::Uuid::new_v4().to_string(),
            session_id: session_id.clone(),
            content: req.message.clone(),
            stream_tx: None,
        };

        let (reply_tx, reply_rx) = oneshot::channel::<Output>();

        // Send thinking status
        yield Ok::<_, Infallible>(Event::default()
            .event("status")
            .data(r#"{"type":"thinking"}"#));

        // Send to agent worker
        if inbound_tx.send((input, reply_tx)).await.is_err() {
            yield Ok(Event::default()
                .event("error")
                .data(r#"{"error":"Agent worker unavailable"}"#));
            return;
        }

        // Wait for response
        match tokio::time::timeout(
            std::time::Duration::from_secs(120),
            reply_rx,
        ).await {
            Ok(Ok(output)) => {
                // Send usage if available
                if let Some(usage) = &output.usage {
                    yield Ok(Event::default()
                        .event("usage")
                        .data(serde_json::json!({
                            "input_tokens": usage.input_tokens,
                            "output_tokens": usage.output_tokens,
                        }).to_string()));
                }

                // Stream text in chunks for visual streaming effect
                let text = &output.content;
                let chunk_size = 20;
                let mut pos = 0;
                while pos < text.len() {
                    let mut end = (pos + chunk_size).min(text.len());
                    // Safe UTF-8 boundary
                    while end < text.len() && !text.is_char_boundary(end) {
                        end -= 1;
                    }
                    if end <= pos {
                        end = text.len();
                    }
                    let chunk = &text[pos..end];
                    yield Ok(Event::default()
                        .event("text_delta")
                        .data(serde_json::json!({"text": chunk}).to_string()));
                    pos = end;
                    tokio::time::sleep(std::time::Duration::from_millis(15)).await;
                }

                yield Ok(Event::default()
                    .event("done")
                    .data(serde_json::json!({"session_id": session_id}).to_string()));
            }
            Ok(Err(_)) => {
                yield Ok(Event::default()
                    .event("error")
                    .data(r#"{"error":"Agent worker dropped request"}"#));
            }
            Err(_) => {
                yield Ok(Event::default()
                    .event("error")
                    .data(r#"{"error":"Request timed out (120s)"}"#));
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::default())
}
