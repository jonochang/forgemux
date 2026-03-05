use axum::{
    Json, Router,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::Uri,
    http::{HeaderMap, HeaderValue},
    response::IntoResponse,
    routing::{get, post},
};
use chrono::Utc;
use clap::{Parser, Subcommand};
use forgehub::{DecisionEvent, DecisionStatus, HubConfig, HubEdge, HubService};
use forgemux_core::{Decision, DecisionAction, DecisionContext, Severity};
use futures_util::{SinkExt, StreamExt, future::join_all};
use include_dir::{Dir, include_dir};
use mime_guess::MimeGuess;
use serde::Deserialize;
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::broadcast;
use tokio_tungstenite::tungstenite::Message as TungsteniteMessage;
use tracing::{debug, warn};
use uuid::Uuid;

const DASHBOARD_DIR: Dir = include_dir!("$CARGO_MANIFEST_DIR/../../dashboard");
const FORGEHUB_VERSION_HEADER: &str = "x-forgemux-version";

#[derive(Debug, Subcommand)]
enum Command {
    Configure {
        #[arg(long)]
        non_interactive: bool,
        #[arg(long)]
        force: bool,
        #[arg(long)]
        dry_run: bool,
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long)]
        enable_tokens: bool,
        #[arg(long)]
        token: Option<String>,
        #[arg(long)]
        shared_fs: bool,
        #[arg(long)]
        edge_id: Option<String>,
        #[arg(long)]
        edge_data_dir: Option<PathBuf>,
        #[arg(long)]
        edge_ws_url: Option<String>,
        #[arg(long)]
        bind: Option<String>,
    },
    Run,
    Check,
    Sessions,
    Export {
        #[arg(long, default_value = "sessions")]
        kind: String,
    },
    Version,
}

#[derive(Debug, Parser)]
#[command(name = "forgehub")]
#[command(about = "Forgemux hub", long_about = None)]
struct Cli {
    #[arg(long, default_value = "./.forgemux-hub.toml")]
    config: PathBuf,
    #[arg(long, default_value = "127.0.0.1:8080")]
    bind: String,
    #[command(subcommand)]
    command: Command,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let config = if cli.config.exists() {
        HubConfig::load(&cli.config)?
    } else {
        HubConfig {
            data_dir: PathBuf::from("./.forgemux-hub"),
            edges: Vec::new(),
            tokens: Vec::new(),
            organization: None,
            workspaces: Vec::new(),
        }
    };
    let rt = tokio::runtime::Runtime::new()?;
    let service = Arc::new(rt.block_on(HubService::new(config))?);

    match cli.command {
        Command::Configure {
            non_interactive,
            force,
            dry_run,
            ref data_dir,
            enable_tokens,
            ref token,
            shared_fs,
            ref edge_id,
            ref edge_data_dir,
            ref edge_ws_url,
            ref bind,
        } => {
            run_configure(
                &cli,
                non_interactive,
                force,
                dry_run,
                data_dir.clone(),
                enable_tokens,
                token.clone(),
                shared_fs,
                edge_id.clone(),
                edge_data_dir.clone(),
                edge_ws_url.clone(),
                bind.clone(),
            )?;
        }
        Command::Run => {
            let addr: SocketAddr = cli.bind.parse()?;
            let shared = service.clone();
            let app = Router::new()
                .route("/health", get(health))
                .route("/version", get(version))
                .route("/metrics", get(metrics))
                .route("/sessions", get(list_sessions).post(start_session))
                .route("/workspaces", get(list_workspaces))
                .route("/workspaces/:id", get(get_workspace))
                .route("/decisions", get(list_decisions).post(create_decision))
                .route("/decisions/ws", get(ws_decisions))
                .route("/decisions/:id", get(get_decision))
                .route("/decisions/:id/approve", post(decision_approve))
                .route("/decisions/:id/deny", post(decision_deny))
                .route("/decisions/:id/comment", post(decision_comment))
                .route("/sessions/ws", get(ws_sessions))
                .route("/sessions/:id/stop", post(stop_session))
                .route("/sessions/:id/kill", post(kill_session))
                .route("/sessions/:id/logs", get(session_logs))
                .route("/sessions/:id/input", post(session_input))
                .route("/sessions/:id/usage", get(session_usage))
                .route("/sessions/:id/replay/timeline", get(replay_timeline))
                .route("/sessions/:id/replay/diff", get(replay_diff))
                .route("/sessions/:id/replay/terminal", get(replay_terminal))
                .route("/foreman/report", get(foreman_report))
                .route("/pairing/start", post(pairing_start))
                .route("/pairing/exchange", post(pairing_exchange))
                .route("/pair", get(pairing_landing))
                .route("/edges", get(list_edges))
                .route("/edges/:id/config", get(edge_config))
                .route("/edges/register", post(register_edge))
                .route("/edges/heartbeat", post(heartbeat))
                .route("/ws", get(ws_handler))
                .route("/sessions/:id/attach", get(ws_attach))
                .route("/", get(dashboard_index))
                .route("/index.html", get(dashboard_index))
                .route("/legacy", get(dashboard_legacy))
                .fallback(get(dashboard_asset))
                .with_state(shared);
            rt.block_on(async move {
                let poller = service.clone();
                tokio::spawn(async move {
                    poller.poll_edges().await;
                });
                let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
                axum::serve(listener, app).await.unwrap();
            });
        }
        Command::Check => println!("ok"),
        Command::Sessions => {
            let sessions = service.list_sessions()?;
            if sessions.is_empty() {
                println!("no sessions");
            } else {
                for session in sessions {
                    println!("{} {:?} {:?}", session.id, session.agent, session.state);
                }
            }
        }
        Command::Export { kind } => {
            let output = match kind.as_str() {
                "sessions" => {
                    let sessions = rt.block_on(async { fetch_sessions(&service).await });
                    serde_json::to_string_pretty(&sessions)?
                }
                _ => {
                    anyhow::bail!("unknown export kind: {kind}");
                }
            };
            println!("{output}");
        }
        Command::Version => println!("forgehub {}", env!("CARGO_PKG_VERSION")),
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_configure(
    cli: &Cli,
    non_interactive: bool,
    force: bool,
    dry_run: bool,
    data_dir: Option<PathBuf>,
    enable_tokens: bool,
    token: Option<String>,
    shared_fs: bool,
    edge_id: Option<String>,
    edge_data_dir: Option<PathBuf>,
    edge_ws_url: Option<String>,
    bind: Option<String>,
) -> anyhow::Result<()> {
    let default_data_dir = PathBuf::from("./.forgemux-hub");
    let bind_value = bind.unwrap_or_else(|| cli.bind.clone());
    let data_dir_value = data_dir.unwrap_or(default_data_dir);

    let use_tokens = if non_interactive {
        enable_tokens || token.is_some()
    } else {
        prompt_bool("Enable auth tokens?", false)?
    };
    let token_value = if use_tokens {
        if let Some(token) = token {
            token
        } else if non_interactive {
            Uuid::new_v4().simple().to_string()
        } else {
            let input = prompt_string("Token (leave blank to generate)", None)?;
            if input.is_empty() {
                Uuid::new_v4().simple().to_string()
            } else {
                input
            }
        }
    } else {
        String::new()
    };

    let use_shared_fs = if non_interactive {
        shared_fs
    } else {
        prompt_bool(
            "Hub shares filesystem with edge for session listing?",
            false,
        )?
    };

    let edges = if use_shared_fs {
        let id = if let Some(id) = edge_id {
            id
        } else if non_interactive {
            "edge-01".to_string()
        } else {
            prompt_string("Edge ID", Some("edge-01"))?
        };
        let data_dir = if let Some(dir) = edge_data_dir {
            dir
        } else if non_interactive {
            PathBuf::from("./.forgemux")
        } else {
            let input = prompt_string("Edge data_dir", Some("./.forgemux"))?;
            PathBuf::from(input)
        };
        let ws_url = if let Some(url) = edge_ws_url {
            Some(url)
        } else if non_interactive {
            None
        } else {
            let input = prompt_string("Edge ws_url (optional)", None)?;
            if input.is_empty() { None } else { Some(input) }
        };
        vec![HubEdge {
            id,
            data_dir,
            ws_url,
        }]
    } else {
        Vec::new()
    };

    let config = HubConfig {
        data_dir: data_dir_value.clone(),
        edges,
        tokens: if use_tokens {
            vec![token_value.clone()]
        } else {
            Vec::new()
        },
        organization: None,
        workspaces: Vec::new(),
    };

    write_config_file(&cli.config, &config, force, dry_run)?;
    if !dry_run {
        std::fs::create_dir_all(&data_dir_value)?;
    }

    println!("Configured forgehub.");
    println!("Config: {}", cli.config.display());
    println!("Data dir: {}", data_dir_value.display());
    println!(
        "Run: forgehub --bind {} --config {} run",
        bind_value,
        cli.config.display()
    );
    if use_tokens {
        println!("Token: {}", token_value);
    }
    Ok(())
}

fn prompt_string(prompt: &str, default: Option<&str>) -> anyhow::Result<String> {
    use std::io::{self, Write};
    let mut stdout = io::stdout();
    if let Some(default) = default {
        write!(stdout, "{} [{}]: ", prompt, default)?;
    } else {
        write!(stdout, "{}: ", prompt)?;
    }
    stdout.flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim().to_string();
    if input.is_empty() {
        Ok(default.unwrap_or("").to_string())
    } else {
        Ok(input)
    }
}

fn prompt_bool(prompt: &str, default: bool) -> anyhow::Result<bool> {
    let suffix = if default { "Y/n" } else { "y/N" };
    let input = prompt_string(&format!("{} ({})", prompt, suffix), None)?;
    if input.is_empty() {
        return Ok(default);
    }
    let value = input.to_lowercase();
    Ok(matches!(value.as_str(), "y" | "yes" | "true" | "1"))
}

fn write_config_file<T: serde::Serialize>(
    path: &Path,
    config: &T,
    force: bool,
    dry_run: bool,
) -> anyhow::Result<()> {
    if path.exists() && !force {
        anyhow::bail!("config already exists: {}", path.display());
    }
    let body = toml::to_string_pretty(config)?;
    if dry_run {
        println!("--- {} ---\n{}", path.display(), body);
        return Ok(());
    }
    std::fs::write(path, body)?;
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "healthy" }))
}

