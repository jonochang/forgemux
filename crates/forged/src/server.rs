use crate::{CommandRunner, SessionService, WorktreeSpec};
use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Path, State,
    },
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use forgemux_core::AgentType;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug, Deserialize, Serialize)]
pub struct StartRequest {
    pub agent: String,
    pub model: String,
    pub repo: String,
    pub worktree: bool,
    pub branch: Option<String>,
    pub worktree_path: Option<String>,
    pub notify: Option<Vec<String>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StartResponse {
    pub session_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
}

pub fn build_router<R: CommandRunner + 'static>(
    service: Arc<SessionService<R>>,
) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/sessions", get(list_sessions::<R>))
        .route("/sessions/start", post(start_session::<R>))
        .route("/sessions/:id/stop", post(stop_session::<R>))
        .route("/sessions/:id/logs", get(session_logs::<R>))
        .route("/sessions/:id/attach", get(ws_attach::<R>))
        .with_state(service)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "healthy" }))
}

async fn list_sessions<R: CommandRunner + 'static>(
    State(service): State<Arc<SessionService<R>>>,
) -> Json<Vec<forgemux_core::SessionRecord>> {
    let sessions = service.refresh_states().unwrap_or_default();
    Json(sessions)
}

async fn start_session<R: CommandRunner + 'static>(
    State(service): State<Arc<SessionService<R>>>,
    Json(req): Json<StartRequest>,
) -> Result<Json<StartResponse>, (StatusCode, Json<ErrorResponse>)> {
    let agent = match req.agent.as_str() {
        "claude" => AgentType::Claude,
        "codex" => AgentType::Codex,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "unknown agent".to_string(),
                }),
            ))
        }
    };

    let worktree_spec = if req.worktree {
        let Some(branch) = req.branch.clone() else {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "branch is required with worktree".to_string(),
                }),
            ));
        };
        Some(WorktreeSpec {
            branch,
            path: req.worktree_path.map(std::path::PathBuf::from),
        })
    } else {
        None
    };

    let notify = match req.notify {
        Some(list) => {
            let mut kinds = Vec::new();
            for entry in list {
                let kind = match entry.as_str() {
                    "desktop" => crate::NotificationKind::Desktop,
                    "webhook" => crate::NotificationKind::Webhook,
                    "command" => crate::NotificationKind::Command,
                    _ => {
                        return Err((
                            StatusCode::BAD_REQUEST,
                            Json(ErrorResponse {
                                error: format!("unknown notify kind: {entry}"),
                            }),
                        ))
                    }
                };
                kinds.push(kind);
            }
            Some(kinds)
        }
        None => None,
    };

    match service.start_session_with_worktree(agent, req.model, req.repo, worktree_spec, notify) {
        Ok(record) => Ok(Json(StartResponse {
            session_id: record.id.as_str().to_string(),
        })),
        Err(err) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: err.to_string(),
            }),
        )),
    }
}

async fn stop_session<R: CommandRunner + 'static>(
    State(service): State<Arc<SessionService<R>>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    match service.stop_session(&id) {
        Ok(()) => Ok(Json(serde_json::json!({ "status": "stopped" }))),
        Err(err) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: err.to_string(),
            }),
        )),
    }
}

async fn session_logs<R: CommandRunner + 'static>(
    State(service): State<Arc<SessionService<R>>>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    match service.logs(&id, 200) {
        Ok(content) => Ok(Json(serde_json::json!({ "content": content }))),
        Err(err) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: err.to_string(),
            }),
        )),
    }
}

async fn ws_attach<R: CommandRunner + 'static>(
    State(service): State<Arc<SessionService<R>>>,
    Path(id): Path<String>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_ws(service, id, socket))
}

async fn handle_ws<R: CommandRunner + 'static>(
    service: Arc<SessionService<R>>,
    id: String,
    mut socket: WebSocket,
) {
    let session_id = forgemux_core::SessionId::from(id.as_str());
    let mut last_snapshot = String::new();

    loop {
        if let Ok(snapshot) = service.capture_output(&session_id, 200) {
            if snapshot != last_snapshot {
                if socket.send(Message::Text(snapshot.clone())).await.is_err() {
                    break;
                }
                last_snapshot = snapshot;
            }
        }

        if let Ok(Some(Ok(msg))) = tokio::time::timeout(
            std::time::Duration::from_millis(200),
            socket.recv(),
        )
        .await
        {
            match msg {
                Message::Text(text) => {
                    let _ = service.send_keys(&session_id, &text);
                }
                Message::Binary(bytes) => {
                    if let Ok(text) = String::from_utf8(bytes) {
                        let _ = service.send_keys(&session_id, &text);
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FakeRunner, ForgedConfig, SessionService};
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    #[tokio::test]
    async fn health_endpoint_works() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let service = Arc::new(SessionService::new(config, FakeRunner::default()));
        let app = build_router(service);

        let response = app
            .oneshot(Request::builder().uri("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn start_requires_branch_with_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let service = Arc::new(SessionService::new(config, FakeRunner::default()));
        let app = build_router(service);

        let body = serde_json::json!({
            "agent": "claude",
            "model": "sonnet",
            "repo": ".",
            "worktree": true
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/sessions/start")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn start_rejects_unknown_notify_kind() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let service = Arc::new(SessionService::new(config, FakeRunner::default()));
        let app = build_router(service);

        let body = serde_json::json!({
            "agent": "claude",
            "model": "sonnet",
            "repo": ".",
            "worktree": false,
            "notify": ["bogus"]
        });

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/sessions/start")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}
