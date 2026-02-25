use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    response::IntoResponse,
    routing::get,
    Json, Router,
};
use futures_util::{SinkExt, StreamExt};
use clap::{Parser, Subcommand};
use forgehub::{HubConfig, HubService};
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
                .route("/sessions", get(list_sessions))
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
    let sessions = service.list_sessions().unwrap_or_default();
    Json(sessions)
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
