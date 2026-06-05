use clap::{Parser, Subcommand};
use std::path::PathBuf;

use noa::cli;

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
    Status,
    Log {
        #[arg(short, long)]
        workspace: Option<String>,
        #[arg(short, long, default_value_t = 20)]
        limit: usize,
    },
    Snapshot {
        #[command(subcommand)]
        cmd: cli::snapshot_cmd::SnapshotCommands,
    },
    Workspace {
        #[command(subcommand)]
        cmd: cli::workspace_cmd::WorkspaceCommands,
    },
    Remote {
        #[command(subcommand)]
        cmd: cli::remote_cmd::RemoteCommands,
    },
    Push {
        #[arg(short, long, default_value = "origin")]
        remote: String,
    },
    Pull {
        #[arg(short, long, default_value = "origin")]
        remote: String,
    },
    Fetch {
        #[arg(short, long, default_value = "origin")]
        remote: String,
    },
    Clone {
        url: String,
        #[arg(short, long, default_value = ".")]
        path: String,
    },
}

fn main() -> anyhow::Result<()> {
    let app = App::parse();

    match app.command {
        Commands::Init { path } => {
            cli::init::run(&cli::init::InitArgs { path })?;
        }
        Commands::Status => {
            cli::status::run()?;
        }
        Commands::Log { workspace, limit } => {
            cli::log_cmd::run(&cli::log_cmd::LogArgs { workspace, limit })?;
        }
        Commands::Snapshot { cmd } => {
            cli::snapshot_cmd::run(&cmd)?;
        }
        Commands::Workspace { cmd } => {
            cli::workspace_cmd::run(&cmd)?;
        }
        Commands::Remote { cmd } => {
            cli::remote_cmd::run(&cmd)?;
        }
        Commands::Push { remote } => {
            cli::pushpull::push(&cli::pushpull::PushArgs { remote })?;
        }
        Commands::Pull { remote } => {
            cli::pushpull::pull(&cli::pushpull::PullArgs { remote })?;
        }
        Commands::Fetch { remote } => {
            cli::pushpull::fetch(&cli::pushpull::FetchArgs { remote })?;
        }
        Commands::Clone { url, path } => {
            cli::pushpull::clone(&cli::pushpull::CloneArgs { url, path })?;
        }
    }

    Ok(())
}
