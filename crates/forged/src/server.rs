use crate::{
    CommandRunner, SessionService, WorktreeSpec,
    stream::{STREAM_PROTOCOL_VERSION, StreamMessage},
};
use axum::{
    Json, Router,
    extract::{
        Path, Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::HeaderMap,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
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
    pub policy: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StartResponse {
    pub session_id: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorResponse {
    pub error: String,
}

pub fn build_router<R: CommandRunner + 'static>(service: Arc<SessionService<R>>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics::<R>))
        .route("/sessions", get(list_sessions::<R>))
        .route("/sessions/start", post(start_session::<R>))
        .route("/sessions/:id/stop", post(stop_session::<R>))
        .route("/sessions/:id/logs", get(session_logs::<R>))
        .route("/sessions/:id/input", post(session_input::<R>))
        .route("/sessions/:id/usage", get(session_usage::<R>))
        .route("/foreman/report", get(foreman_report::<R>))
        .route("/sessions/:id/attach", get(ws_attach::<R>))
        .with_state(service)
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "healthy" }))
}

async fn metrics<R: CommandRunner + 'static>(
    State(service): State<Arc<SessionService<R>>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if !authorized(&service, &headers, None) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let sessions = service.list_sessions().unwrap_or_default();
    let total = sessions.len();
    let running = sessions
        .iter()
        .filter(|s| matches!(s.state, forgemux_core::SessionState::Running))
        .count();
    let body = format!(
        "forgemux_sessions_total {}\nforgemux_sessions_running {}\n",
        total, running
    );
    (StatusCode::OK, body).into_response()
}

async fn list_sessions<R: CommandRunner + 'static>(
    State(service): State<Arc<SessionService<R>>>,
    headers: HeaderMap,
) -> Json<Vec<forgemux_core::SessionRecord>> {
    if !authorized(&service, &headers, None) {
        return Json(Vec::new());
    }
    let sessions = service.refresh_states().unwrap_or_default();
    Json(sessions)
}

async fn start_session<R: CommandRunner + 'static>(
    State(service): State<Arc<SessionService<R>>>,
    headers: HeaderMap,
    Json(req): Json<StartRequest>,
) -> Result<Json<StartResponse>, (StatusCode, Json<ErrorResponse>)> {
    if !authorized(&service, &headers, None) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "unauthorized".to_string(),
            }),
        ));
    }
    let agent = match req.agent.as_str() {
        "claude" => AgentType::Claude,
        "codex" => AgentType::Codex,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "unknown agent".to_string(),
                }),
            ));
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
                        ));
                    }
                };
                kinds.push(kind);
            }
            Some(kinds)
        }
        None => None,
    };

    match service.start_session_with_worktree(
        agent,
        req.model,
        req.repo,
        worktree_spec,
        notify,
        req.policy,
    ) {
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
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    if !authorized(&service, &headers, None) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "unauthorized".to_string(),
            }),
        ));
    }
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
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    if !authorized(&service, &headers, None) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "unauthorized".to_string(),
            }),
        ));
    }
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

#[derive(Debug, Deserialize)]
struct InputRequest {
    input: String,
}

async fn session_input<R: CommandRunner + 'static>(
    State(service): State<Arc<SessionService<R>>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<InputRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    if !authorized(&service, &headers, None) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "unauthorized".to_string(),
            }),
        ));
    }
    let session_id = forgemux_core::SessionId::from(id.as_str());
    match service.send_keys(&session_id, &req.input) {
        Ok(()) => Ok(Json(serde_json::json!({ "status": "sent" }))),
        Err(err) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: err.to_string(),
            }),
        )),
    }
}

async fn session_usage<R: CommandRunner + 'static>(
    State(service): State<Arc<SessionService<R>>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<crate::UsageRecord>, (StatusCode, Json<ErrorResponse>)> {
    if !authorized(&service, &headers, None) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "unauthorized".to_string(),
            }),
        ));
    }
    match service.usage(&id) {
        Ok(record) => Ok(Json(record)),
        Err(err) => Err((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: err.to_string(),
            }),
        )),
    }
}

