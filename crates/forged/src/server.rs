use crate::{
    CommandRunner, SessionService, WorktreeSpec,
    stream::{STREAM_PROTOCOL_VERSION, StreamMessage},
};
use aes_gcm::aead::{Aead, KeyInit};
use aes_gcm::{Aes256Gcm, Nonce};
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
use base64::Engine;
use forgemux_core::{AgentType, DecisionAction, DecisionContext, ReplayEventType, Severity, scrub};
use rand::RngCore;
use reqwest::Client;
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

#[derive(Debug, Deserialize)]
pub struct DecisionRequest {
    pub question: String,
    pub context: DecisionContext,
    pub severity: Severity,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub impact_repo_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct DecisionResponseRequest {
    pub action: DecisionAction,
    pub reviewer: String,
    pub comment: Option<String>,
}

pub fn build_router<R: CommandRunner + 'static>(service: Arc<SessionService<R>>) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/metrics", get(metrics::<R>))
        .route("/sessions", get(list_sessions::<R>))
        .route("/sessions/start", post(start_session::<R>))
        .route("/sessions/:id/stop", post(stop_session::<R>))
        .route("/sessions/:id/logs", get(session_logs::<R>))
        .route("/sessions/:id/replay.jsonl", get(session_replay::<R>))
        .route("/sessions/:id/input", post(session_input::<R>))
        .route("/sessions/:id/usage", get(session_usage::<R>))
        .route("/sessions/:id/decision", post(create_decision::<R>))
        .route(
            "/sessions/:id/decision-response",
            post(decision_response::<R>),
        )
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
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
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
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, None) {
        return (
            StatusCode::UNAUTHORIZED,
            Json(Vec::<forgemux_core::SessionRecord>::new()),
        )
            .into_response();
    }
    let sessions = service.refresh_states().unwrap_or_default();
    (StatusCode::OK, Json(sessions)).into_response()
}

async fn start_session<R: CommandRunner + 'static>(
    State(service): State<Arc<SessionService<R>>>,
    headers: HeaderMap,
    Json(req): Json<StartRequest>,
) -> Result<Json<StartResponse>, (StatusCode, Json<ErrorResponse>)> {
    if let Some(resp) = check_version(&headers) {
        return Err((
            resp.status(),
            Json(ErrorResponse {
                error: "version mismatch".to_string(),
            }),
        ));
    }
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
    if let Some(resp) = check_version(&headers) {
        return Err((
            resp.status(),
            Json(ErrorResponse {
                error: "version mismatch".to_string(),
            }),
        ));
    }
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
    if let Some(resp) = check_version(&headers) {
        return Err((
            resp.status(),
            Json(ErrorResponse {
                error: "version mismatch".to_string(),
            }),
        ));
    }
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

async fn session_replay<R: CommandRunner + 'static>(
    State(service): State<Arc<SessionService<R>>>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<String, (StatusCode, Json<ErrorResponse>)> {
    if let Some(resp) = check_version(&headers) {
        return Err((
            resp.status(),
            Json(ErrorResponse {
                error: "version mismatch".to_string(),
            }),
        ));
    }
    if !authorized(&service, &headers, None) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "unauthorized".to_string(),
            }),
        ));
    }
    match service.replay_jsonl(&id) {
        Ok(content) => Ok(content),
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
    if let Some(resp) = check_version(&headers) {
        return Err((
            resp.status(),
            Json(ErrorResponse {
                error: "version mismatch".to_string(),
            }),
        ));
    }
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
    if let Some(resp) = check_version(&headers) {
        return Err((
            resp.status(),
            Json(ErrorResponse {
                error: "version mismatch".to_string(),
            }),
        ));
    }
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

#[derive(Debug, Serialize)]
struct HubDecisionPayload {
    session_id: String,
    workspace_id: String,
    repo_id: String,
    question: String,
    context: DecisionContext,
    severity: Severity,
    tags: Vec<String>,
    impact_repo_ids: Vec<String>,
    assigned_to: Option<String>,
    agent_goal: String,
}

async fn create_decision<R: CommandRunner + 'static>(
    State(service): State<Arc<SessionService<R>>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<DecisionRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    if let Some(resp) = check_version(&headers) {
        return Err((
            resp.status(),
            Json(ErrorResponse {
                error: "version mismatch".to_string(),
            }),
        ));
    }
    if !authorized(&service, &headers, None) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "unauthorized".to_string(),
            }),
        ));
    }
    let record = service.session(&id).map_err(|err| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: err.to_string(),
            }),
        )
    })?;
    let hub_url = service.config().hub_url.clone().ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "hub_url not configured".to_string(),
            }),
        )
    })?;

    let payload = HubDecisionPayload {
        session_id: record.id.as_str().to_string(),
        workspace_id: "default".to_string(),
        repo_id: record.repo_root.display().to_string(),
        question: req.question,
        context: scrub_context(req.context),
        severity: req.severity,
        tags: req.tags,
        impact_repo_ids: req.impact_repo_ids,
        assigned_to: None,
        agent_goal: record.goal.unwrap_or_else(|| "(no goal set)".to_string()),
    };
    let replay_question = payload.question.clone();
    let replay_repo_id = payload.repo_id.clone();

    let client = Client::new();
    let url = format!("{}/decisions", hub_url.trim_end_matches('/'));
    let mut request = client.post(&url).json(&payload);
    if let Some(token) = &service.config().hub_token {
        request = request.bearer_auth(token);
    }
    let response = request.send().await.map_err(|err| {
        (
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse {
                error: err.to_string(),
            }),
        )
    })?;
    if !response.status().is_success() {
        return Err((
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse {
                error: format!("hub returned {}", response.status()),
            }),
        ));
    }
    let value = response.json::<serde_json::Value>().await.map_err(|err| {
        (
            StatusCode::BAD_GATEWAY,
            Json(ErrorResponse {
                error: err.to_string(),
            }),
        )
    })?;
    service.record_replay_event(
        &record.id,
        ReplayEventType::Decision,
        format!("Decision requested: {replay_question}"),
        None,
        Some(replay_repo_id),
    );
    Ok(Json(value))
}

