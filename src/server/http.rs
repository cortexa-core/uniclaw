use axum::{
    extract::State,
    http::StatusCode,
    middleware,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};

use crate::agent::{Input, Output};

/// Shared state for HTTP handlers
#[allow(dead_code)] // data_dir used in future phases
pub struct HttpState {
    pub inbound_tx: mpsc::Sender<(Input, oneshot::Sender<Output>)>,
    pub version: String,
    pub model: String,
    pub start_time: std::time::Instant,
    pub config_path: std::path::PathBuf,
    pub data_dir: std::path::PathBuf,
    pub api_token: String,
}

pub fn router(state: Arc<HttpState>) -> Router {
    let api_token = state.api_token.clone();

    Router::new()
        .route("/api/chat", post(chat_handler))
        .route("/api/status", get(status_handler))
        .route("/api/config", get(super::api_config::get_config).post(super::api_config::post_config))
        .route("/api/skills", get(super::api_skills::get_skills))
        .route("/api/chat/stream", post(super::api_stream::stream_chat))
        .layer(axum::extract::DefaultBodyLimit::max(1024 * 1024)) // 1 MB
        .layer(middleware::from_fn(move |req: axum::extract::Request, next: middleware::Next| {
            let token = api_token.clone();
            async move {
                // Skip auth for non-API routes (static files, SPA fallback)
                if !req.uri().path().starts_with("/api/") || token.is_empty() {
                    return Ok(next.run(req).await);
                }
                // Allow GET /api/status without auth (health check)
                if req.uri().path() == "/api/status" && req.method() == axum::http::Method::GET {
                    return Ok(next.run(req).await);
                }
                let auth = req.headers()
                    .get("authorization")
                    .and_then(|v| v.to_str().ok());
                match auth {
                    Some(h) if h.strip_prefix("Bearer ").map_or(false, |t| t == token) => {
                        Ok(next.run(req).await)
                    }
                    _ => Err(StatusCode::UNAUTHORIZED),
                }
            }
        }))
        .fallback(super::static_files::static_handler)
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
