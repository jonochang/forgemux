use axum::{extract::State, routing::get, Json, Router};
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
                .fallback_service(ServeDir::new("dashboard"))
                .with_state(shared);
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async move {
                axum::Server::bind(&addr)
                    .serve(app.into_make_service())
                    .await
                    .unwrap();
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
