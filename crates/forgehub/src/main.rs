use axum::{
    Json, Router,
    extract::{
        Query, State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{HeaderMap, HeaderValue},
    response::IntoResponse,
    routing::{get, post},
};
use clap::{Parser, Subcommand};
use forgehub::{EdgeRegistration, HubConfig, HubService};
use futures_util::{SinkExt, StreamExt, future::join_all};
use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tokio_tungstenite::tungstenite::Message as TungsteniteMessage;
use tower_http::services::ServeDir;

const DASHBOARD_HTML: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../dashboard/index.html"
));
const FORGEHUB_VERSION_HEADER: &str = "x-forgemux-version";

#[derive(Debug, Subcommand)]
enum Command {
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
        }
    };
    let service = HubService::new(config);

    match cli.command {
        Command::Run => {
            let addr: SocketAddr = cli.bind.parse()?;
            let shared = Arc::new(service);
            let app = Router::new()
                .route("/health", get(health))
                .route("/metrics", get(metrics))
                .route("/sessions", get(list_sessions).post(start_session))
                .route("/sessions/ws", get(ws_sessions))
                .route("/sessions/:id/stop", post(stop_session))
                .route("/sessions/:id/logs", get(session_logs))
                .route("/sessions/:id/input", post(session_input))
                .route("/sessions/:id/usage", get(session_usage))
                .route("/foreman/report", get(foreman_report))
                .route("/pairing/start", post(pairing_start))
                .route("/pairing/exchange", post(pairing_exchange))
                .route("/pair", get(pairing_landing))
                .route("/edges", get(list_edges))
                .route("/edges/register", post(register_edge))
                .route("/edges/heartbeat", post(heartbeat))
                .route("/ws", get(ws_handler))
                .route("/sessions/:id/attach", get(ws_attach))
                .route("/", get(dashboard_index))
                .route("/index.html", get(dashboard_index))
                .fallback_service(ServeDir::new("dashboard"))
                .with_state(shared);
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async move {
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
            let rt = tokio::runtime::Runtime::new()?;
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

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "healthy" }))
}

async fn dashboard_index() -> impl IntoResponse {
    let mut headers = HeaderMap::new();
    headers.insert(
        axum::http::header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    (axum::http::StatusCode::OK, headers, DASHBOARD_HTML)
}

async fn list_sessions(
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
        Json(fetch_sessions(&service).await),
    )
        .into_response()
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
    Query(query): Query<AuthQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    if let Some(resp) = check_version(&headers) {
        return resp;
    }
    if !authorized(&service, &headers, query.token.as_deref()) {
        return (axum::http::StatusCode::UNAUTHORIZED, "unauthorized").into_response();
    }
    ws.on_upgrade(move |socket| sessions_socket(service, socket))
}

async fn sessions_socket(service: Arc<HubService>, mut socket: WebSocket) {
    loop {
        let sessions = fetch_sessions(&service).await;
        if socket
            .send(Message::Text(serde_json::to_string(&sessions).unwrap()))
            .await
            .is_err()
        {
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

#[derive(Debug, serde::Deserialize)]
struct RegisterEdgeRequest {
    id: String,
    addr: String,
}

async fn register_edge(
    State(service): State<Arc<HubService>>,
    headers: HeaderMap,
    Json(req): Json<RegisterEdgeRequest>,
) -> Json<EdgeRegistration> {
    if check_version(&headers).is_some() {
        return Json(EdgeRegistration {
            id: "version-mismatch".to_string(),
            addr: "".to_string(),
            last_seen: chrono::Utc::now(),
        });
    }
    if !authorized(&service, &headers, None) {
        return Json(EdgeRegistration {
            id: "unauthorized".to_string(),
            addr: "".to_string(),
            last_seen: chrono::Utc::now(),
        });
    }
    Json(service.register_edge(req.id, req.addr))
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
    let Some(edge) = service.pick_edge() else {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(serde_json::json!({ "error": "no edges registered" })),
        )
            .into_response();
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
    use tower::util::ServiceExt;

    #[tokio::test]
    async fn register_and_list_edges() {
        let tmp = tempfile::tempdir().unwrap();
        let service = Arc::new(HubService::new(HubConfig {
            data_dir: tmp.path().join("hub"),
            edges: Vec::new(),
            tokens: Vec::new(),
        }));
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
        let service = Arc::new(HubService::new(HubConfig {
            data_dir: tmp.path().join("hub"),
            edges: Vec::new(),
            tokens: Vec::new(),
        }));
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
        let service = Arc::new(HubService::new(HubConfig {
            data_dir: tmp.path().join("hub"),
            edges: Vec::new(),
            tokens: Vec::new(),
        }));
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
        let service = Arc::new(HubService::new(HubConfig {
            data_dir: tmp.path().join("hub"),
            edges: Vec::new(),
            tokens: Vec::new(),
        }));
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
        assert!(DASHBOARD_HTML.contains("sessions/ws"));
        assert!(DASHBOARD_HTML.contains("sessions/${id}/attach"));
        assert!(DASHBOARD_HTML.contains("pendingInputs"));
        assert!(DASHBOARD_HTML.contains("detail-logs"));
        assert!(DASHBOARD_HTML.contains("start-session"));
    }

    #[tokio::test]
    async fn routes_basic_health() {
        let tmp = tempfile::tempdir().unwrap();
        let service = Arc::new(HubService::new(HubConfig {
            data_dir: tmp.path().join("hub"),
            edges: Vec::new(),
            tokens: Vec::new(),
        }));
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
        let service = Arc::new(HubService::new(HubConfig {
            data_dir: tmp.path().join("hub"),
            edges: Vec::new(),
            tokens: Vec::new(),
        }));
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
        let service = Arc::new(HubService::new(HubConfig {
            data_dir: tmp.path().join("hub"),
            edges: Vec::new(),
            tokens: Vec::new(),
        }));
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
        let service = Arc::new(HubService::new(HubConfig {
            data_dir: tmp.path().join("hub"),
            edges: Vec::new(),
            tokens: vec!["secret".to_string()],
        }));
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
