use clap::{Parser, Subcommand};
use forged::{ForgedConfig, OsCommandRunner, SessionService};
use forgemux_core::{sort_sessions, AgentType, SessionState};
use serde::Deserialize;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

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
    Start {
        #[arg(long, default_value = "claude")]
        agent: String,
        #[arg(long, default_value = "sonnet")]
        model: String,
        #[arg(long, default_value = ".")]
        repo: String,
        #[arg(long)]
        notify: Vec<String>,
        #[arg(long)]
        policy: Option<String>,
        #[arg(long)]
        worktree: bool,
        #[arg(long)]
        branch: Option<String>,
        #[arg(long)]
        worktree_path: Option<String>,
    },
    Attach {
        session_id: String,
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
    let config = ForgedConfig::default_with_data_dir(cli.data_dir);
    let service = SessionService::new(config, OsCommandRunner);

    match cli.command {
        Command::Start {
            agent,
            model,
            repo,
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
                repo,
                worktree: worktree_spec.is_some(),
                branch: worktree_spec.as_ref().map(|spec| spec.branch.clone()),
                worktree_path: worktree_spec.as_ref().and_then(|spec| {
                    spec.path.as_ref().map(|p| p.to_string_lossy().to_string())
                }),
                notify: if notify.is_empty() { None } else { Some(notify) },
                policy,
            };

            let client = reqwest::blocking::Client::new();
            let url = format!("{}/sessions/start", edge_addr.trim_end_matches('/'));
            let mut req = client.post(url).json(&request);
            if let Some(token) = token.as_deref() {
                req = req.bearer_auth(token);
            }
            let response = req.send();
            match response {
                Ok(resp) if resp.status().is_success() => {
                    let body = resp.json::<forged::server::StartResponse>().unwrap();
                    println!("{}", body.session_id);
                }
                Ok(resp) => {
                    let body = resp
                        .json::<forged::server::ErrorResponse>()
                        .unwrap_or(forged::server::ErrorResponse {
                            error: "unknown error".to_string(),
                        });
                    eprintln!("start failed: {}", body.error);
                    std::process::exit(1);
                }
                Err(err) => {
                    eprintln!("start failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        Command::Attach { session_id } => {
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
            let mut req = client.post(url);
            if let Some(token) = token.as_deref() {
                req = req.bearer_auth(token);
            }
            let response = req.send();
            match response {
                Ok(resp) if resp.status().is_success() => {}
                Ok(resp) => {
                    let body = resp
                        .json::<forged::server::ErrorResponse>()
                        .unwrap_or(forged::server::ErrorResponse {
                            error: "unknown error".to_string(),
                        });
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
                "{}/sessions/{}/stop",
                edge_addr.trim_end_matches('/'),
                session_id
            );
            let mut req = client.post(url);
            if let Some(token) = token.as_deref() {
                req = req.bearer_auth(token);
            }
            let response = req.send();
            match response {
                Ok(resp) if resp.status().is_success() => {}
                Ok(resp) => {
                    let body = resp
                        .json::<forged::server::ErrorResponse>()
                        .unwrap_or(forged::server::ErrorResponse {
                            error: "unknown error".to_string(),
                        });
                    eprintln!("kill failed: {}", body.error);
                    std::process::exit(1);
                }
                Err(err) => {
                    eprintln!("kill failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        Command::Ls => {
            let client = reqwest::blocking::Client::new();
            let url = format!("{}/sessions", edge_addr.trim_end_matches('/'));
            let mut req = client.get(url);
            if let Some(token) = token.as_deref() {
                req = req.bearer_auth(token);
            }
            let response = req.send();
            match response {
                Ok(resp) if resp.status().is_success() => {
                    let sessions: Vec<forgemux_core::SessionRecord> = resp.json().unwrap();
                    print_sessions(sessions);
                }
                Ok(resp) => {
                    let body = resp
                        .json::<forged::server::ErrorResponse>()
                        .unwrap_or(forged::server::ErrorResponse {
                            error: "unknown error".to_string(),
                        });
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
            let mut req = client.get(url);
            if let Some(token) = token.as_deref() {
                req = req.bearer_auth(token);
            }
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
                    let body = resp
                        .json::<forged::server::ErrorResponse>()
                        .unwrap_or(forged::server::ErrorResponse {
                            error: "unknown error".to_string(),
                        });
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
                let mut req = client.get(url);
                if let Some(token) = token.as_deref() {
                    req = req.bearer_auth(token);
                }
                let response = req.send();
                match response {
                    Ok(resp) if resp.status().is_success() => {
                        let payload: JsonValue = resp.json().unwrap();
                        if let Some(content) = payload.get("content").and_then(|v| v.as_str()) {
                            println!("{content}");
                        }
                    }
                    Ok(resp) => {
                        let body = resp
                            .json::<forged::server::ErrorResponse>()
                            .unwrap_or(forged::server::ErrorResponse {
                                error: "unknown error".to_string(),
                            });
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
                    let mut req = client.get(url);
                    if let Some(token) = token.as_deref() {
                        req = req.bearer_auth(token);
                    }
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
                            let body = resp
                                .json::<forged::server::ErrorResponse>()
                                .unwrap_or(forged::server::ErrorResponse {
                                    error: "unknown error".to_string(),
                                });
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
            let mut req = client.get(url);
            if let Some(token) = token.as_deref() {
                req = req.bearer_auth(token);
            }
            let response = req.send();
            match response {
                Ok(resp) if resp.status().is_success() => {
                    let sessions: Vec<forgemux_core::SessionRecord> = resp.json().unwrap();
                    print_sessions(sessions);
                }
                Ok(resp) => {
                    let body = resp
                        .json::<forged::server::ErrorResponse>()
                        .unwrap_or(forged::server::ErrorResponse {
                            error: "unknown error".to_string(),
                        });
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
            let mut req = client.get(url);
            if let Some(token) = token.as_deref() {
                req = req.bearer_auth(token);
            }
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
            let mut req = client.get(url);
            if let Some(token) = token.as_deref() {
                req = req.bearer_auth(token);
            }
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
            let mut req = client.post(url).json(&serde_json::json!({ "input": input }));
            if let Some(token) = token.as_deref() {
                req = req.bearer_auth(token);
            }
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
        Command::Version => {
            println!("fmux 0.1.0");
        }
    }
}

fn print_sessions(sessions: Vec<forgemux_core::SessionRecord>) {
    if sessions.is_empty() {
        println!("no sessions");
        return;
    }
    println!("ID\\tAGENT\\tMODEL\\tSTATE");
    for session in sessions {
        let state = match session.state {
            SessionState::WaitingInput => "waiting",
            SessionState::Running => "running",
            SessionState::Idle => "idle",
            SessionState::Errored => "errored",
            SessionState::Terminated => "terminated",
            SessionState::Provisioning => "provisioning",
            SessionState::Starting => "starting",
        };
        println!(
            "{}\\t{:?}\\t{}\\t{}",
            session.id, session.agent, session.model, state
        );
    }
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

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(stripped) = path.strip_prefix("~/") {
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(stripped);
        }
    }
    PathBuf::from(path)
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
        assert_eq!(
            resolve_edge(None, None, &config),
            "https://hub.example"
        );
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
}