async fn dashboard_index() -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    (
        axum::http::StatusCode::OK,
        headers,
        dashboard_bytes("index.html"),
    )
}

async fn dashboard_legacy() -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    (
        axum::http::StatusCode::OK,
        headers,
        dashboard_bytes("index-legacy.html"),
    )
}

async fn dashboard_asset(uri: Uri) -> impl IntoResponse {
    let path = uri.path().trim_start_matches('/');
    if path.is_empty() || path == "index.html" {
        return dashboard_index().await.into_response();
    }
    if path == "legacy" {
        return dashboard_legacy().await.into_response();
    }
    if let Some(body) = DASHBOARD_DIR.get_file(path).map(|file| file.contents()) {
        let mut headers = HeaderMap::new();
        let mime = MimeGuess::from_path(path).first_or_octet_stream();
        if let Ok(value) = HeaderValue::from_str(mime.as_ref()) {
            headers.insert(axum::http::header::CONTENT_TYPE, value);
        }
        return (axum::http::StatusCode::OK, headers, body).into_response();
    }
    axum::http::StatusCode::NOT_FOUND.into_response()
}

fn dashboard_bytes(path: &str) -> &'static [u8] {
    DASHBOARD_DIR
        .get_file(path)
        .map(|file| file.contents())
        .unwrap_or_else(|| b"")
}

#[derive(Debug, Deserialize)]
struct SessionsQuery {
    workspace_id: Option<String>,
    token: Option<String>,
}

async fn list_sessions(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    Query(query): Query<SessionsQuery>,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, query.token.as_deref()) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    let mut sessions = fetch_sessions(&service).await;
    if let Some(workspace_id) = query.workspace_id.as_deref() {
        sessions = service.filter_sessions_by_workspace(sessions, workspace_id);
    }
    (
        axum::http::StatusCode::OK,
        Json(sessions),
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
struct DecisionQuery {
    workspace_id: String,
    repo_id: Option<String>,
    status: Option<String>,
    token: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CreateDecisionRequest {
    session_id: String,
    workspace_id: String,
    repo_id: String,
    question: String,
    context: DecisionContext,
    severity: Severity,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    impact_repo_ids: Vec<String>,
    assigned_to: Option<String>,
    agent_goal: String,
}

#[derive(Debug, Deserialize)]
struct DecisionActionRequest {
    reviewer: String,
    comment: Option<String>,
}

async fn list_decisions(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    Query(query): Query<DecisionQuery>,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, query.token.as_deref()) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    let status = match query.status.as_deref() {
        None => None,
        Some("pending") => Some(DecisionStatus::Pending),
        Some("resolved") => Some(DecisionStatus::Resolved),
        Some(_) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": "invalid status" })),
            )
                .into_response();
        }
    };
    match service
        .list_decisions(&query.workspace_id, query.repo_id.as_deref(), status)
        .await
    {
        Ok(decisions) => (axum::http::StatusCode::OK, Json(decisions)).into_response(),
        Err(err) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn list_workspaces(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    Query(query): Query<AuthQuery>,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, query.token.as_deref()) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    match service.list_workspaces().await {
        Ok(workspaces) => (axum::http::StatusCode::OK, Json(workspaces)).into_response(),
        Err(err) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn get_workspace(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
    Query(query): Query<AuthQuery>,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, query.token.as_deref()) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    match service.get_workspace(&id).await {
        Ok(Some(workspace)) => (axum::http::StatusCode::OK, Json(workspace)).into_response(),
        Ok(None) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "workspace not found" })),
        )
            .into_response(),
        Err(err) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn get_decision(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
    Query(query): Query<AuthQuery>,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, query.token.as_deref()) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    match service.get_decision(&id).await {
        Ok(Some(decision)) => (axum::http::StatusCode::OK, Json(decision)).into_response(),
        Ok(None) => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "not found" })),
        )
            .into_response(),
        Err(err) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn create_decision(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    Json(req): Json<CreateDecisionRequest>,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, None) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    let decision = Decision {
        id: String::new(),
        session_id: req.session_id,
        workspace_id: req.workspace_id,
        repo_id: req.repo_id,
        question: req.question,
        context: req.context,
        severity: req.severity,
        tags: req.tags,
        impact_repo_ids: req.impact_repo_ids,
        assigned_to: req.assigned_to,
        agent_goal: req.agent_goal,
        created_at: Utc::now(),
        resolved_at: None,
        resolution: None,
    };
    match service.create_decision(decision).await {
        Ok(created) => (axum::http::StatusCode::OK, Json(created)).into_response(),
        Err(err) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn decision_approve(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<DecisionActionRequest>,
) -> axum::response::Response {
    decision_action(service, headers, id, DecisionAction::Approve, req).await
}

async fn decision_deny(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<DecisionActionRequest>,
) -> axum::response::Response {
    decision_action(service, headers, id, DecisionAction::Deny, req).await
}

async fn decision_comment(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(req): Json<DecisionActionRequest>,
) -> axum::response::Response {
    if req.comment.as_deref().unwrap_or("").is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "comment required" })),
        )
            .into_response();
    }
    decision_action(service, headers, id, DecisionAction::Comment, req).await
}

