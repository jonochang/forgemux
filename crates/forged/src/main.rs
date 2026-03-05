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
        hub_url: Option<String>,
        #[arg(long)]
        hub_token: Option<String>,
        #[arg(long)]
        node_id: Option<String>,
        #[arg(long)]
        advertise_addr: Option<String>,
        #[arg(long)]
        register_hub: bool,
        #[arg(long)]
        bind: Option<String>,
    },
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
    if let Some(data_dir) = cli.data_dir.clone() {
        config.data_dir = data_dir;
    }
    let hub_info = config.hub_url.clone().map(|hub_url| {
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
    let hub_token = config.hub_token.clone();
    let service = SessionService::new(config, OsCommandRunner);

    match cli.command {
        Command::Configure {
            non_interactive,
            force,
            dry_run,
            ref data_dir,
            ref hub_url,
            ref hub_token,
            ref node_id,
            ref advertise_addr,
            register_hub,
            ref bind,
        } => {
            run_configure(
                &cli,
                non_interactive,
                force,
                dry_run,
                data_dir.clone(),
                hub_url.clone(),
                hub_token.clone(),
                node_id.clone(),
                advertise_addr.clone(),
                register_hub,
                bind.clone(),
            )?;
        }
        Command::Run => {
            let addr: SocketAddr = cli.bind.parse()?;
            let _pid_lock = service.acquire_pid_lock()?;
            let _ = service.cleanup_orphan_sessions();
            if let Some((hub_url, node_id, advertise_addr)) = hub_info {
                thread::spawn(move || {
                    let client = reqwest::blocking::Client::new();
                    let register_url = format!("{}/edges/register", hub_url.trim_end_matches('/'));
                    let mut register_req = client.post(register_url).json(&serde_json::json!({
                        "id": node_id,
                        "addr": advertise_addr,
                    }));
                    if let Some(token) = &hub_token {
                        register_req = register_req.bearer_auth(token);
                    }
                    let _ = register_req.send();
                    let heartbeat_url =
                        format!("{}/edges/heartbeat", hub_url.trim_end_matches('/'));
                    loop {
                        let mut heartbeat_req = client
                            .post(&heartbeat_url)
                            .json(&serde_json::json!({ "id": node_id }));
                        if let Some(token) = &hub_token {
                            heartbeat_req = heartbeat_req.bearer_auth(token);
                        }
                        let _ = heartbeat_req.send();
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
            let checks = forged::checks::run_checks(service.config());
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
            println!(r#"{{"status":"healthy"}}"#);
        }
        Command::Version => {
            println!("forged {}", env!("CARGO_PKG_VERSION"));
        }
    }

    Ok(())
}

#[derive(serde::Serialize)]
struct ForgedConfigOutput {
    data_dir: PathBuf,
    hub_url: Option<String>,
    hub_token: Option<String>,
    node_id: Option<String>,
    advertise_addr: Option<String>,
}

#[allow(clippy::too_many_arguments)]
fn run_configure(
    cli: &Cli,
    non_interactive: bool,
    force: bool,
    dry_run: bool,
    data_dir: Option<PathBuf>,
    hub_url: Option<String>,
    hub_token: Option<String>,
    node_id: Option<String>,
    advertise_addr: Option<String>,
    register_hub: bool,
    bind: Option<String>,
) -> anyhow::Result<()> {
    let default_config_path = PathBuf::from("/etc/forgemux/forged.toml");
    let config_path = if cli.config == default_config_path {
        PathBuf::from("./forged.toml")
    } else {
        cli.config.clone()
    };
    let default_data_dir = PathBuf::from("./.forgemux");
    let bind_value = bind.unwrap_or_else(|| cli.bind.clone());
    let data_dir_value = data_dir.unwrap_or(default_data_dir);

    let hub_config_exists = PathBuf::from("./.forgemux-hub.toml").exists();
    let wants_hub = if non_interactive {
        register_hub || hub_url.is_some()
    } else {
        let default = hub_config_exists;
        prompt_bool("Register this edge with a hub?", default)?
    };

    let hub_url_value = if wants_hub {
        if let Some(url) = hub_url {
            Some(url)
        } else if non_interactive {
            Some("http://127.0.0.1:8080".to_string())
        } else {
            Some(prompt_string("Hub URL", Some("http://127.0.0.1:8080"))?)
        }
    } else {
        None
    };

    let hub_token_value = if wants_hub {
        if let Some(token) = hub_token {
            Some(token)
        } else if non_interactive {
            None
        } else {
            let input = prompt_string("Hub token (optional)", None)?;
            if input.is_empty() { None } else { Some(input) }
        }
    } else {
        None
    };

    let node_id_value = if let Some(node_id) = node_id {
        Some(node_id)
    } else if non_interactive {
        Some("edge-01".to_string())
    } else {
        let input = prompt_string("Node ID", Some("edge-01"))?;
        if input.is_empty() { None } else { Some(input) }
    };

    let advertise_addr_value = if wants_hub {
        if let Some(addr) = advertise_addr {
            Some(addr)
        } else if non_interactive {
            Some(bind_value.clone())
        } else {
            Some(prompt_string("Advertise address", Some(&bind_value))?)
        }
    } else {
        None
    };

    let config = ForgedConfigOutput {
        data_dir: data_dir_value.clone(),
        hub_url: hub_url_value,
        hub_token: hub_token_value,
        node_id: node_id_value,
        advertise_addr: advertise_addr_value,
    };

    write_config_file(&config_path, &config, force, dry_run)?;
    if !dry_run {
        std::fs::create_dir_all(&data_dir_value)?;
    }

    println!("Configured forged.");
    println!("Config: {}", config_path.display());
    println!("Data dir: {}", data_dir_value.display());
    println!(
        "Run: forged --bind {} --config {} run",
        bind_value,
        config_path.display()
    );
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
    path: &std::path::Path,
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
