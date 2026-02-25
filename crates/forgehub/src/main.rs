use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "forgehub")]
#[command(about = "Forgemux hub", long_about = None)]
struct Cli {
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
    match cli.command {
        Command::Run => println!("forgehub run: not implemented yet"),
        Command::Check => println!("ok"),
        Command::Sessions => println!("no sessions"),
        Command::Version => println!("forgehub 0.1.0"),
    }
    Ok(())
}