async fn decision_response<R: CommandRunner + 'static>(
    State(service): State<Arc<SessionService<R>>>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(req): Json<DecisionResponseRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ErrorResponse>)> {
    if let Some(resp) = check_version(&headers) {
        return Err((
            resp.status(),
            Json(ErrorResponse {
                error: "version mismatch".to_string(),
            }),
        ));
    }
    if !authorized(&service, &headers, None) {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(ErrorResponse {
                error: "unauthorized".to_string(),
            }),
        ));
    }
    let session_id = forgemux_core::SessionId::from(id.as_str());
    service.session(&id).map_err(|err| {
        (
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: err.to_string(),
            }),
        )
    })?;

    let action = match req.action {
        DecisionAction::Approve => "Approved",
        DecisionAction::Deny => "Denied",
        DecisionAction::Comment => "Comment",
    };
    let comment = req.comment.unwrap_or_default();
    let message = if comment.is_empty() {
        format!("{action} by {}\n", req.reviewer)
    } else {
        format!("{action} by {}: {comment}\n", req.reviewer)
    };
    service.send_keys(&session_id, &message).map_err(|err| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(ErrorResponse {
                error: err.to_string(),
            }),
        )
    })?;
    service.record_replay_event(
        &session_id,
        ReplayEventType::Decision,
        format!("{action} by {}", req.reviewer),
        None,
        None,
    );
    Ok(Json(serde_json::json!({
        "status": "ok",
        "action": format!("{:?}", req.action).to_lowercase(),
    })))
}

fn scrub_context(context: DecisionContext) -> DecisionContext {
    match context {
        DecisionContext::Diff { file, lines } => DecisionContext::Diff {
            file: scrub(&file),
            lines: lines
                .into_iter()
                .map(|mut line| {
                    line.text = scrub(&line.text);
                    line
                })
                .collect(),
        },
        DecisionContext::Log { text } => DecisionContext::Log { text: scrub(&text) },
        DecisionContext::Screenshot { description } => DecisionContext::Screenshot {
            description: scrub(&description),
        },
    }
}