async fn foreman_report<R: CommandRunner + 'static>(
    State(service): State<Arc<SessionService<R>>>,
    headers: HeaderMap,
) -> Result<Json<crate::ForemanReport>, (StatusCode, Json<ErrorResponse>)> {
    if !authorized(&service, &headers, None) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "unauthorized".to_string(),
            }),
        ));
    }
    match service.foreman_report() {
        Ok(report) => Ok(Json(report)),
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
    Query(query): Query<AuthQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if !authorized(&service, &HeaderMap::new(), query.token.as_deref()) {
        return (StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    ws.on_upgrade(move |socket| handle_ws(service, id, socket))
}

#[derive(Debug, Deserialize)]
struct AuthQuery {
    token: Option<String>,
}

fn authorized<R: CommandRunner>(
    service: &SessionService<R>,
    headers: &HeaderMap,
    token: Option<&str>,
) -> bool {
    if service.config().api_tokens.is_empty() {
        return true;
    }
    if let Some(token) = token {
        return service.config().api_tokens.iter().any(|t| t == token);
    }
    if let Some(value) = headers.get(axum::http::header::AUTHORIZATION)
        && let Ok(value) = value.to_str()
        && let Some(token) = value.strip_prefix("Bearer ")
    {
        return service.config().api_tokens.iter().any(|t| t == token);
    }
    false
}

async fn handle_ws<R: CommandRunner + 'static>(
    service: Arc<SessionService<R>>,
    id: String,
    mut socket: WebSocket,
) {
    let session_id = forgemux_core::SessionId::from(id.as_str());
    let manager = service.stream_manager();
    let mut last_snapshot = String::new();
    let mut last_seen = 0u64;
    let mut control_mode = true;

    if let Ok(Some(Ok(msg))) =
        tokio::time::timeout(std::time::Duration::from_secs(1), socket.recv()).await
        && let Message::Text(text) = msg
        && let Ok(StreamMessage::Resume {
            last_seen_event_id,
            mode,
            protocol_version,
        }) = serde_json::from_str::<StreamMessage>(&text)
    {
        if let Some(version) = protocol_version
            && !protocol_supported(version)
        {
            let _ = socket.send(Message::Close(None)).await;
            return;
        }
        last_seen = last_seen_event_id.unwrap_or(0);
        if let Some(mode) = mode {
            control_mode = mode != "watch";
        }
    }

    if let Ok(snapshot) = service.capture_output(&session_id, service.config().snapshot_lines) {
        let snapshot_id = manager.latest_event_id(&session_id);
        let payload = StreamMessage::Snapshot {
            snapshot_id,
            data: snapshot.clone(),
        };
        let _ = socket
            .send(Message::Text(serde_json::to_string(&payload).unwrap()))
            .await;
        last_snapshot = snapshot;
    }

    for event in manager.events_since(&session_id, last_seen) {
        let payload = StreamMessage::Event {
            event_id: event.id,
            data: event.data,
            durable: true,
        };
        if socket
            .send(Message::Text(serde_json::to_string(&payload).unwrap()))
            .await
            .is_err()
        {
            return;
        }
    }

    let mut snapshot_tick = tokio::time::interval(std::time::Duration::from_millis(
        service.config().snapshot_interval_ms,
    ));

    loop {
        tokio::select! {
            msg = socket.recv() => {
                match msg {
                    Some(Ok(Message::Text(text))) => {
                        if let Ok(StreamMessage::Input { input_id, data }) =
                            serde_json::from_str::<StreamMessage>(&text)
                        {
                            if control_mode && manager.accept_input(&session_id, &input_id) {
                                let _ = service.send_keys(&session_id, &data);
                            }
                            let ack = StreamMessage::Ack { input_id };
                            let _ = socket.send(Message::Text(serde_json::to_string(&ack).unwrap())).await;
                        }
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        if let Ok(text) = String::from_utf8(bytes)
                            && let Ok(StreamMessage::Input { input_id, data }) =
                                serde_json::from_str::<StreamMessage>(&text)
                        {
                            if control_mode && manager.accept_input(&session_id, &input_id) {
                                let _ = service.send_keys(&session_id, &data);
                            }
                            let ack = StreamMessage::Ack { input_id };
                            let _ = socket.send(Message::Text(serde_json::to_string(&ack).unwrap())).await;
                        }
                    }
                    Some(Ok(Message::Close(_))) | None => break,
                    _ => {}
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_millis(service.config().poll_interval_ms)) => {
                if let Ok(snapshot) = service.capture_output(&session_id, service.config().snapshot_lines)
                    && snapshot != last_snapshot
                {
                    let event = manager.push_event(&session_id, snapshot.clone());
                    let payload = StreamMessage::Event {
                        event_id: event.id,
                        data: event.data,
                        durable: true,
                    };
                    if socket.send(Message::Text(serde_json::to_string(&payload).unwrap())).await.is_err() {
                        break;
                    }
                    last_snapshot = snapshot;
                }
            }
            _ = snapshot_tick.tick() => {
                if let Ok(snapshot) = service.capture_output(&session_id, service.config().snapshot_lines) {
                    let snapshot_id = manager.latest_event_id(&session_id);
                    let payload = StreamMessage::Snapshot { snapshot_id, data: snapshot };
                    if socket.send(Message::Text(serde_json::to_string(&payload).unwrap())).await.is_err() {
                        break;
                    }
                }
            }
        }
    }
}

fn protocol_supported(version: u32) -> bool {
    version == STREAM_PROTOCOL_VERSION
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
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
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

    #[tokio::test]
    async fn foreman_report_endpoint() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let service = Arc::new(SessionService::new(config, FakeRunner::default()));
        let app = build_router(service);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/foreman/report")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn session_input_endpoint() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let service = Arc::new(SessionService::new(config, FakeRunner::default()));
        let app = build_router(service);

        let body = serde_json::json!({ "input": "echo hi\n" });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/sessions/S-TEST/input")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn routes_basic_health_and_sessions() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let service = Arc::new(SessionService::new(config, FakeRunner::default()));
        let app = build_router(service);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/sessions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn session_usage_endpoint() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let service = Arc::new(SessionService::new(config, FakeRunner::default()));
        let app = build_router(service);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/sessions/S-TEST/usage")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn metrics_endpoint() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let service = Arc::new(SessionService::new(config, FakeRunner::default()));
        let app = build_router(service);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
}
