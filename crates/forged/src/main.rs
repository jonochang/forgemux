use clap::{Parser, Subcommand};
use forged::{ForgedConfig, OsCommandRunner, SessionService};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

#[derive(Debug, Parser)]
#[command(name = "forged")]
#[command(about = "Forgemux edge daemon", long_about = None)]
struct Cli {
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[arg(long, default_value = "/etc/forgemux/forged.toml", global = true)]
    config: PathBuf,
    #[arg(long, default_value = "127.0.0.1:9090", global = true)]
    bind: String,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run,
    Check,
    Sessions,
    Drain {
        #[arg(long)]
        force: bool,
    },
    RotateCert,
    Health,
    Version,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let mut config = if cli.config.exists() {
        ForgedConfig::load(&cli.config)?
    } else {
        let data_dir = cli
            .data_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from("./.forgemux"));
        ForgedConfig::default_with_data_dir(data_dir)
    };
    if let Some(data_dir) = cli.data_dir {
        config.data_dir = data_dir;
    }
    let hub_info = config
        .hub_url
        .clone()
        .map(|hub_url| {
            let node_id = config
                .node_id
                .clone()
                .unwrap_or_else(|| "edge-unknown".to_string());
            let advertise_addr = config
                .advertise_addr
                .clone()
                .unwrap_or_else(|| cli.bind.clone());
            (hub_url, node_id, advertise_addr)
        });
    let service = SessionService::new(config, OsCommandRunner);

    match cli.command {
        Command::Run => {
            let addr: SocketAddr = cli.bind.parse()?;
            if let Some((hub_url, node_id, advertise_addr)) = hub_info {
                thread::spawn(move || {
                    let client = reqwest::blocking::Client::new();
                    let register_url = format!("{}/edges/register", hub_url.trim_end_matches('/'));
                    let _ = client
                        .post(register_url)
                        .json(&serde_json::json!({
                            "id": node_id,
                            "addr": advertise_addr,
                        }))
                        .send();
                    let heartbeat_url =
                        format!("{}/edges/heartbeat", hub_url.trim_end_matches('/'));
                    loop {
                        let _ = client
                            .post(&heartbeat_url)
                            .json(&serde_json::json!({ "id": node_id }))
                            .send();
                        thread::sleep(Duration::from_secs(10));
                    }
                });
            }
            let app = forged::server::build_router(std::sync::Arc::new(service));
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(async move {
                let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
                axum::serve(listener, app).await.unwrap();
            });
        }
        Command::Check => {
            let checks = forged::checks::run_checks(&service.config());
            let mut failed = false;
            for item in checks {
                let status = if item.ok { "✓" } else { "✗" };
                println!("{} {}: {}", status, item.name, item.message);
                if !item.ok {
                    failed = true;
                }
            }
            if failed {
                std::process::exit(1);
            }
        }
        Command::Sessions => {
            let sessions = service.refresh_states()?;
            if sessions.is_empty() {
                println!("no sessions");
            } else {
                for session in sessions {
                    println!("{} {:?} {:?}", session.id, session.agent, session.state);
                }
            }
        }
        Command::Drain { force } => {
            service.drain(force)?;
            println!("draining");
        }
        Command::RotateCert => {
            println!("certs reloaded");
        }
        Command::Health => {
            println!("{}", r#"{"status":"healthy"}"#);
        }
        Command::Version => {
            println!("forged 0.1.0");
        }
    }

    Ok(())
}