async fn decision_action(
    service: Arc<HubService>,
    headers: HeaderMap,
    decision_id: String,
    action: DecisionAction,
    req: DecisionActionRequest,
) -> axum::response::Response {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, None) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    let decision = match service.get_decision(&decision_id).await {
        Ok(Some(decision)) => decision,
        Ok(None) => {
            return (
                axum::http::StatusCode::NOT_FOUND,
                Json(serde_json::json!({ "error": "decision not found" })),
            )
                .into_response();
        }
        Err(err) => {
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": err.to_string() })),
            )
                .into_response();
        }
    };
    if let Err(err) = service
        .resolve_decision(&decision_id, action, &req.reviewer, req.comment.clone())
        .await
    {
        return (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response();
    }
    let mut session_unblocked = action != DecisionAction::Comment;
    if action != DecisionAction::Comment {
        let ok = forward_decision_response(
            &service,
            &decision.session_id,
            action,
            &req.reviewer,
            req.comment.clone(),
        )
        .await;
        if !ok {
            session_unblocked = false;
            return (
                axum::http::StatusCode::BAD_GATEWAY,
                Json(serde_json::json!({
                    "error": "failed to notify edge",
                    "decision_id": decision_id,
                    "action": format!("{action:?}").to_lowercase(),
                    "session_unblocked": session_unblocked,
                })),
            )
                .into_response();
        }
    }
    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({
            "decision_id": decision_id,
            "action": format!("{action:?}").to_lowercase(),
            "session_unblocked": session_unblocked,
        })),
    )
        .into_response()
}

async fn forward_decision_response(
    service: &HubService,
    session_id: &str,
    action: DecisionAction,
    reviewer: &str,
    comment: Option<String>,
) -> bool {
    let payload = serde_json::json!({
        "action": action,
        "reviewer": reviewer,
        "comment": comment,
    });
    let edges = service.list_registered_edges();
    for edge in edges {
        let url = format!(
            "{}/sessions/{}/decision-response",
            normalize_http_addr(&edge.addr),
            session_id
        );
        if let Ok(resp) = reqwest::Client::new().post(url).json(&payload).send().await
            && resp.status().is_success()
        {
            return true;
        }
    }
    false
}

#[derive(Debug, Deserialize)]
struct DecisionsWsQuery {
    workspace_id: String,
    token: Option<String>,
}

async fn ws_decisions(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    Query(query): Query<DecisionsWsQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, query.token.as_deref()) {
        return (axum::http::StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    debug!(workspace_id = %query.workspace_id, "decisions ws connected");
    ws.on_upgrade(move |socket| decisions_socket(service, query.workspace_id, socket))
}

async fn decisions_socket(service: Arc<HubService>, workspace_id: String, mut socket: WebSocket) {
    let pending = service
        .list_decisions(&workspace_id, None, Some(DecisionStatus::Pending))
        .await
        .unwrap_or_default();
    let init = serde_json::json!({ "type": "decisions_init", "decisions": pending });
    if socket.send(Message::Text(init.to_string())).await.is_err() {
        warn!(workspace_id = %workspace_id, "decisions ws send failed");
        return;
    }

    let mut rx = service.subscribe_decisions();
    loop {
        match rx.recv().await {
            Ok(event) => {
                let payload = match &event {
                    DecisionEvent::Created(decision) => {
                        if decision.workspace_id != workspace_id {
                            continue;
                        }
                        serde_json::json!({
                            "type": "decision_created",
                            "decision": decision,
                        })
                    }
                    DecisionEvent::Resolved {
                        decision_id,
                        action,
                    } => {
                        let decision = service.get_decision(decision_id).await.ok().flatten();
                        if let Some(decision) = decision
                            && decision.workspace_id != workspace_id
                        {
                            continue;
                        }
                        serde_json::json!({
                            "type": "decision_resolved",
                            "decision_id": decision_id,
                            "action": format!("{action:?}").to_lowercase(),
                        })
                    }
                };
                if socket
                    .send(Message::Text(payload.to_string()))
                    .await
                    .is_err()
                {
                    warn!(workspace_id = %workspace_id, "decisions ws send failed");
                    break;
                }
            }
            Err(broadcast::error::RecvError::Closed) => break,
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
        }
    }
}

async fn metrics(State(service): State<Arc<HubService>>, headers: HeaderMap) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, None) {
        return (axum::http::StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let edges = service.list_registered_edges().len();
    let sessions = fetch_sessions(&service).await.len();
    let body = format!(
        "forgemux_edges_total {}\nforgemux_sessions_total {}\n",
        edges, sessions
    );
    (axum::http::StatusCode::OK, body).into_response()
}

async fn ws_sessions(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    Query(query): Query<SessionsQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, query.token.as_deref()) {
        return (axum::http::StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    debug!("sessions ws connected");
    ws.on_upgrade(move |socket| sessions_socket(service, query.workspace_id.clone(), socket))
}

async fn sessions_socket(
    service: Arc<HubService>,
    workspace_id: Option<String>,
    mut socket: WebSocket,
) {
    loop {
        let mut sessions = fetch_sessions(&service).await;
        if let Some(id) = workspace_id.as_deref() {
            sessions = service.filter_sessions_by_workspace(sessions, id);
        }
        if socket
            .send(Message::Text(serde_json::to_string(&sessions).unwrap()))
            .await
            .is_err()
        {
            warn!("sessions ws send failed");
            break;
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

async fn fetch_sessions(service: &HubService) -> Vec<forgemux_core::SessionRecord> {
    let client = reqwest::Client::new();
    let edges = service.list_registered_edges();
    let futures = edges.into_iter().map(|edge| {
        let url = format!("{}/sessions", normalize_http_addr(&edge.addr));
        let client = client.clone();
        async move { client.get(url).send().await }
    });
    let mut sessions = Vec::new();
    for response in join_all(futures).await.into_iter().flatten() {
        if response.status().is_success()
            && let Ok(mut edge_sessions) =
                response.json::<Vec<forgemux_core::SessionRecord>>().await
        {
            sessions.append(&mut edge_sessions);
        }
    }
    forgemux_core::sort_sessions(sessions)
}

async fn list_edges(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, None) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    (
        axum::http::StatusCode::OK,
        Json(service.list_registered_edges()),
    )
        .into_response()
}

async fn edge_config(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, None) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    let edge = service
        .list_registered_edges()
        .into_iter()
        .find(|edge| edge.id == id);
    let Some(edge) = edge else {
        return (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "unknown edge" })),
        )
            .into_response();
    };
    let url = format!("{}/config", normalize_http_addr(&edge.addr));
    match reqwest::Client::new().get(url).send().await {
        Ok(resp) => {
            let status = resp.status();
            let body = resp
                .json::<serde_json::Value>()
                .await
                .unwrap_or_else(|_| serde_json::json!({ "error": "invalid response" }));
            (status, Json(body)).into_response()
        }
        Err(err) => (
            axum::http::StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Debug, serde::Deserialize)]
struct RegisterEdgeRequest {
    id: String,
    addr: String,
}

async fn register_edge(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    Json(req): Json<RegisterEdgeRequest>,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, None) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    (
        axum::http::StatusCode::OK,
        Json(service.register_edge(req.id, req.addr)),
    )
        .into_response()
}

