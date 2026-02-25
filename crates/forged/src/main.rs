use clap::{Parser, Subcommand};
use forged::{ForgedConfig, OsCommandRunner, SessionService};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "forged")]
#[command(about = "Forgemux edge daemon", long_about = None)]
struct Cli {
    #[arg(long, default_value = "./.forgemux")]
    data_dir: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run,
    Check,
    Sessions,
    Health,
    Version,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    let config = ForgedConfig::default_with_data_dir(cli.data_dir);
    let service = SessionService::new(config, OsCommandRunner);

    match cli.command {
        Command::Run => {
            println!("forged run: not implemented yet");
        }
        Command::Check => {
            service.list_sessions()?;
            println!("ok");
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
        Command::Health => {
            println!("{\"status\":\"healthy\"}");
        }
        Command::Version => {
            println!("forged 0.1.0");
        }
    }

    Ok(())
}