async fn foreman_report<R: CommandRunner + 'static>(
    State(service): State<Arc<SessionService<R>>>,
    headers: HeaderMap,
) -> Result<Json<crate::ForemanReport>, (StatusCode, Json<ErrorResponse>)> {
    if let Some(resp) = check_version(&headers) {
        return Err((
            resp.status(),
            Json(ErrorResponse {
                error: "version mismatch".to_string(),
            }),
        ));
    }
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
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<AuthQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
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

fn check_version(headers: &HeaderMap) -> Option<axum::response::Response> {
    let version = headers
        .get("x-forgemux-version")
        .and_then(|value| value.to_str().ok())?;
    if version_compatible(version) {
        None
    } else {
        Some(
            (
                StatusCode::UPGRADE_REQUIRED,
                format!(
                    "forged {} requires fmux >= {}.x",
                    env!("CARGO_PKG_VERSION"),
                    env!("CARGO_PKG_VERSION").split('.').next().unwrap_or("0")
                ),
            )
                .into_response(),
        )
    }
}

fn version_compatible(client: &str) -> bool {
    let Some(client_major) = client
        .split('.')
        .next()
        .and_then(|part| part.parse::<u64>().ok())
    else {
        return true;
    };
    let server_major = env!("CARGO_PKG_VERSION")
        .split('.')
        .next()
        .and_then(|part| part.parse::<u64>().ok())
        .unwrap_or(client_major);
    client_major == server_major
}

#[derive(Clone)]
struct StreamCrypto {
    cipher: Aes256Gcm,
}

impl StreamCrypto {
    fn from_key(key_b64: Option<&str>) -> Option<Self> {
        let key_b64 = key_b64?;
        let raw = base64::engine::general_purpose::STANDARD
            .decode(key_b64)
            .ok()?;
        if raw.len() != 32 {
            return None;
        }
        let cipher = Aes256Gcm::new_from_slice(&raw).ok()?;
        Some(Self { cipher })
    }

    fn encrypt(&self, plaintext: &str) -> Option<String> {
        let mut nonce_bytes = [0u8; 12];
        rand::rngs::OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = self.cipher.encrypt(nonce, plaintext.as_bytes()).ok()?;
        let mut out = Vec::with_capacity(nonce_bytes.len() + ciphertext.len());
        out.extend_from_slice(&nonce_bytes);
        out.extend_from_slice(&ciphertext);
        Some(base64::engine::general_purpose::STANDARD.encode(out))
    }

    fn decrypt(&self, data: &str) -> Option<String> {
        let raw = base64::engine::general_purpose::STANDARD
            .decode(data)
            .ok()?;
        if raw.len() < 12 {
            return None;
        }
        let (nonce_bytes, ciphertext) = raw.split_at(12);
        let nonce = Nonce::from_slice(nonce_bytes);
        let plaintext = self.cipher.decrypt(nonce, ciphertext).ok()?;
        String::from_utf8(plaintext).ok()
    }
}

async fn handle_ws<R: CommandRunner + 'static>(
    service: Arc<SessionService<R>>,
    id: String,
    mut socket: WebSocket,
) {
    let session_id = forgemux_core::SessionId::from(id.as_str());
    let manager = service.stream_manager();
    let crypto = StreamCrypto::from_key(service.config().stream_encryption_key.as_deref());
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
        let (data, encrypted) = match &crypto {
            Some(crypto) => crypto
                .encrypt(&snapshot)
                .map(|data| (data, true))
                .unwrap_or_else(|| (snapshot.clone(), false)),
            None => (snapshot.clone(), false),
        };
        let payload = StreamMessage::Snapshot {
            snapshot_id,
            data,
            encrypted,
        };
        let _ = socket
            .send(Message::Text(serde_json::to_string(&payload).unwrap()))
            .await;
        last_snapshot = snapshot;
    }

    for event in manager.events_since(&session_id, last_seen) {
        let (data, encrypted) = match &crypto {
            Some(crypto) => crypto
                .encrypt(&event.data)
                .map(|data| (data, true))
                .unwrap_or_else(|| (event.data.clone(), false)),
            None => (event.data.clone(), false),
        };
        let payload = StreamMessage::Event {
            event_id: event.id,
            data,
            durable: event.durable,
            encrypted,
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
                        if let Ok(StreamMessage::Input {
                            input_id,
                            data,
                            encrypted,
                        }) = serde_json::from_str::<StreamMessage>(&text)
                        {
                            let payload = if encrypted {
                                crypto.as_ref().and_then(|crypto| crypto.decrypt(&data))
                            } else {
                                Some(data)
                            };
                            if let Some(payload) = payload
                                && control_mode
                                && manager.accept_input(&session_id, &input_id)
                            {
                                let _ = service.send_keys(&session_id, &payload);
                            }
                            let ack = StreamMessage::Ack { input_id };
                            let _ = socket.send(Message::Text(serde_json::to_string(&ack).unwrap())).await;
                        }
                    }
                    Some(Ok(Message::Binary(bytes))) => {
                        if let Ok(text) = String::from_utf8(bytes)
                            && let Ok(StreamMessage::Input {
                                input_id,
                                data,
                                encrypted,
                            }) = serde_json::from_str::<StreamMessage>(&text)
                        {
                            let payload = if encrypted {
                                crypto.as_ref().and_then(|crypto| crypto.decrypt(&data))
                            } else {
                                Some(data)
                            };
                            if let Some(payload) = payload
                                && control_mode
                                && manager.accept_input(&session_id, &input_id)
                            {
                                let _ = service.send_keys(&session_id, &payload);
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
                    let event = manager.push_event(&session_id, snapshot.clone(), true);
                    let (data, encrypted) = match &crypto {
                        Some(crypto) => crypto
                            .encrypt(&event.data)
                            .map(|data| (data, true))
                            .unwrap_or_else(|| (event.data.clone(), false)),
                        None => (event.data.clone(), false),
                    };
                    let payload = StreamMessage::Event {
                        event_id: event.id,
                        data,
                        durable: event.durable,
                        encrypted,
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
                    let (data, encrypted) = match &crypto {
                        Some(crypto) => crypto
                            .encrypt(&snapshot)
                            .map(|data| (data, true))
                            .unwrap_or_else(|| (snapshot.clone(), false)),
                        None => (snapshot.clone(), false),
                    };
                    let payload = StreamMessage::Snapshot {
                        snapshot_id,
                        data,
                        encrypted,
                    };
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
    async fn decision_endpoint_scrubs_context() {
        let captured = Arc::new(std::sync::Mutex::new(None::<serde_json::Value>));
        let captured_clone = captured.clone();
        let hub_app = Router::new().route(
            "/decisions",
            post(move |Json(payload): Json<serde_json::Value>| {
                let captured = captured_clone.clone();
                async move {
                    *captured.lock().unwrap() = Some(payload.clone());
                    Json(payload)
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, hub_app).await.unwrap();
        });

        let tmp = tempfile::tempdir().unwrap();
        let mut config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        config.hub_url = Some(format!("http://{}", addr));
        let runner = FakeRunner::default();
        let service = Arc::new(SessionService::new(config, runner));
        let app = build_router(service.clone());

        let session = service
            .start_session(AgentType::Claude, "sonnet", tmp.path())
            .unwrap();

        let body = serde_json::json!({
            "question": "Ship it?",
            "context": { "type": "log", "text": "token=abcdEFGH1234" },
            "severity": "high",
            "tags": ["release"],
            "impact_repo_ids": ["repo-2"]
        });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/sessions/{}/decision", session.id.as_str()))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let captured = captured.lock().unwrap().clone().unwrap();
        let text = captured
            .get("context")
            .and_then(|ctx| ctx.get("text"))
            .and_then(|value| value.as_str())
            .unwrap();
        assert!(text.contains("[REDACTED_TOKEN]"));
    }

    #[tokio::test]
    async fn decision_response_injects_tmux_input() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let runner = FakeRunner::default();
        let service = Arc::new(SessionService::new(config, runner.clone()));
        let app = build_router(service.clone());

        let session = service
            .start_session(AgentType::Claude, "sonnet", tmp.path())
            .unwrap();

        let body = serde_json::json!({
            "action": "approve",
            "reviewer": "jono",
            "comment": "ok"
        });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!(
                        "/sessions/{}/decision-response",
                        session.id.as_str()
                    ))
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let calls = runner.calls();
        assert!(calls.iter().any(|call| {
            call.contains(&"send-keys".to_string())
                && call.contains(&"Approved by jono: ok".to_string())
        }));
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
    async fn routes_reject_incompatible_version() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ForgedConfig::default_with_data_dir(tmp.path().to_path_buf());
        let service = Arc::new(SessionService::new(config, FakeRunner::default()));
        let app = build_router(service);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/sessions")
                    .header("x-forgemux-version", "2.0.0")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UPGRADE_REQUIRED);
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