#[derive(Debug, serde::Deserialize)]
struct HeartbeatRequest {
    id: String,
}

async fn heartbeat(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    Json(req): Json<HeartbeatRequest>,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, None) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    match service.heartbeat(&req.id) {
        Some(reg) => (axum::http::StatusCode::OK, Json(reg)).into_response(),
        None => (
            axum::http::StatusCode::NOT_FOUND,
            Json(serde_json::json!({ "error": "unknown edge" })),
        )
            .into_response(),
    }
}

async fn start_session(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    Json(mut payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, None) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    let edge_id = payload
        .get("edge_id")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string());
    if let Some(obj) = payload.as_object_mut() {
        obj.remove("edge_id");
    }
    let edge = if let Some(edge_id) = edge_id {
        service
            .list_registered_edges()
            .into_iter()
            .find(|edge| edge.id == edge_id)
            .ok_or_else(|| {
                (
                    axum::http::StatusCode::NOT_FOUND,
                    Json(serde_json::json!({ "error": "unknown edge" })),
                )
                    .into_response()
            })
    } else {
        service.pick_edge().ok_or_else(|| {
            (
                axum::http::StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({ "error": "no edges registered" })),
            )
                .into_response()
        })
    };
    let edge = match edge {
        Ok(edge) => edge,
        Err(resp) => return resp,
    };
    let url = format!("{}/sessions/start", normalize_http_addr(&edge.addr));
    match reqwest::Client::new().post(url).json(&payload).send().await {
        Ok(resp) => {
            let status = resp.status();
            let body = resp
                .json::<serde_json::Value>()
                .await
                .unwrap_or_else(|_| serde_json::json!({ "error": "invalid response" }));
            (status, Json(body)).into_response()
        }
        Err(err) => (
            axum::http::StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn stop_session(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, None) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    let edges = service.list_registered_edges();
    for edge in edges {
        let url = format!("{}/sessions/{}/stop", normalize_http_addr(&edge.addr), id);
        if let Ok(resp) = reqwest::Client::new().post(url).send().await
            && resp.status().is_success()
        {
            return (
                axum::http::StatusCode::OK,
                Json(serde_json::json!({ "status": "stopped" })),
            )
                .into_response();
        }
    }
    (
        axum::http::StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "session not found" })),
    )
        .into_response()
}

async fn kill_session(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, None) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    let edges = service.list_registered_edges();
    for edge in edges {
        let url = format!("{}/sessions/{}/kill", normalize_http_addr(&edge.addr), id);
        if let Ok(resp) = reqwest::Client::new().post(url).send().await
            && resp.status().is_success()
        {
            return (
                axum::http::StatusCode::OK,
                Json(serde_json::json!({ "status": "killed" })),
            )
                .into_response();
        }
    }
    (
        axum::http::StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "session not found" })),
    )
        .into_response()
}

async fn session_logs(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, None) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    let edges = service.list_registered_edges();
    for edge in edges {
        let url = format!("{}/sessions/{}/logs", normalize_http_addr(&edge.addr), id);
        if let Ok(resp) = reqwest::Client::new().get(url).send().await
            && resp.status().is_success()
        {
            let body = resp
                .json::<serde_json::Value>()
                .await
                .unwrap_or_else(|_| serde_json::json!({ "error": "invalid response" }));
            return (axum::http::StatusCode::OK, Json(body)).into_response();
        }
    }
    (
        axum::http::StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "session not found" })),
    )
        .into_response()
}

async fn session_input(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, None) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    let edges = service.list_registered_edges();
    for edge in edges {
        let url = format!("{}/sessions/{}/input", normalize_http_addr(&edge.addr), id);
        if let Ok(resp) = reqwest::Client::new().post(url).json(&payload).send().await
            && resp.status().is_success()
        {
            return (
                axum::http::StatusCode::OK,
                Json(serde_json::json!({ "status": "sent" })),
            )
                .into_response();
        }
    }
    (
        axum::http::StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "session not found" })),
    )
        .into_response()
}

async fn session_usage(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, None) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    let edges = service.list_registered_edges();
    for edge in edges {
        let url = format!("{}/sessions/{}/usage", normalize_http_addr(&edge.addr), id);
        if let Ok(resp) = reqwest::Client::new().get(url).send().await
            && resp.status().is_success()
        {
            let body = resp
                .json::<serde_json::Value>()
                .await
                .unwrap_or_else(|_| serde_json::json!({ "error": "invalid response" }));
            return (axum::http::StatusCode::OK, Json(body)).into_response();
        }
    }
    (
        axum::http::StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "session not found" })),
    )
        .into_response()
}

#[derive(Debug, Deserialize)]
struct ReplayQuery {
    after: Option<u64>,
    limit: Option<u32>,
}

async fn replay_timeline(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
    Query(query): Query<ReplayQuery>,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, None) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    let limit = query.limit.unwrap_or(200).min(500);
    match service.replay_timeline(&id, query.after, limit).await {
        Ok((events, next_cursor)) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({ "events": events, "next_cursor": next_cursor })),
        )
            .into_response(),
        Err(err) => (
            axum::http::StatusCode::INTERNAL_SERVER_ERROR,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

async fn replay_diff(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, None) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    let edges = service.list_registered_edges();
    for edge in edges {
        let url = format!(
            "{}/sessions/{}/replay/diff",
            normalize_http_addr(&edge.addr),
            id
        );
        if let Ok(resp) = reqwest::Client::new().get(url).send().await
            && resp.status().is_success()
        {
            let body = resp
                .json::<serde_json::Value>()
                .await
                .unwrap_or_else(|_| serde_json::json!({ "groups": [] }));
            return (axum::http::StatusCode::OK, Json(body)).into_response();
        }
    }
    (
        axum::http::StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "session not found" })),
    )
        .into_response()
}

