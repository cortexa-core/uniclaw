use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

use crate::agent::{Input, Output};

/// Shared state for HTTP handlers
pub struct HttpState {
    pub inbound_tx: mpsc::Sender<(Input, oneshot::Sender<Output>)>,
    pub version: String,
    pub model: String,
    pub start_time: std::time::Instant,
}

pub fn router(state: Arc<HttpState>) -> Router {
    Router::new()
        .route("/api/chat", post(chat_handler))
        .route("/api/status", get(status_handler))
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024)) // 1 MB
        .with_state(state)
}

#[derive(Deserialize)]
struct ChatRequest {
    message: String,
    #[serde(default = "default_session")]
    session_id: String,
}

fn default_session() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[derive(Serialize)]
struct StatusResponse {
    version: String,
    model: String,
    uptime_secs: u64,
    status: String,
}

// --- Handlers ---

async fn chat_handler(
    State(state): State<Arc<HttpState>>,
    Json(req): Json<ChatRequest>,
) -> impl IntoResponse {
    let input = Input {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: req.session_id.clone(),
        content: req.message,
    };

    let (reply_tx, reply_rx) = oneshot::channel();

    if state.inbound_tx.send((input, reply_tx)).await.is_err() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Agent worker unavailable"})),
        );
    }

    match tokio::time::timeout(std::time::Duration::from_secs(60), reply_rx).await {
        Ok(Ok(output)) => {
            let (input_tokens, output_tokens) = match &output.usage {
                Some(u) => (Some(u.input_tokens), Some(u.output_tokens)),
                None => (None, None),
            };
            (
                StatusCode::OK,
                Json(serde_json::json!({
                    "response": output.content,
                    "session_id": req.session_id,
                    "input_tokens": input_tokens,
                    "output_tokens": output_tokens,
                })),
            )
        }
        Ok(Err(_)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({"error": "Agent worker dropped the request"})),
        ),
        Err(_) => (
            StatusCode::GATEWAY_TIMEOUT,
            Json(serde_json::json!({"error": "Request timed out (60s)"})),
        ),
    }
}

async fn status_handler(State(state): State<Arc<HttpState>>) -> Json<StatusResponse> {
    Json(StatusResponse {
        version: state.version.clone(),
        model: state.model.clone(),
        uptime_secs: state.start_time.elapsed().as_secs(),
        status: "running".into(),
    })
}
