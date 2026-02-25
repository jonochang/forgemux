use clap::{Parser, Subcommand};
use forgehub::{HubConfig, HubService};
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(name = "forgehub")]
#[command(about = "Forgemux hub", long_about = None)]
struct Cli {
    #[arg(long, default_value = "./.forgemux-hub.toml")]
    config: PathBuf,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run,
    Check,
    Sessions,
    Version,
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
        Command::Run => println!("forgehub run: not implemented yet"),
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
