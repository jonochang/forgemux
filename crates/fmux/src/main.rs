use clap::{Parser, Subcommand};
use forged::{ForgedConfig, OsCommandRunner, SessionService};
use forgemux_core::{AgentType, SessionState, sort_sessions};
use serde::Deserialize;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;
use uuid::Uuid;

const FMUX_VERSION_HEADER: &str = "x-forgemux-version";

#[derive(Debug, Parser)]
#[command(name = "fmux")]
#[command(about = "Forgemux CLI", long_about = None)]
struct Cli {
    #[arg(long, default_value = "./.forgemux")]
    data_dir: PathBuf,
    #[arg(long, default_value = "~/.config/forgemux/config.toml")]
    config: String,
    #[arg(long, default_value = "./.forgemux-hub.toml")]
    hub_config: PathBuf,
    #[arg(long)]
    edge: Option<String>,
    #[arg(long)]
    hub: Option<String>,
    #[arg(long)]
    token: Option<String>,
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
    },
    #[command(alias = "s")]
    Start {
        #[arg(long, default_value = "claude")]
        agent: String,
        #[arg(long, default_value = "sonnet")]
        model: String,
        #[arg(long)]
        repo: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        notify: Vec<String>,
        #[arg(long)]
        policy: Option<String>,
        #[arg(long, short = 'w')]
        worktree: bool,
        #[arg(long, short = 'b')]
        branch: Option<String>,
        #[arg(long)]
        worktree_path: Option<String>,
    },
    Attach {
        #[arg(long, short = 'n')]
        name: Option<String>,
        session_id: Option<String>,
    },
    Detach {
        session_id: String,
    },
    Stop {
        session_id: String,
    },
    Kill {
        session_id: String,
    },
    Import {
        tmux_session: String,
        #[arg(long, default_value = "claude")]
        agent: String,
        #[arg(long, default_value = "sonnet")]
        model: String,
        #[arg(long)]
        repo: Option<String>,
        #[arg(long)]
        name: Option<String>,
        #[arg(long)]
        no_attach: bool,
    },
    Ls,
    Status {
        session_id: String,
    },
    Logs {
        session_id: String,
        #[arg(long, default_value_t = 100)]
        tail: usize,
        #[arg(long)]
        follow: bool,
    },
    Watch {
        #[arg(long, default_value_t = 5)]
        interval: u64,
    },
    Edges,
    ForemanStart {
        #[arg(long, default_value = "claude")]
        agent: String,
        #[arg(long, default_value = "sonnet")]
        model: String,
        #[arg(long, default_value = ".")]
        repo: String,
        #[arg(long, default_value = "all")]
        watch: String,
        #[arg(long, default_value = "advisory")]
        intervention: String,
    },
    ForemanStatus,
    ForemanReport,
    Inject {
        session_id: String,
        input: String,
    },
    Usage {
        session_id: String,
    },
    Pair {
        #[arg(long, default_value_t = 600)]
        ttl_secs: i64,
        #[arg(long)]
        qr: bool,
        #[arg(long)]
        key: Option<String>,
    },
    Doctor,
    Version,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let cli_config = load_cli_config(&cli.config).unwrap_or_default();
    let edge_addr = resolve_edge(cli.edge.as_deref(), cli.hub.as_deref(), &cli_config);
    let token = resolve_token(cli.token.as_deref(), &cli_config);
    let config = ForgedConfig::default_with_data_dir(cli.data_dir.clone());
    let service = SessionService::new(config, OsCommandRunner);

    match cli.command {
        Command::Configure {
            non_interactive,
            force,
            dry_run,
        } => {
            if let Err(err) = run_configure(&cli, non_interactive, force, dry_run) {
                eprintln!("configure failed: {err}");
                std::process::exit(1);
            }
        }
        Command::Start {
            agent,
            model,
            repo,
            name,
            notify,
            policy,
            worktree,
            branch,
            worktree_path,
        } => {
            let agent = match agent.as_str() {
                "claude" => AgentType::Claude,
                "codex" => AgentType::Codex,
                other => {
                    eprintln!("unknown agent: {other}");
                    std::process::exit(2);
                }
            };
            let worktree_spec = if worktree {
                let Some(branch) = branch else {
                    eprintln!("--branch is required with --worktree");
                    std::process::exit(2);
                };
                Some(forged::WorktreeSpec {
                    branch,
                    path: worktree_path.map(std::path::PathBuf::from),
                })
            } else {
                None
            };

            let request = forged::server::StartRequest {
                agent: match agent {
                    AgentType::Claude => "claude".to_string(),
                    AgentType::Codex => "codex".to_string(),
                },
                model,
                repo: repo.unwrap_or_default(),
                name,
                worktree: worktree_spec.is_some(),
                branch: worktree_spec.as_ref().map(|spec| spec.branch.clone()),
                worktree_path: worktree_spec
                    .as_ref()
                    .and_then(|spec| spec.path.as_ref().map(|p| p.to_string_lossy().to_string())),
                notify: if notify.is_empty() {
                    None
                } else {
                    Some(notify)
                },
                policy,
            };

            let client = reqwest::blocking::Client::new();
            let url = format!("{}/sessions/start", edge_addr.trim_end_matches('/'));
            let req = apply_headers(client.post(url).json(&request), token.as_deref());
            let response = req.send();
            match response {
                Ok(resp) if resp.status().is_success() => {
                    let body = resp.json::<forged::server::StartResponse>().unwrap();
                    println!("{}", body.session_id);
                    if let Err(err) = service.attach_session(&body.session_id) {
                        eprintln!("attach failed: {err}");
                        std::process::exit(1);
                    }
                }
                Ok(resp) => {
                    let body = resp.json::<forged::server::ErrorResponse>().unwrap_or(
                        forged::server::ErrorResponse {
                            error: "unknown error".to_string(),
                        },
                    );
                    eprintln!("start failed: {}", body.error);
                    std::process::exit(1);
                }
                Err(err) => {
                    eprintln!("start failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        Command::Attach { name, session_id } => {
            if name.is_some() && session_id.is_some() {
                eprintln!("attach supports either --name or session_id, not both");
                std::process::exit(2);
            }
            let query = if let Some(prefix) = name {
                prefix
            } else if let Some(id) = session_id {
                id
            } else {
                eprintln!("attach requires a session_id or --name");
                std::process::exit(2);
            };
            if query.trim().is_empty() {
                eprintln!("attach requires a non-empty query");
                std::process::exit(2);
            }
            let session_id = {
                let client = reqwest::blocking::Client::new();
                let url = format!("{}/sessions", edge_addr.trim_end_matches('/'));
                let req = apply_headers(client.get(url), token.as_deref());
                let response = req.send();
                match response {
                    Ok(resp) if resp.status().is_success() => {
                        let sessions: Vec<forgemux_core::SessionRecord> = resp.json().unwrap();
                        match resolve_session_id_by_query(&sessions, &query) {
                            Ok(id) => id,
                            Err(err) => {
                                eprintln!("attach failed: {err}");
                                std::process::exit(1);
                            }
                        }
                    }
                    Ok(resp) => {
                        let body = resp.json::<forged::server::ErrorResponse>().unwrap_or(
                            forged::server::ErrorResponse {
                                error: "unknown error".to_string(),
                            },
                        );
                        eprintln!("attach failed: {}", body.error);
                        std::process::exit(1);
                    }
                    Err(err) => {
                        eprintln!("attach failed: {err}");
                        std::process::exit(1);
                    }
                }
            };
            if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
                eprintln!(
                    "attach requires a TTY. Try: `ssh -t <host>` or `tmux attach -t {}`",
                    session_id
                );
                std::process::exit(1);
            }
            if let Err(err) = service.attach_session(&session_id) {
                eprintln!("attach failed: {err}");
                std::process::exit(1);
            }
        }
        Command::Detach { session_id } => {
            if let Err(err) = service.detach_session(&session_id) {
                eprintln!("detach failed: {err}");
                std::process::exit(1);
            }
        }
        Command::Stop { session_id } => {
            let client = reqwest::blocking::Client::new();
            let url = format!(
                "{}/sessions/{}/stop",
                edge_addr.trim_end_matches('/'),
                session_id
            );
            let req = apply_headers(client.post(url), token.as_deref());
            let response = req.send();
            match response {
                Ok(resp) if resp.status().is_success() => {}
                Ok(resp) => {
                    let body = resp.json::<forged::server::ErrorResponse>().unwrap_or(
                        forged::server::ErrorResponse {
                            error: "unknown error".to_string(),
                        },
                    );
                    eprintln!("stop failed: {}", body.error);
                    std::process::exit(1);
                }
                Err(err) => {
                    eprintln!("stop failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        Command::Kill { session_id } => {
            let client = reqwest::blocking::Client::new();
            let url = format!(
                "{}/sessions/{}/kill",
                edge_addr.trim_end_matches('/'),
                session_id
            );
            let req = apply_headers(client.post(url), token.as_deref());
            let response = req.send();
            match response {
                Ok(resp) if resp.status().is_success() => {}
                Ok(resp) => {
                    let body = resp.json::<forged::server::ErrorResponse>().unwrap_or(
                        forged::server::ErrorResponse {
                            error: "unknown error".to_string(),
                        },
                    );
                    eprintln!("kill failed: {}", body.error);
                    std::process::exit(1);
                }
                Err(err) => {
                    eprintln!("kill failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        Command::Import {
            tmux_session,
            agent,
            model,
            repo,
            name,
            no_attach,
        } => {
            let agent = match agent.as_str() {
                "claude" => AgentType::Claude,
                "codex" => AgentType::Codex,
                other => {
                    eprintln!("unknown agent: {other}");
                    std::process::exit(2);
                }
            };
            let request = forged::server::ImportRequest {
                tmux_session,
                agent: match agent {
                    AgentType::Claude => "claude".to_string(),
                    AgentType::Codex => "codex".to_string(),
                },
                model,
                repo,
                name,
            };
            let client = reqwest::blocking::Client::new();
            let url = format!("{}/sessions/import", edge_addr.trim_end_matches('/'));
            let req = apply_headers(client.post(url).json(&request), token.as_deref());
            let response = req.send();
            match response {
                Ok(resp) if resp.status().is_success() => {
                    let body = resp.json::<forged::server::StartResponse>().unwrap();
                    println!("{}", body.session_id);
                    if !no_attach && let Err(err) = service.attach_session(&body.session_id) {
                        eprintln!("attach failed: {err}");
                        std::process::exit(1);
                    }
                }
                Ok(resp) => {
                    let body = resp.json::<forged::server::ErrorResponse>().unwrap_or(
                        forged::server::ErrorResponse {
                            error: "unknown error".to_string(),
                        },
                    );
                    eprintln!("import failed: {}", body.error);
                    std::process::exit(1);
                }
                Err(err) => {
                    eprintln!("import failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        Command::Ls => {
            let client = reqwest::blocking::Client::new();
            let url = format!("{}/sessions", edge_addr.trim_end_matches('/'));
            let req = apply_headers(client.get(url), token.as_deref());
            let response = req.send();
            match response {
                Ok(resp) if resp.status().is_success() => {
                    let sessions: Vec<forgemux_core::SessionRecord> = resp.json().unwrap();
                    print_sessions(sessions);
                }
                Ok(resp) => {
                    let body = resp.json::<forged::server::ErrorResponse>().unwrap_or(
                        forged::server::ErrorResponse {
                            error: "unknown error".to_string(),
                        },
                    );
                    eprintln!("ls failed: {}", body.error);
                    std::process::exit(1);
                }
                Err(err) => {
                    eprintln!("ls failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        Command::Status { session_id } => {
            let client = reqwest::blocking::Client::new();
            let url = format!("{}/sessions", edge_addr.trim_end_matches('/'));
            let req = apply_headers(client.get(url), token.as_deref());
            let response = req.send();
            match response {
                Ok(resp) if resp.status().is_success() => {
                    let sessions: Vec<forgemux_core::SessionRecord> = resp.json().unwrap();
                    let sessions = sort_sessions(sessions);
                    match sessions.into_iter().find(|s| s.id.as_str() == session_id) {
                        Some(session) => {
                            println!("ID: {}", session.id);
                            println!("Agent: {:?}", session.agent);
                            println!("Model: {}", session.model);
                            println!("State: {:?}", session.state);
                            println!("Repo: {}", session.repo_root.display());
                            println!("Last activity: {}", session.last_activity_at);
                        }
                        None => {
                            eprintln!("session not found");
                            std::process::exit(1);
                        }
                    }
                }
                Ok(resp) => {
                    let body = resp.json::<forged::server::ErrorResponse>().unwrap_or(
                        forged::server::ErrorResponse {
                            error: "unknown error".to_string(),
                        },
                    );
                    eprintln!("status failed: {}", body.error);
                    std::process::exit(1);
                }
                Err(err) => {
                    eprintln!("status failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        Command::Logs {
            session_id,
            tail: _tail,
            follow,
        } => {
            if !follow {
                let client = reqwest::blocking::Client::new();
                let url = format!(
                    "{}/sessions/{}/logs",
                    edge_addr.trim_end_matches('/'),
                    session_id
                );
                let req = apply_headers(client.get(url), token.as_deref());
                let response = req.send();
                match response {
                    Ok(resp) if resp.status().is_success() => {
                        let payload: JsonValue = resp.json().unwrap();
                        if let Some(content) = payload.get("content").and_then(|v| v.as_str()) {
                            println!("{content}");
                        }
                    }
                    Ok(resp) => {
                        let body = resp.json::<forged::server::ErrorResponse>().unwrap_or(
                            forged::server::ErrorResponse {
                                error: "unknown error".to_string(),
                            },
                        );
                        eprintln!("logs failed: {}", body.error);
                        std::process::exit(1);
                    }
                    Err(err) => {
                        eprintln!("logs failed: {err}");
                        std::process::exit(1);
                    }
                }
            } else {
                let mut last_content = String::new();
                loop {
                    let client = reqwest::blocking::Client::new();
                    let url = format!(
                        "{}/sessions/{}/logs",
                        edge_addr.trim_end_matches('/'),
                        session_id
                    );
                    let req = apply_headers(client.get(url), token.as_deref());
                    let response = req.send();
                    match response {
                        Ok(resp) if resp.status().is_success() => {
                            let payload: JsonValue = resp.json().unwrap();
                            let content = payload
                                .get("content")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            if content.starts_with(&last_content) {
                                let delta = &content[last_content.len()..];
                                if !delta.is_empty() {
                                    println!("{delta}");
                                }
                            } else if content != last_content {
                                println!("{content}");
                            }
                            last_content = content;
                        }
                        Ok(resp) => {
                            let body = resp.json::<forged::server::ErrorResponse>().unwrap_or(
                                forged::server::ErrorResponse {
                                    error: "unknown error".to_string(),
                                },
                            );
                            eprintln!("logs failed: {}", body.error);
                            std::process::exit(1);
                        }
                        Err(err) => {
                            eprintln!("logs failed: {err}");
                            std::process::exit(1);
                        }
                    }
                    thread::sleep(Duration::from_secs(2));
                }
            }
        }
        Command::Watch { interval } => loop {
            let client = reqwest::blocking::Client::new();
            let url = format!("{}/sessions", edge_addr.trim_end_matches('/'));
            let req = apply_headers(client.get(url), token.as_deref());
            let response = req.send();
            match response {
                Ok(resp) if resp.status().is_success() => {
                    let sessions: Vec<forgemux_core::SessionRecord> = resp.json().unwrap();
                    print_sessions(sessions);
                }
                Ok(resp) => {
                    let body = resp.json::<forged::server::ErrorResponse>().unwrap_or(
                        forged::server::ErrorResponse {
                            error: "unknown error".to_string(),
                        },
                    );
                    eprintln!("watch failed: {}", body.error);
                    std::process::exit(1);
                }
                Err(err) => {
                    eprintln!("watch failed: {err}");
                    std::process::exit(1);
                }
            }
            thread::sleep(Duration::from_secs(interval));
        },
        Command::Edges => {
            let Some(hub_url) = resolve_hub(cli.hub.as_deref(), &cli_config) else {
                eprintln!("edges failed: no hub configured");
                std::process::exit(1);
            };
            let client = reqwest::blocking::Client::new();
            let url = format!("{}/edges", hub_url.trim_end_matches('/'));
            let req = apply_headers(client.get(url), token.as_deref());
            let response = req.send();
            match response {
                Ok(resp) if resp.status().is_success() => {
                    let edges: Vec<forgehub::EdgeRegistration> = resp.json().unwrap();
                    if edges.is_empty() {
                        println!("no edges");
                    } else {
                        for edge in edges {
                            println!("{} {}", edge.id, edge.addr);
                        }
                    }
                }
                Ok(resp) => {
                    eprintln!("edges failed: {}", resp.status());
                    std::process::exit(1);
                }
                Err(err) => {
                    eprintln!("edges failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        Command::ForemanStart {
            agent,
            model,
            repo,
            watch,
            intervention,
        } => {
            let agent = match agent.as_str() {
                "claude" => AgentType::Claude,
                "codex" => AgentType::Codex,
                other => {
                    eprintln!("unknown agent: {other}");
                    std::process::exit(2);
                }
            };
            let intervention = match intervention.as_str() {
                "advisory" => forgemux_core::InterventionLevel::Advisory,
                "assisted" => forgemux_core::InterventionLevel::Assisted,
                "autonomous" => forgemux_core::InterventionLevel::Autonomous,
                other => {
                    eprintln!("unknown intervention: {other}");
                    std::process::exit(2);
                }
            };
            let watch_scope = if watch == "all" {
                Vec::new()
            } else {
                watch.split(',').map(|s| s.trim().to_string()).collect()
            };
            match service.start_foreman(agent, model, repo, watch_scope, intervention) {
                Ok(record) => println!("{}", record.id),
                Err(err) => {
                    eprintln!("foreman start failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        Command::ForemanStatus => match service.list_sessions() {
            Ok(sessions) => {
                let foremen: Vec<_> = sessions
                    .into_iter()
                    .filter(|s| matches!(s.role, forgemux_core::SessionRole::Foreman { .. }))
                    .collect();
                if foremen.is_empty() {
                    println!("no foreman sessions");
                } else {
                    for session in foremen {
                        println!("{} {:?} {:?}", session.id, session.agent, session.state);
                    }
                }
            }
            Err(err) => {
                eprintln!("foreman status failed: {err}");
                std::process::exit(1);
            }
        },
        Command::ForemanReport => {
            let client = reqwest::blocking::Client::new();
            let url = format!("{}/foreman/report", edge_addr.trim_end_matches('/'));
            let req = apply_headers(client.get(url), token.as_deref());
            let response = req.send();
            match response {
                Ok(resp) if resp.status().is_success() => {
                    let report: serde_json::Value = resp.json().unwrap();
                    println!("{}", serde_json::to_string_pretty(&report).unwrap());
                }
                Ok(resp) => {
                    eprintln!("foreman report failed: {}", resp.status());
                    std::process::exit(1);
                }
                Err(err) => {
                    eprintln!("foreman report failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        Command::Inject { session_id, input } => {
            let client = reqwest::blocking::Client::new();
            let url = format!(
                "{}/sessions/{}/input",
                edge_addr.trim_end_matches('/'),
                session_id
            );
            let req = apply_headers(
                client
                    .post(url)
                    .json(&serde_json::json!({ "input": input })),
                token.as_deref(),
            );
            let response = req.send();
            match response {
                Ok(resp) if resp.status().is_success() => {}
                Ok(resp) => {
                    eprintln!("inject failed: {}", resp.status());
                    std::process::exit(1);
                }
                Err(err) => {
                    eprintln!("inject failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        Command::Usage { session_id } => {
            let client = reqwest::blocking::Client::new();
            let url = format!(
                "{}/sessions/{}/usage",
                edge_addr.trim_end_matches('/'),
                session_id
            );
            let req = apply_headers(client.get(url), token.as_deref());
            let response = req.send();
            match response {
                Ok(resp) if resp.status().is_success() => {
                    let usage: serde_json::Value = resp.json().unwrap();
                    println!("{}", serde_json::to_string_pretty(&usage).unwrap());
                }
                Ok(resp) => {
                    eprintln!("usage failed: {}", resp.status());
                    std::process::exit(1);
                }
                Err(err) => {
                    eprintln!("usage failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        Command::Pair { ttl_secs, qr, key } => {
            let Some(hub_url) = resolve_hub(cli.hub.as_deref(), &cli_config) else {
                eprintln!("pair requires a hub url (set hub_url in config or pass --hub)");
                std::process::exit(1);
            };
            let client = reqwest::blocking::Client::new();
            let url = format!("{}/pairing/start", hub_url.trim_end_matches('/'));
            let req = apply_headers(
                client
                    .post(url)
                    .json(&serde_json::json!({ "ttl_secs": ttl_secs })),
                token.as_deref(),
            );
            let response = req.send();
            match response {
                Ok(resp) if resp.status().is_success() => {
                    let body: serde_json::Value = resp.json().unwrap();
                    let mut url = body
                        .get("url")
                        .and_then(|value| value.as_str())
                        .unwrap_or("")
                        .to_string();
                    if let Some(key) = key {
                        let separator = if url.contains('?') { "&" } else { "?" };
                        url = format!("{url}{separator}key={}", urlencoding::encode(&key));
                    }
                    if url.is_empty() {
                        eprintln!("pair failed: missing url");
                        std::process::exit(1);
                    }
                    println!("{url}");
                    if qr {
                        println!();
                        println!("{}", render_qr(&url));
                    }
                }
                Ok(resp) => {
                    eprintln!("pair failed: {}", resp.status());
                    std::process::exit(1);
                }
                Err(err) => {
                    eprintln!("pair failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        Command::Doctor => {
            let config_path = expand_tilde(&cli.config);
            if let Err(err) = load_cli_config(&cli.config) {
                eprintln!("config: error ({})", err);
                std::process::exit(1);
            }
            let report = doctor_report(
                &config_path,
                resolve_hub(cli.hub.as_deref(), &cli_config),
                resolve_edge(cli.edge.as_deref(), cli.hub.as_deref(), &cli_config),
                resolve_token(cli.token.as_deref(), &cli_config),
                check_health,
            );
            for line in report.lines() {
                println!("{line}");
            }
            if !report.ok() {
                std::process::exit(1);
            }
        }
        Command::Version => {
            println!("fmux {}", env!("CARGO_PKG_VERSION"));
        }
    }
}

fn run_configure(
    cli: &Cli,
    non_interactive: bool,
    force: bool,
    dry_run: bool,
) -> anyhow::Result<()> {
    let hub_config_path = cli.hub_config.clone();
    let hub_data_dir_default = PathBuf::from("./.forgemux-hub");
    let hub_bind_default = "127.0.0.1:8080".to_string();

    let use_tokens = if non_interactive {
        false
    } else {
        prompt_bool("Enable hub auth tokens?", false)?
    };
    let token_value = if use_tokens {
        let input = prompt_string("Token (leave blank to generate)", None)?;
        if input.is_empty() {
            Uuid::new_v4().simple().to_string()
        } else {
            input
        }
    } else {
        String::new()
    };
    let use_shared_fs = if non_interactive {
        false
    } else {
        prompt_bool(
            "Hub shares filesystem with edge for session listing?",
            false,
        )?
    };

    let hub_data_dir = if non_interactive {
        hub_data_dir_default.clone()
    } else {
        PathBuf::from(prompt_string(
            "Hub data_dir",
            Some(hub_data_dir_default.to_string_lossy().as_ref()),
        )?)
    };
    let hub_bind = if non_interactive {
        hub_bind_default.clone()
    } else {
        prompt_string("Hub bind", Some(&hub_bind_default))?
    };

    let mut hub_args = vec![
        "configure".to_string(),
        "--non-interactive".to_string(),
        format!("--data-dir={}", hub_data_dir.display()),
        format!("--bind={}", hub_bind),
    ];
    if force {
        hub_args.push("--force".to_string());
    }
    if dry_run {
        hub_args.push("--dry-run".to_string());
    }
    if use_tokens {
        hub_args.push("--enable-tokens".to_string());
        hub_args.push(format!("--token={}", token_value));
    }
    if use_shared_fs {
        let edge_id = if non_interactive {
            "edge-01".to_string()
        } else {
            prompt_string("Edge ID", Some("edge-01"))?
        };
        let edge_data_dir = if non_interactive {
            "./.forgemux".to_string()
        } else {
            prompt_string("Edge data_dir", Some("./.forgemux"))?
        };
        let edge_ws_url = if non_interactive {
            "".to_string()
        } else {
            prompt_string("Edge ws_url (optional)", None)?
        };
        hub_args.push("--shared-fs".to_string());
        hub_args.push(format!("--edge-id={}", edge_id));
        hub_args.push(format!("--edge-data-dir={}", edge_data_dir));
        if !edge_ws_url.is_empty() {
            hub_args.push(format!("--edge-ws-url={}", edge_ws_url));
        }
    }

    let status = std::process::Command::new("forgehub")
        .arg(format!("--config={}", hub_config_path.display()))
        .args(&hub_args)
        .status()?;
    if !status.success() {
        anyhow::bail!("forgehub configure failed");
    }

    let edge_config_path = PathBuf::from("./forged.toml");
    let edge_data_dir_default = PathBuf::from("./.forgemux");
    let edge_bind_default = "127.0.0.1:9090".to_string();
    let edge_register_hub = if non_interactive {
        false
    } else {
        prompt_bool("Register this edge with the hub?", true)?
    };
    let edge_data_dir = if non_interactive {
        edge_data_dir_default.clone()
    } else {
        PathBuf::from(prompt_string(
            "Edge data_dir",
            Some(edge_data_dir_default.to_string_lossy().as_ref()),
        )?)
    };
    let edge_bind = if non_interactive {
        edge_bind_default.clone()
    } else {
        prompt_string("Edge bind", Some(&edge_bind_default))?
    };
    let edge_hub_url = if edge_register_hub {
        if non_interactive {
            "http://127.0.0.1:8080".to_string()
        } else {
            prompt_string("Hub URL", Some("http://127.0.0.1:8080"))?
        }
    } else {
        String::new()
    };
    let edge_node_id = if non_interactive {
        "edge-01".to_string()
    } else {
        prompt_string("Node ID", Some("edge-01"))?
    };
    let edge_advertise = if edge_register_hub {
        if non_interactive {
            edge_bind.clone()
        } else {
            prompt_string("Advertise address", Some(&edge_bind))?
        }
    } else {
        String::new()
    };

    let mut edge_args = vec![
        "configure".to_string(),
        "--non-interactive".to_string(),
        format!("--data-dir={}", edge_data_dir.display()),
        format!("--bind={}", edge_bind),
    ];
    if force {
        edge_args.push("--force".to_string());
    }
    if dry_run {
        edge_args.push("--dry-run".to_string());
    }
    if edge_register_hub {
        edge_args.push("--register-hub".to_string());
        edge_args.push(format!("--hub-url={}", edge_hub_url));
        if use_tokens && !token_value.is_empty() {
            edge_args.push(format!("--hub-token={}", token_value));
        }
        edge_args.push(format!("--node-id={}", edge_node_id));
        edge_args.push(format!("--advertise-addr={}", edge_advertise));
    }

    let status = std::process::Command::new("forged")
        .arg(format!("--config={}", edge_config_path.display()))
        .args(&edge_args)
        .status()?;
    if !status.success() {
        anyhow::bail!("forged configure failed");
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

fn print_sessions(sessions: Vec<forgemux_core::SessionRecord>) {
    if sessions.is_empty() {
        println!("no sessions");
        return;
    }
    println!(
        "{:<20} {:<10} {:<8} {:<16} STATE",
        "NAME", "ID", "AGENT", "MODEL"
    );
    for session in sessions {
        let name = truncate_name(&session_display_name(&session), 20);
        let agent = match session.agent {
            AgentType::Claude => "Claude",
            AgentType::Codex => "Codex",
        };
        let state = match session.state {
            SessionState::WaitingInput => "waiting",
            SessionState::Running => "running",
            SessionState::Idle => "idle",
            SessionState::Errored => "errored",
            SessionState::Unreachable => "unreachable",
            SessionState::Terminated => "terminated",
            SessionState::Provisioning => "provisioning",
            SessionState::Starting => "starting",
        };
        println!(
            "{:<20} {:<10} {:<8} {:<16} {}",
            name, session.id, agent, session.model, state
        );
    }
}

fn session_display_name(session: &forgemux_core::SessionRecord) -> String {
    if let Some(name) = session.name.as_ref().filter(|name| !name.trim().is_empty()) {
        return name.trim().to_string();
    }
    session
        .repo_root
        .file_name()
        .and_then(|part| part.to_str())
        .filter(|part| !part.trim().is_empty())
        .unwrap_or("session")
        .to_string()
}

fn truncate_name(name: &str, max_len: usize) -> String {
    name.chars().take(max_len).collect()
}

fn resolve_session_id_by_query(
    sessions: &[forgemux_core::SessionRecord],
    query: &str,
) -> Result<String, String> {
    let mut name_matches = Vec::new();
    for session in sessions {
        let name = session_display_name(session);
        if name.starts_with(query) {
            name_matches.push((session.id.as_str().to_string(), name));
        }
    }
    if name_matches.len() == 1 {
        return Ok(name_matches[0].0.clone());
    }
    if name_matches.len() > 1 {
        let summary = name_matches
            .iter()
            .map(|(id, name)| format!("{} ({})", id, name))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "multiple session names match \"{}\": {}",
            query, summary
        ));
    }

    let mut id_matches = Vec::new();
    for session in sessions {
        let id = session.id.as_str().to_string();
        if id.starts_with(query) {
            id_matches.push((id, session_display_name(session)));
        }
    }
    if id_matches.len() == 1 {
        return Ok(id_matches[0].0.clone());
    }
    if id_matches.len() > 1 {
        let summary = id_matches
            .iter()
            .map(|(id, name)| format!("{} ({})", id, name))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "multiple session IDs match \"{}\": {}",
            query, summary
        ));
    }
    Err(format!("no session name or ID starts with \"{}\"", query))
}

#[derive(Debug, Default, Deserialize)]
struct CliConfig {
    #[allow(dead_code)]
    pub hub_url: Option<String>,
    pub token: Option<String>,
    #[serde(default)]
    pub edges: HashMap<String, String>,
}

fn load_cli_config(path: &str) -> anyhow::Result<CliConfig> {
    let path = expand_tilde(path);
    if !path.exists() {
        return Ok(CliConfig::default());
    }
    let data = std::fs::read_to_string(path)?;
    let config: CliConfig = toml::from_str(&data)?;
    Ok(config)
}

fn resolve_edge(edge: Option<&str>, hub: Option<&str>, config: &CliConfig) -> String {
    if let Some(edge) = edge {
        return config
            .edges
            .get(edge)
            .cloned()
            .unwrap_or_else(|| edge.to_string());
    }
    if let Some(hub) = hub {
        return hub.to_string();
    }
    if let Some(hub_url) = &config.hub_url {
        return hub_url.clone();
    }
    "http://127.0.0.1:9090".to_string()
}

fn resolve_hub(hub: Option<&str>, config: &CliConfig) -> Option<String> {
    if let Some(hub) = hub {
        return Some(hub.to_string());
    }
    config.hub_url.clone()
}

fn resolve_token(token: Option<&str>, config: &CliConfig) -> Option<String> {
    if let Some(token) = token {
        return Some(token.to_string());
    }
    config.token.clone()
}

fn apply_headers(
    mut req: reqwest::blocking::RequestBuilder,
    token: Option<&str>,
) -> reqwest::blocking::RequestBuilder {
    req = req.header(FMUX_VERSION_HEADER, env!("CARGO_PKG_VERSION"));
    if let Some(token) = token {
        req = req.bearer_auth(token);
    }
    req
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/")
        && let Ok(home) = std::env::var("HOME")
    {
        return PathBuf::from(home).join(stripped);
    }
    PathBuf::from(path)
}

struct DoctorReport {
    config_present: bool,
    hub_url: Option<String>,
    hub_ok: Option<bool>,
    edge_url: String,
    edge_ok: bool,
    token_present: bool,
}

impl DoctorReport {
    fn ok(&self) -> bool {
        let hub_ok = self.hub_ok.unwrap_or(true);
        self.edge_ok && hub_ok
    }

    fn lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let config_status = if self.config_present { "ok" } else { "missing" };
        lines.push(format!("config: {config_status}"));
        if let Some(hub_url) = &self.hub_url {
            let status = if self.hub_ok.unwrap_or(false) {
                "ok"
            } else {
                "unreachable"
            };
            lines.push(format!("hub: {status} ({hub_url})"));
        } else {
            lines.push("hub: not configured".to_string());
        }
        let edge_status = if self.edge_ok { "ok" } else { "unreachable" };
        lines.push(format!("edge: {edge_status} ({})", self.edge_url));
        let token_status = if self.token_present {
            "present"
        } else {
            "missing"
        };
        lines.push(format!("token: {token_status}"));
        lines
    }
}

fn doctor_report(
    config_path: &Path,
    hub_url: Option<String>,
    edge_url: String,
    token: Option<String>,
    health_check: impl Fn(&str, Option<&str>) -> bool,
) -> DoctorReport {
    let hub_ok = hub_url
        .as_deref()
        .map(|url| health_check(url, token.as_deref()));
    let edge_ok = health_check(&edge_url, token.as_deref());
    DoctorReport {
        config_present: config_path.exists(),
        hub_url,
        hub_ok,
        edge_url,
        edge_ok,
        token_present: token.is_some(),
    }
}

fn check_health(url: &str, token: Option<&str>) -> bool {
    let client = reqwest::blocking::Client::new();
    let url = format!("{}/health", url.trim_end_matches('/'));
    let req = apply_headers(client.get(url), token);
    req.send()
        .map(|resp| resp.status().is_success())
        .unwrap_or(false)
}

fn render_qr(data: &str) -> String {
    let qr = qrcodegen::QrCode::encode_text(data, qrcodegen::QrCodeEcc::Low).unwrap();
    let mut out = String::new();
    let border = 2;
    for y in -border..qr.size() + border {
        for x in -border..qr.size() + border {
            out.push(if qr.get_module(x, y) { '█' } else { ' ' });
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_cli_config_resolves_edge_aliases() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("config.toml");
        let contents = r#"
hub_url = "https://hub.example"

[edges]
mel-01 = "edge-mel-01.tailnet:9443"
"#;
        std::fs::write(&path, contents).unwrap();

        let config = load_cli_config(path.to_string_lossy().as_ref()).unwrap();
        assert_eq!(
            resolve_edge(Some("mel-01"), None, &config),
            "edge-mel-01.tailnet:9443"
        );
        assert_eq!(
            resolve_edge(Some("http://127.0.0.1:9090"), None, &config),
            "http://127.0.0.1:9090"
        );
        assert_eq!(resolve_edge(None, None, &config), "https://hub.example");
    }

    #[test]
    fn resolve_edge_prefers_explicit_hub() {
        let config = CliConfig {
            hub_url: Some("https://hub.local".to_string()),
            token: None,
            edges: HashMap::new(),
        };
        assert_eq!(
            resolve_edge(None, Some("http://override:8080"), &config),
            "http://override:8080"
        );
    }

    #[test]
    fn resolve_hub_prefers_override() {
        let config = CliConfig {
            hub_url: Some("https://hub.local".to_string()),
            token: None,
            edges: HashMap::new(),
        };
        assert_eq!(
            resolve_hub(Some("http://override:8080"), &config).as_deref(),
            Some("http://override:8080")
        );
    }

    #[test]
    fn resolve_token_prefers_override() {
        let config = CliConfig {
            hub_url: None,
            token: Some("from-config".to_string()),
            edges: HashMap::new(),
        };
        assert_eq!(
            resolve_token(Some("override"), &config).as_deref(),
            Some("override")
        );
    }

    #[test]
    fn expand_tilde_uses_home_dir() {
        let Ok(home) = std::env::var("HOME") else {
            return;
        };
        let path = expand_tilde("~/.config/forgemux/config.toml");
        assert!(path.starts_with(PathBuf::from(home)));
    }

    #[test]
    fn doctor_report_marks_unreachable_hub() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("config.toml");
        std::fs::write(&config_path, "hub_url = \"http://hub\"").unwrap();
        let report = doctor_report(
            &config_path,
            Some("http://hub".to_string()),
            "http://edge".to_string(),
            None,
            |url, _| url.contains("edge"),
        );
        assert_eq!(report.hub_ok, Some(false));
        assert!(!report.ok());
    }

    #[test]
    fn doctor_report_allows_missing_hub() {
        let tmp = tempfile::tempdir().unwrap();
        let config_path = tmp.path().join("config.toml");
        std::fs::write(&config_path, "token = \"abc\"").unwrap();
        let report = doctor_report(
            &config_path,
            None,
            "http://edge".to_string(),
            Some("abc".to_string()),
            |_, _| true,
        );
        assert!(report.ok());
        assert_eq!(report.hub_ok, None);
        assert!(report.token_present);
    }

    fn make_session(id: &str, name: Option<&str>) -> forgemux_core::SessionRecord {
        let mut session = forgemux_core::SessionRecord::new(
            forgemux_core::AgentType::Codex,
            "gpt-5.3-codex",
            PathBuf::from("/tmp/repo"),
        );
        session.id = forgemux_core::SessionId::from(id);
        session.name = name.map(|value| value.to_string());
        session
    }

    #[test]
    fn resolve_session_id_by_query_prefers_name_match() {
        let sessions = vec![
            make_session("S-11111111", Some("hl-main")),
            make_session("S-aaaa2222", Some("other")),
        ];
        let id = resolve_session_id_by_query(&sessions, "hl").unwrap();
        assert_eq!(id, "S-11111111");
    }

    #[test]
    fn resolve_session_id_by_query_falls_back_to_id_prefix() {
        let sessions = vec![
            make_session("S-11111111", Some("alpha")),
            make_session("S-2222bbbb", Some("beta")),
        ];
        let id = resolve_session_id_by_query(&sessions, "S-2222").unwrap();
        assert_eq!(id, "S-2222bbbb");
    }

    #[test]
    fn resolve_session_id_by_query_errors_on_multiple_name_matches() {
        let sessions = vec![
            make_session("S-11111111", Some("hl-main")),
            make_session("S-22222222", Some("hl-worker")),
        ];
        let err = resolve_session_id_by_query(&sessions, "hl").unwrap_err();
        assert!(err.contains("multiple session names match"));
    }

    #[test]
    fn resolve_session_id_by_query_errors_on_multiple_id_matches() {
        let sessions = vec![
            make_session("S-abc11111", Some("alpha")),
            make_session("S-abc22222", Some("beta")),
        ];
        let err = resolve_session_id_by_query(&sessions, "S-abc").unwrap_err();
        assert!(err.contains("multiple session IDs match"));
    }

    #[test]
    fn resolve_session_id_by_query_errors_when_no_match() {
        let sessions = vec![
            make_session("S-11111111", Some("alpha")),
            make_session("S-22222222", Some("beta")),
        ];
        let err = resolve_session_id_by_query(&sessions, "zzz").unwrap_err();
        assert!(err.contains("no session name or ID starts with"));
    }
}