async fn replay_terminal(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, None) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    let edges = service.list_registered_edges();
    for edge in edges {
        let url = format!("{}/sessions/{}/logs", normalize_http_addr(&edge.addr), id);
        if let Ok(resp) = reqwest::Client::new().get(url).send().await
            && resp.status().is_success()
        {
            let body = resp
                .json::<serde_json::Value>()
                .await
                .unwrap_or_else(|_| serde_json::json!({ "error": "invalid response" }));
            return (axum::http::StatusCode::OK, Json(body)).into_response();
        }
    }
    (
        axum::http::StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "session not found" })),
    )
        .into_response()
}

async fn foreman_report(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, None) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    let Some(edge) = service.pick_edge() else {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "no edges registered" })),
        )
            .into_response();
    };
    let url = format!("{}/foreman/report", normalize_http_addr(&edge.addr));
    match reqwest::Client::new().get(url).send().await {
        Ok(resp) => {
            let status = resp.status();
            let body = resp
                .json::<serde_json::Value>()
                .await
                .unwrap_or_else(|_| serde_json::json!({ "error": "invalid response" }));
            (status, Json(body)).into_response()
        }
        Err(err) => (
            axum::http::StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({ "error": err.to_string() })),
        )
            .into_response(),
    }
}

#[derive(Debug, serde::Deserialize)]
struct PairingStartRequest {
    ttl_secs: Option<i64>,
}

#[derive(Debug, serde::Deserialize)]
struct PairingExchangeRequest {
    token: String,
    ttl_secs: Option<i64>,
}

async fn pairing_start(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    Json(req): Json<PairingStartRequest>,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if service.tokens_required() && !authorized(&service, &headers, None) {
        return (
            axum::http::StatusCode::UNAUTHORIZED,
            Json(serde_json::json!({ "error": "unauthorized" })),
        )
            .into_response();
    }
    let ttl = req.ttl_secs.unwrap_or(600);
    let pairing = service.start_pairing(ttl);
    let host = headers
        .get(axum::http::header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("localhost");
    let url = format!("http://{host}/pair?token={}", pairing.token);
    (
        axum::http::StatusCode::OK,
        Json(serde_json::json!({
            "pair_token": pairing.token,
            "expires_at": pairing.expires_at,
            "url": url,
        })),
    )
        .into_response()
}

async fn pairing_exchange(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    Json(req): Json<PairingExchangeRequest>,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    let ttl = req.ttl_secs.unwrap_or(86_400);
    match service.exchange_pairing(&req.token, ttl) {
        Some(issued) => (
            axum::http::StatusCode::OK,
            Json(serde_json::json!({
                "access_token": issued.token,
                "expires_at": issued.expires_at,
            })),
        )
            .into_response(),
        None => (
            axum::http::StatusCode::BAD_REQUEST,
            Json(serde_json::json!({ "error": "invalid token" })),
        )
            .into_response(),
    }
}

#[derive(Debug, serde::Deserialize)]
struct PairingQuery {
    token: String,
}

async fn pairing_landing(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    Query(query): Query<PairingQuery>,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    let issued = match service.exchange_pairing(&query.token, 86_400) {
        Some(token) => token,
        None => {
            return (axum::http::StatusCode::BAD_REQUEST, "invalid pairing token").into_response();
        }
    };
    let body = format!(
        r#"<!DOCTYPE html>
<html>
  <head>
    <meta charset="utf-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1" />
    <title>Forgemux Pairing</title>
  </head>
  <body>
    <p>Pairing successful. Redirecting...</p>
    <script>
      window.location.href = "/?token={token}";
    </script>
  </body>
</html>"#,
        token = issued.token
    );
    (
        axum::http::StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")],
        body,
    )
        .into_response()
}

fn normalize_http_addr(addr: &str) -> String {
    if addr.starts_with("http://") || addr.starts_with("https://") {
        addr.to_string()
    } else {
        format!("http://{addr}")
    }
}

async fn ws_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_socket)
}

async fn handle_socket(mut socket: WebSocket) {
    while let Some(Ok(msg)) = socket.recv().await {
        if let Message::Close(_) = msg {
            break;
        }
        let _ = socket.send(msg).await;
    }
}

#[derive(Debug, serde::Deserialize)]
struct AttachQuery {
    edge: Option<String>,
    token: Option<String>,
}

