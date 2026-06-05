use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "noa")]
#[command(about = "AI-native distributed version control system")]
struct App {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Init {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
}

fn main() -> anyhow::Result<()> {
    let app = App::parse();

    match app.command {
        Commands::Init { path } => {
            let args = noa::cli::init::InitArgs { path };
            noa::cli::init::run(&args)?;
        }
    }

    Ok(())
}
