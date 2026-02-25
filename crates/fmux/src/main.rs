use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "fmux")]
#[command(about = "Forgemux CLI", long_about = None)]
struct Cli {
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
    },
    Ls,
    Status {
        session_id: String,
    },
    Version,
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let cli = Cli::parse();
    match cli.command {
        Command::Start { agent, model, repo } => {
            println!("start: agent={agent} model={model} repo={repo}");
        }
        Command::Ls => {
            println!("ls: not implemented yet");
        }
        Command::Status { session_id } => {
            println!("status: {session_id}");
        }
        Command::Version => {
            println!("fmux 0.1.0");
        }
    }
}
