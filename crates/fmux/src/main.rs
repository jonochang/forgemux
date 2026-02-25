use clap::{Parser, Subcommand};
use forged::{ForgedConfig, OsCommandRunner, SessionService};
use forgemux_core::{sort_sessions, AgentType, SessionState};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

#[derive(Debug, Parser)]
#[command(name = "fmux")]
#[command(about = "Forgemux CLI", long_about = None)]
struct Cli {
    #[arg(long, default_value = "./.forgemux")]
    data_dir: PathBuf,
    #[arg(long, default_value = "./.forgemux-hub.toml")]
    hub_config: PathBuf,
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
        notify: bool,
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
    Version,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let config = ForgedConfig::default_with_data_dir(cli.data_dir);
    let service = SessionService::new(config, OsCommandRunner);

    match cli.command {
        Command::Start {
            agent,
            model,
            repo,
            notify,
        } => {
            if notify {
                eprintln!("warning: --notify requires forged daemon support; falling back to no-op");
            }
            let agent = match agent.as_str() {
                "claude" => AgentType::Claude,
                "codex" => AgentType::Codex,
                other => {
                    eprintln!("unknown agent: {other}");
                    std::process::exit(2);
                }
            };
            match service.start_session(agent, model, repo) {
                Ok(record) => println!("{}", record.id),
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
            if let Err(err) = service.stop_session(&session_id) {
                eprintln!("stop failed: {err}");
                std::process::exit(1);
            }
        }
        Command::Kill { session_id } => {
            if let Err(err) = service.kill_session(&session_id) {
                eprintln!("kill failed: {err}");
                std::process::exit(1);
            }
        }
        Command::Ls => {
            match service.list_sessions() {
                Ok(sessions) => print_sessions(sessions),
                Err(err) => {
                    eprintln!("ls failed: {err}");
                    std::process::exit(1);
                }
            }
        }
        Command::Status { session_id } => match service.list_sessions() {
            Ok(sessions) => {
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
            Err(err) => {
                eprintln!("status failed: {err}");
                std::process::exit(1);
            }
        },
        Command::Logs {
            session_id,
            tail,
            follow,
        } => {
            if !follow {
                match service.logs(&session_id, tail) {
                    Ok(content) => println!("{content}"),
                    Err(err) => {
                        eprintln!("logs failed: {err}");
                        std::process::exit(1);
                    }
                }
            } else {
                loop {
                    match service.logs(&session_id, tail) {
                        Ok(content) => println!("{content}"),
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
            match service.refresh_states() {
                Ok(sessions) => print_sessions(sessions),
                Err(err) => {
                    eprintln!("watch failed: {err}");
                    std::process::exit(1);
                }
            }
            thread::sleep(Duration::from_secs(interval));
        },
        Command::Edges => match forgehub::HubConfig::load(&cli.hub_config) {
            Ok(cfg) => {
                let hub = forgehub::HubService::new(cfg);
                let edges = hub.list_edges();
                if edges.is_empty() {
                    println!("no edges");
                } else {
                    for edge in edges {
                        println!("{} {}", edge.id, edge.data_dir.display());
                    }
                }
            }
            Err(err) => {
                eprintln!("edges failed: {err}");
                std::process::exit(1);
            }
        },
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
