use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use futures_util::{future::join_all, SinkExt, StreamExt};
use clap::{Parser, Subcommand};
use forgehub::{EdgeRegistration, HubConfig, HubService};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use tower_http::services::ServeDir;

#[derive(Debug, Subcommand)]
enum Command {
    Run,
    Check,
    Sessions,
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
        }
    };
    let service = HubService::new(config);

    match cli.command {
        Command::Run => {
            let addr: SocketAddr = cli.bind.parse()?;
            let shared = Arc::new(service);
            let app = Router::new()
                .route("/health", get(health))
                .route("/sessions", get(list_sessions).post(start_session))
                .route("/sessions/ws", get(ws_sessions))
                .route("/sessions/:id/stop", post(stop_session))
                .route("/sessions/:id/logs", get(session_logs))
                .route("/sessions/:id/input", post(session_input))
                .route("/foreman/report", get(foreman_report))
                .route("/edges", get(list_edges))
                .route("/edges/register", post(register_edge))
                .route("/edges/heartbeat", post(heartbeat))
                .route("/ws", get(ws_handler))
                .route("/sessions/:id/attach", get(ws_attach))
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
        Command::Version => println!("forgehub 0.1.0"),
    }
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "status": "healthy" }))
}

async fn list_sessions(
    State(service): State<Arc<HubService>>,
) -> Json<Vec<forgemux_core::SessionRecord>> {
    Json(fetch_sessions(&service).await)
}

async fn ws_sessions(
    State(service): State<Arc<HubService>>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
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
    for response in join_all(futures).await {
        if let Ok(resp) = response {
            if resp.status().is_success() {
                if let Ok(mut edge_sessions) = resp.json::<Vec<forgemux_core::SessionRecord>>().await
                {
                    sessions.append(&mut edge_sessions);
                }
            }
        }
    }
    forgemux_core::sort_sessions(sessions)
}

async fn list_edges(State(service): State<Arc<HubService>>) -> Json<Vec<EdgeRegistration>> {
    Json(service.list_registered_edges())
}

#[derive(Debug, serde::Deserialize)]
struct RegisterEdgeRequest {
    id: String,
    addr: String,
}

async fn register_edge(
    State(service): State<Arc<HubService>>,
    Json(req): Json<RegisterEdgeRequest>,
) -> Json<EdgeRegistration> {
    Json(service.register_edge(req.id, req.addr))
}

#[derive(Debug, serde::Deserialize)]
struct HeartbeatRequest {
    id: String,
}

async fn heartbeat(
    State(service): State<Arc<HubService>>,
    Json(req): Json<HeartbeatRequest>,
) -> impl IntoResponse {
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
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
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
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let edges = service.list_registered_edges();
    for edge in edges {
        let url = format!(
            "{}/sessions/{}/stop",
            normalize_http_addr(&edge.addr),
            id
        );
        if let Ok(resp) = reqwest::Client::new().post(url).send().await {
            if resp.status().is_success() {
                return (axum::http::StatusCode::OK, Json(serde_json::json!({ "status": "stopped" })))
                    .into_response();
            }
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
    axum::extract::Path(id): axum::extract::Path<String>,
) -> impl IntoResponse {
    let edges = service.list_registered_edges();
    for edge in edges {
        let url = format!(
            "{}/sessions/{}/logs",
            normalize_http_addr(&edge.addr),
            id
        );
        if let Ok(resp) = reqwest::Client::new().get(url).send().await {
            if resp.status().is_success() {
                let body = resp
                    .json::<serde_json::Value>()
                    .await
                    .unwrap_or_else(|_| serde_json::json!({ "error": "invalid response" }));
                return (axum::http::StatusCode::OK, Json(body)).into_response();
            }
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
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let edges = service.list_registered_edges();
    for edge in edges {
        let url = format!(
            "{}/sessions/{}/input",
            normalize_http_addr(&edge.addr),
            id
        );
        if let Ok(resp) = reqwest::Client::new().post(url).json(&payload).send().await {
            if resp.status().is_success() {
                return (axum::http::StatusCode::OK, Json(serde_json::json!({ "status": "sent" })))
                    .into_response();
            }
        }
    }
    (
        axum::http::StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": "session not found" })),
    )
        .into_response()
}

async fn foreman_report(State(service): State<Arc<HubService>>) -> impl IntoResponse {
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
}

async fn ws_attach(
    State(service): State<Arc<HubService>>,
    axum::extract::Path(id): axum::extract::Path<String>,
    Query(query): Query<AttachQuery>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    let ws_url = service
        .resolve_ws_url(query.edge.as_deref())
        .unwrap_or_default();
    ws.on_upgrade(move |socket| relay_ws(ws_url, id, socket))
}

async fn relay_ws(ws_url: String, id: String, mut client: WebSocket) {
    if ws_url.is_empty() {
        let _ = client
            .send(Message::Text("no edge ws_url configured".to_string()))
            .await;
        return;
    }
    let target = format!("{}/sessions/{}/attach", ws_url.trim_end_matches('/'), id);
    let Ok((mut upstream, _)) = tokio_tungstenite::connect_async(target).await else {
        let _ = client
            .send(Message::Text("failed to connect to edge".to_string()))
            .await;
        return;
    };

    loop {
        tokio::select! {
            msg = client.recv() => {
                match msg {
                    Some(Ok(msg)) => {
                        let tungstenite_msg = match msg {
                            Message::Text(text) => tokio_tungstenite::tungstenite::Message::Text(text),
                            Message::Binary(bin) => tokio_tungstenite::tungstenite::Message::Binary(bin),
                            Message::Close(_) => {
                                let _ = upstream.close(None).await;
                                break;
                            }
                            _ => continue,
                        };
                        let _ = upstream.send(tungstenite_msg).await;
                    }
                    _ => break,
                }
            }
            msg = upstream.next() => {
                match msg {
                    Some(Ok(msg)) => {
                        let axum_msg = match msg {
                            tokio_tungstenite::tungstenite::Message::Text(text) => Message::Text(text),
                            tokio_tungstenite::tungstenite::Message::Binary(bin) => Message::Binary(bin),
                            tokio_tungstenite::tungstenite::Message::Close(_) => {
                                let _ = client.close().await;
                                break;
                            }
                            _ => continue,
                        };
                        let _ = client.send(axum_msg).await;
                    }
                    _ => break,
                }
            }
        }
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
            post(|axum::extract::Path(_id): axum::extract::Path<String>| async {
                Json(serde_json::json!({ "status": "sent" }))
            }),
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
}