async fn ws_attach(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
    Query(query): Query<AttachQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, query.token.as_deref()) {
        return (axum::http::StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    let ws_url = service
        .resolve_ws_url(query.edge.as_deref())
        .unwrap_or_default();
    ws.on_upgrade(move |socket| relay_ws(ws_url, id, socket))
}

#[derive(Debug, serde::Deserialize)]
struct AuthQuery {
    token: Option<String>,
}

fn authorized(service: &HubService, headers: &HeaderMap, token: Option<&str>) -> bool {
    if !service.tokens_required() {
        return true;
    }
    if let Some(token) = token {
        return service.is_token_valid(token);
    }
    if let Some(value) = headers.get(axum::http::header::AUTHORIZATION)
        && let Ok(value) = value.to_str()
        && let Some(token) = value.strip_prefix("Bearer ")
    {
        return service.is_token_valid(token);
    }
    false
}

fn check_version(headers: &HeaderMap) -> Option<axum::response::Response> {
    let version = headers
        .get(FORGEHUB_VERSION_HEADER)
        .and_then(|value| value.to_str().ok())?;
    if version_compatible(version) {
        None
    } else {
        Some(
            (
                axum::http::StatusCode::UPGRADE_REQUIRED,
                format!(
                    "forgehub {} requires fmux >= {}.x",
                    env!("CARGO_PKG_VERSION"),
                    env!("CARGO_PKG_VERSION").split('.').next().unwrap_or("0")
                ),
            )
                .into_response(),
        )
    }
}

async fn version() -> impl IntoResponse {
    Json(serde_json::json!({
        "name": "forgehub",
        "version": env!("CARGO_PKG_VERSION"),
    }))
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

async fn relay_ws(ws_url: String, id: String, mut client: WebSocket) {
    if ws_url.is_empty() {
        let _ = client
            .send(Message::Text("no edge ws_url configured".to_string()))
            .await;
        return;
    }
    let target = format!("{}/sessions/{}/attach", ws_url.trim_end_matches('/'), id);
    let mut buffer = RelayBuffer::new(256);
    let mut upstream = None;

    loop {
        if upstream.is_none() {
            match tokio_tungstenite::connect_async(target.clone()).await {
                Ok((socket, _)) => {
                    upstream = Some(socket);
                    while let Some(msg) = buffer.pop_front() {
                        if let Some(upstream_socket) = upstream.as_mut() {
                            let _ = upstream_socket.send(msg).await;
                        }
                    }
                }
                Err(_) => {
                    tokio::select! {
                        msg = client.recv() => {
                            if let Some(Ok(msg)) = msg {
                                if let Some(tungstenite_msg) = to_tungstenite(msg) {
                                    buffer.push(tungstenite_msg);
                                } else {
                                    break;
                                }
                            } else {
                                break;
                            }
                        }
                        _ = tokio::time::sleep(std::time::Duration::from_secs(1)) => {}
                    }
                    continue;
                }
            }
        }

        let Some(upstream_socket) = upstream.as_mut() else {
            continue;
        };
        tokio::select! {
            msg = client.recv() => {
                match msg {
                    Some(Ok(msg)) => {
                        if let Some(tungstenite_msg) = to_tungstenite(msg) {
                            if upstream_socket.send(tungstenite_msg).await.is_err() {
                                upstream = None;
                            }
                        } else {
                            let _ = upstream_socket.close(None).await;
                            break;
                        }
                    }
                    _ => break,
                }
            }
            msg = upstream_socket.next() => {
                match msg {
                    Some(Ok(msg)) => {
                        if let Some(axum_msg) = to_axum(msg)
                            && client.send(axum_msg).await.is_err()
                        {
                            break;
                        }
                    }
                    _ => {
                        upstream = None;
                    }
                }
            }
        }
    }
}

fn to_tungstenite(msg: Message) -> Option<TungsteniteMessage> {
    match msg {
        Message::Text(text) => Some(TungsteniteMessage::Text(text)),
        Message::Binary(bin) => Some(TungsteniteMessage::Binary(bin)),
        Message::Close(_) => None,
        _ => None,
    }
}

fn to_axum(msg: TungsteniteMessage) -> Option<Message> {
    match msg {
        TungsteniteMessage::Text(text) => Some(Message::Text(text)),
        TungsteniteMessage::Binary(bin) => Some(Message::Binary(bin)),
        TungsteniteMessage::Close(_) => None,
        _ => None,
    }
}

struct RelayBuffer {
    max: usize,
    queue: VecDeque<TungsteniteMessage>,
}

impl RelayBuffer {
    fn new(max: usize) -> Self {
        Self {
            max: max.max(1),
            queue: VecDeque::new(),
        }
    }

    fn push(&mut self, msg: TungsteniteMessage) {
        if self.queue.len() >= self.max {
            self.queue.pop_front();
        }
        self.queue.push_back(msg);
    }

    fn pop_front(&mut self) -> Option<TungsteniteMessage> {
        self.queue.pop_front()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use forgemux_core::{ReplayEvent, ReplayEventType};
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn register_and_list_edges() {
        let tmp = tempfile::tempdir().unwrap();
        let service = Arc::new(
            HubService::new(HubConfig {
                data_dir: tmp.path().join("hub"),
                edges: Vec::new(),
                tokens: Vec::new(),
                organization: None,
                workspaces: Vec::new(),
            })
            .await
            .unwrap(),
        );
        let app = Router::new()
            .route("/edges", get(list_edges))
            .route("/edges/register", post(register_edge))
            .with_state(service);

        let body = serde_json::json!({
            "id": "edge-01",
            "addr": "127.0.0.1:9443"
        });
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/edges/register")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/edges")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn decision_roundtrip_endpoints() {
        let edge_app = Router::new().route(
            "/sessions/:id/decision-response",
            post(
                |axum::extract::Path(_id): axum::extract::Path<String>| async {
                    Json(serde_json::json!({ "status": "ok" }))
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, edge_app).await.unwrap();
        });

        let tmp = tempfile::tempdir().unwrap();
        let service = Arc::new(
            HubService::new(HubConfig {
                data_dir: tmp.path().join("hub"),
                edges: Vec::new(),
                tokens: Vec::new(),
                organization: None,
                workspaces: Vec::new(),
            })
            .await
            .unwrap(),
        );
        service.register_edge("edge-01".to_string(), addr.to_string());
        let app = Router::new()
            .route("/decisions", get(list_decisions).post(create_decision))
            .route("/decisions/:id/approve", post(decision_approve))
            .with_state(service);

        let payload = serde_json::json!({
            "session_id": "S-1234abcd",
            "workspace_id": "ws-1",
            "repo_id": "repo-1",
            "question": "Ship the change?",
            "context": { "type": "log", "text": "diff summary" },
            "severity": "high",
            "tags": ["release"],
            "impact_repo_ids": ["repo-2"],
            "assigned_to": null,
            "agent_goal": "Ship release"
        });
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/decisions")
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let decision_id = created
            .get("id")
            .and_then(|value| value.as_str())
            .unwrap()
            .to_string();

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/decisions?workspace_id=ws-1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let approve = serde_json::json!({ "reviewer": "jono", "comment": "ok" });
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/decisions/{}/approve", decision_id))
                    .header("content-type", "application/json")
                    .body(Body::from(approve.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/decisions?workspace_id=ws-1&status=resolved")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[test]
    fn normalize_http_addr_adds_scheme() {
        assert_eq!(
            normalize_http_addr("127.0.0.1:9000"),
            "http://127.0.0.1:9000"
        );
        assert_eq!(
            normalize_http_addr("https://edge.local:9443"),
            "https://edge.local:9443"
        );
    }

    #[tokio::test]
    async fn foreman_report_proxies_to_edge() {
        let edge_app = Router::new().route(
            "/foreman/report",
            get(|| async { Json(serde_json::json!({ "ok": true })) }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, edge_app).await.unwrap();
        });

        let tmp = tempfile::tempdir().unwrap();
        let service = Arc::new(
            HubService::new(HubConfig {
                data_dir: tmp.path().join("hub"),
                edges: Vec::new(),
                tokens: Vec::new(),
                organization: None,
                workspaces: Vec::new(),
            })
            .await
            .unwrap(),
        );
        service.register_edge("edge-01".to_string(), addr.to_string());
        let app = Router::new()
            .route("/foreman/report", get(foreman_report))
            .with_state(service);

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
    async fn session_input_proxies_to_edge() {
        let edge_app = Router::new().route(
            "/sessions/:id/input",
            post(
                |axum::extract::Path(_id): axum::extract::Path<String>| async {
                    Json(serde_json::json!({ "status": "sent" }))
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, edge_app).await.unwrap();
        });

        let tmp = tempfile::tempdir().unwrap();
        let service = Arc::new(
            HubService::new(HubConfig {
                data_dir: tmp.path().join("hub"),
                edges: Vec::new(),
                tokens: Vec::new(),
                organization: None,
                workspaces: Vec::new(),
            })
            .await
            .unwrap(),
        );
        service.register_edge("edge-01".to_string(), addr.to_string());
        let app = Router::new()
            .route("/sessions/:id/input", post(session_input))
            .with_state(service);

        let body = serde_json::json!({ "input": "echo hi" });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/sessions/S-1/input")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn session_kill_proxies_to_edge() {
        let edge_app = Router::new().route(
            "/sessions/:id/kill",
            post(
                |axum::extract::Path(_id): axum::extract::Path<String>| async {
                    Json(serde_json::json!({ "status": "killed" }))
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, edge_app).await.unwrap();
        });

        let tmp = tempfile::tempdir().unwrap();
        let service = Arc::new(
            HubService::new(HubConfig {
                data_dir: tmp.path().join("hub"),
                edges: Vec::new(),
                tokens: Vec::new(),
                organization: None,
                workspaces: Vec::new(),
            })
            .await
            .unwrap(),
        );
        service.register_edge("edge-01".to_string(), addr.to_string());
        let app = Router::new()
            .route("/sessions/:id/kill", post(kill_session))
            .with_state(service);

        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/sessions/S-1/kill")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn session_usage_proxies_to_edge() {
        let edge_app = Router::new().route(
            "/sessions/:id/usage",
            get(
                |axum::extract::Path(_id): axum::extract::Path<String>| async {
                    Json(serde_json::json!({ "total_tokens": 0 }))
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, edge_app).await.unwrap();
        });

        let tmp = tempfile::tempdir().unwrap();
        let service = Arc::new(
            HubService::new(HubConfig {
                data_dir: tmp.path().join("hub"),
                edges: Vec::new(),
                tokens: Vec::new(),
                organization: None,
                workspaces: Vec::new(),
            })
            .await
            .unwrap(),
        );
        service.register_edge("edge-01".to_string(), addr.to_string());
        let app = Router::new()
            .route("/sessions/:id/usage", get(session_usage))
            .with_state(service);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/sessions/S-1/usage")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn decision_resolution_forwards_to_edge() {
        let captured = Arc::new(std::sync::Mutex::new(None));
        let captured_clone = captured.clone();
        let edge_app = Router::new().route(
            "/sessions/:id/decision-response",
            post(
                move |axum::extract::Path(id): axum::extract::Path<String>,
                      Json(payload): Json<serde_json::Value>| async move {
                    let mut guard = captured_clone.lock().unwrap();
                    *guard = Some(serde_json::json!({ "id": id, "payload": payload }));
                    Json(serde_json::json!({ "status": "ok" }))
                },
            ),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, edge_app).await.unwrap();
        });

        let tmp = tempfile::tempdir().unwrap();
        let service = Arc::new(
            HubService::new(HubConfig {
                data_dir: tmp.path().join("hub"),
                edges: Vec::new(),
                tokens: Vec::new(),
                organization: None,
                workspaces: Vec::new(),
            })
            .await
            .unwrap(),
        );
        service.register_edge("edge-01".to_string(), addr.to_string());
        let app = Router::new()
            .route("/decisions", post(create_decision))
            .route("/decisions/:id/approve", post(decision_approve))
            .with_state(service);

        let payload = serde_json::json!({
            "session_id": "S-1234",
            "workspace_id": "ws-1",
            "repo_id": "repo-1",
            "question": "Proceed?",
            "context": { "type": "log", "text": "ok" },
            "severity": "high",
            "tags": [],
            "impact_repo_ids": [],
            "assigned_to": null,
            "agent_goal": "Ship"
        });
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/decisions")
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let decision_id = created.get("id").and_then(|value| value.as_str()).unwrap();

        let approve = serde_json::json!({ "reviewer": "jono", "comment": "ok" });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/decisions/{}/approve", decision_id))
                    .header("content-type", "application/json")
                    .body(Body::from(approve.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let guard = captured.lock().unwrap();
        let entry = guard.as_ref().expect("edge payload");
        assert_eq!(entry.get("id").and_then(|v| v.as_str()), Some("S-1234"));
        let payload = entry.get("payload").unwrap();
        assert_eq!(
            payload.get("reviewer").and_then(|v| v.as_str()),
            Some("jono")
        );
    }

    #[tokio::test]
    async fn decision_flow_over_http() {
        let captured = Arc::new(std::sync::Mutex::new(None));
        let captured_clone = captured.clone();
        let edge_app = Router::new().route(
            "/sessions/:id/decision-response",
            post(
                move |axum::extract::Path(id): axum::extract::Path<String>,
                      Json(payload): Json<serde_json::Value>| async move {
                    let mut guard = captured_clone.lock().unwrap();
                    *guard = Some(serde_json::json!({ "id": id, "payload": payload }));
                    Json(serde_json::json!({ "status": "ok" }))
                },
            ),
        );
        let edge_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let edge_addr = edge_listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(edge_listener, edge_app).await.unwrap();
        });

        let tmp = tempfile::tempdir().unwrap();
        let service = Arc::new(
            HubService::new(HubConfig {
                data_dir: tmp.path().join("hub"),
                edges: Vec::new(),
                tokens: Vec::new(),
                organization: None,
                workspaces: Vec::new(),
            })
            .await
            .unwrap(),
        );
        let app = Router::new()
            .route("/edges/register", post(register_edge))
            .route("/decisions", post(create_decision))
            .route("/decisions/:id/approve", post(decision_approve))
            .with_state(service);
        let hub_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let hub_addr = hub_listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(hub_listener, app).await.unwrap();
        });

        let client = reqwest::Client::new();
        let register = serde_json::json!({
            "id": "edge-01",
            "addr": edge_addr.to_string(),
        });
        let register_resp = client
            .post(format!("http://{}/edges/register", hub_addr))
            .json(&register)
            .send()
            .await
            .unwrap();
        assert!(register_resp.status().is_success());

        let payload = serde_json::json!({
            "session_id": "S-5678",
            "workspace_id": "ws-1",
            "repo_id": "repo-1",
            "question": "Ship?",
            "context": { "type": "log", "text": "ok" },
            "severity": "high",
            "tags": [],
            "impact_repo_ids": [],
            "assigned_to": null,
            "agent_goal": "Ship"
        });
        let create_resp = client
            .post(format!("http://{}/decisions", hub_addr))
            .json(&payload)
            .send()
            .await
            .unwrap();
        assert!(create_resp.status().is_success());
        let created: serde_json::Value = create_resp.json().await.unwrap();
        let decision_id = created.get("id").and_then(|value| value.as_str()).unwrap();

        let approve = serde_json::json!({ "reviewer": "jono", "comment": "ok" });
        let approve_resp = client
            .post(format!(
                "http://{}/decisions/{}/approve",
                hub_addr, decision_id
            ))
            .json(&approve)
            .send()
            .await
            .unwrap();
        assert!(approve_resp.status().is_success());

        let guard = captured.lock().unwrap();
        let entry = guard.as_ref().expect("edge payload");
        assert_eq!(entry.get("id").and_then(|v| v.as_str()), Some("S-5678"));
    }

    #[tokio::test]
    async fn replay_timeline_returns_events() {
        let tmp = tempfile::tempdir().unwrap();
        let service = Arc::new(
            HubService::new(HubConfig {
                data_dir: tmp.path().join("hub"),
                edges: Vec::new(),
                tokens: Vec::new(),
                organization: None,
                workspaces: Vec::new(),
            })
            .await
            .unwrap(),
        );
        let event = ReplayEvent {
            id: 0,
            session_id: "S-1234".to_string(),
            repo_id: Some("repo-1".to_string()),
            timestamp: Utc::now(),
            elapsed: "1m".to_string(),
            event_type: ReplayEventType::Tool,
            action: "cargo test".to_string(),
            result: Some("pass".to_string()),
            payload: None,
        };
        service.record_replay_event(event).await.unwrap();

        let app = Router::new()
            .route("/sessions/:id/replay/timeline", get(replay_timeline))
            .with_state(service);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/sessions/S-1234/replay/timeline")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let action = json
            .get("events")
            .and_then(|events| events.as_array())
            .and_then(|events| events.first())
            .and_then(|event| event.get("action"))
            .and_then(|value| value.as_str())
            .unwrap();
        assert_eq!(action, "cargo test");
    }

    #[tokio::test]
    async fn replay_diff_proxies_to_edge() {
        let edge_app = Router::new().route(
            "/sessions/:id/replay/diff",
            get(|axum::extract::Path(_id): axum::extract::Path<String>| async {
                Json(serde_json::json!({
                    "groups": [
                        { "repo": "repo-1", "files": [ { "path": "src/lib.rs", "additions": 1, "deletions": 0 } ] }
                    ]
                }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, edge_app).await.unwrap();
        });

        let tmp = tempfile::tempdir().unwrap();
        let service = Arc::new(
            HubService::new(HubConfig {
                data_dir: tmp.path().join("hub"),
                edges: Vec::new(),
                tokens: Vec::new(),
                organization: None,
                workspaces: Vec::new(),
            })
            .await
            .unwrap(),
        );
        service.register_edge("edge-01".to_string(), addr.to_string());
        let app = Router::new()
            .route("/sessions/:id/replay/diff", get(replay_diff))
            .with_state(service);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/sessions/S-1/replay/diff")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let repo = json
            .get("groups")
            .and_then(|groups| groups.as_array())
            .and_then(|groups| groups.first())
            .and_then(|group| group.get("repo"))
            .and_then(|value| value.as_str())
            .unwrap();
        assert_eq!(repo, "repo-1");
    }

    #[tokio::test]
    async fn start_session_respects_edge_id() {
        let edge_app_a = Router::new().route(
            "/sessions/start",
            post(|| async { Json(serde_json::json!({ "session_id": "S-A" })) }),
        );
        let edge_app_b = Router::new().route(
            "/sessions/start",
            post(|| async { Json(serde_json::json!({ "session_id": "S-B" })) }),
        );

        let listener_a = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr_a = listener_a.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener_a, edge_app_a).await.unwrap();
        });

        let listener_b = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr_b = listener_b.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener_b, edge_app_b).await.unwrap();
        });

        let tmp = tempfile::tempdir().unwrap();
        let service = Arc::new(
            HubService::new(HubConfig {
                data_dir: tmp.path().join("hub"),
                edges: Vec::new(),
                tokens: Vec::new(),
                organization: None,
                workspaces: Vec::new(),
            })
            .await
            .unwrap(),
        );
        service.register_edge("edge-a".to_string(), addr_a.to_string());
        service.register_edge("edge-b".to_string(), addr_b.to_string());

        let app = Router::new()
            .route("/sessions", post(start_session))
            .with_state(service);

        let payload = serde_json::json!({
            "edge_id": "edge-b",
            "agent": "claude",
            "model": "sonnet",
            "repo": "/tmp",
            "worktree": false
        });
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/sessions")
                    .header("content-type", "application/json")
                    .body(Body::from(payload.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json.get("session_id").and_then(|v| v.as_str()), Some("S-B"));
    }

    #[test]
    fn relay_buffer_evicts_oldest() {
        let mut buffer = RelayBuffer::new(2);
        buffer.push(TungsteniteMessage::Text("a".to_string()));
        buffer.push(TungsteniteMessage::Text("b".to_string()));
        buffer.push(TungsteniteMessage::Text("c".to_string()));
        let first = buffer.pop_front().unwrap();
        let second = buffer.pop_front().unwrap();
        assert_eq!(first, TungsteniteMessage::Text("b".to_string()));
        assert_eq!(second, TungsteniteMessage::Text("c".to_string()));
    }

    #[test]
    fn dashboard_includes_attach_and_queue() {
        let html = std::str::from_utf8(dashboard_bytes("index.html")).unwrap();
        let legacy = std::str::from_utf8(dashboard_bytes("index-legacy.html")).unwrap();
        assert!(html.contains("app.js"));
        assert!(html.contains("id=\"app\""));
        assert!(legacy.contains("sessions/ws"));
    }

    #[tokio::test]
    async fn routes_basic_health() {
        let tmp = tempfile::tempdir().unwrap();
        let service = Arc::new(
            HubService::new(HubConfig {
                data_dir: tmp.path().join("hub"),
                edges: Vec::new(),
                tokens: Vec::new(),
                organization: None,
                workspaces: Vec::new(),
            })
            .await
            .unwrap(),
        );
        let app = Router::new()
            .route("/health", get(health))
            .route("/sessions", get(list_sessions))
            .route("/edges", get(list_edges))
            .route("/metrics", get(metrics))
            .with_state(service);

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
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/sessions")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/edges")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);

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

    #[tokio::test]
    async fn routes_reject_incompatible_version() {
        let tmp = tempfile::tempdir().unwrap();
        let service = Arc::new(
            HubService::new(HubConfig {
                data_dir: tmp.path().join("hub"),
                edges: Vec::new(),
                tokens: Vec::new(),
                organization: None,
                workspaces: Vec::new(),
            })
            .await
            .unwrap(),
        );
        let app = Router::new()
            .route("/sessions", get(list_sessions))
            .with_state(service);

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
    async fn pairing_start_returns_token() {
        let tmp = tempfile::tempdir().unwrap();
        let service = Arc::new(
            HubService::new(HubConfig {
                data_dir: tmp.path().join("hub"),
                edges: Vec::new(),
                tokens: Vec::new(),
                organization: None,
                workspaces: Vec::new(),
            })
            .await
            .unwrap(),
        );
        let app = Router::new()
            .route("/pairing/start", post(pairing_start))
            .with_state(service);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/pairing/start")
                    .header("content-type", "application/json")
                    .body(Body::from("{\"ttl_secs\":60}"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn routes_require_auth_when_configured() {
        let tmp = tempfile::tempdir().unwrap();
        let service = Arc::new(
            HubService::new(HubConfig {
                data_dir: tmp.path().join("hub"),
                edges: Vec::new(),
                tokens: vec!["secret".to_string()],
                organization: None,
                workspaces: Vec::new(),
            })
            .await
            .unwrap(),
        );
        let app = Router::new()
            .route("/edges", get(list_edges))
            .with_state(service);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/edges")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
