use clap::{Parser, Subcommand};
use std::path::PathBuf;

use libnoa::cli;
use libnoa::snapshot::SnapshotStore;

const VERSION: &str = env!("CARGO_PKG_VERSION");

#[derive(Parser)]
#[command(name = "noa")]
#[command(about = "AI-native distributed version control system")]
#[command(version = VERSION)]
struct App {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    Init {
        #[arg(default_value = ".")]
        path: PathBuf,
        #[arg(long)]
        noa_remote: Option<String>,
        #[arg(long)]
        git: bool,
    },
    Status,
    Log {
        #[arg(short, long)]
        workspace: Option<String>,
        #[arg(short, long, default_value_t = 20)]
        limit: usize,
        #[arg(short, long)]
        tui: bool,
    },
    Snapshot {
        #[command(subcommand)]
        cmd: SnapshotSub,
    },
    Workspace {
        #[command(subcommand)]
        cmd: WorkspaceSub,
    },
    Remote {
        #[command(subcommand)]
        cmd: RemoteSub,
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
        #[arg(long)]
        svn: bool,
    },
    Resolve {
        #[arg(short, long, default_value = "ours")]
        strategy: String,
        #[arg(short, long)]
        path: Option<String>,
    },
}

#[derive(Subcommand)]
enum SnapshotSub {
    Create {
        #[arg(short, long, default_value = "")]
        message: String,
        #[arg(short, long, default_value = "default")]
        author: String,
    },
    List,
    Diff {
        a: String,
        b: String,
    },
}

#[derive(Subcommand)]
enum WorkspaceSub {
    Create {
        name: String,
        #[arg(short, long)]
        agent: Option<String>,
    },
    Switch {
        name: String,
    },
    List {
        #[arg(short, long)]
        tui: bool,
    },
    Delete {
        name: String,
    },
    Merge {
        from: String,
    },
}

#[derive(Subcommand)]
enum RemoteSub {
    Add { name: String, url: String },
    Remove { name: String },
    List,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app = App::parse();

    match app.command {
        None => {}
        Some(Commands::Init {
            path,
            noa_remote,
            git,
        }) => {
            cli::init::run(&cli::init::InitArgs {
                path,
                noa_remote,
                git,
            })?;
        }
        Some(Commands::Status) => {
            cli::status::run().await?;
        }
        Some(Commands::Log {
            workspace,
            limit,
            tui,
        }) => {
            if tui {
                let root = libnoa::repo::Repository::find(std::path::Path::new("."))?;
                let repo = libnoa::repo::Repository::open(&root)?;
                let snap_store = repo.snapshot_store()?;
                let snapshots = snap_store.list_all().await?;
                let current = repo.read_head()?;
                let app = libnoa::tui::App::for_log(snapshots, current);
                let mut terminal = libnoa::tui::setup_terminal()?;
                libnoa::tui::run_interactive(&mut terminal, app)?;
                libnoa::tui::cleanup_terminal(&mut terminal)?;
            } else {
                cli::log_cmd::run(workspace.as_deref(), limit).await?;
            }
        }
        Some(Commands::Snapshot { cmd }) => match cmd {
            SnapshotSub::Create { message, author } => {
                let root = libnoa::repo::Repository::find(std::path::Path::new("."))?;
                let repo = libnoa::repo::Repository::open(&root)?;
                cli::snapshot_cmd::run_create(&repo, &message, &author).await?;
            }
            SnapshotSub::List => {
                let root = libnoa::repo::Repository::find(std::path::Path::new("."))?;
                let repo = libnoa::repo::Repository::open(&root)?;
                cli::snapshot_cmd::run_list(&repo).await?;
            }
            SnapshotSub::Diff { a, b } => {
                let root = libnoa::repo::Repository::find(std::path::Path::new("."))?;
                let repo = libnoa::repo::Repository::open(&root)?;
                cli::snapshot_cmd::run_diff(&repo, &a, &b).await?;
            }
        },
        Some(Commands::Workspace { cmd }) => match cmd {
            WorkspaceSub::Create { name, agent } => {
                let root = libnoa::repo::Repository::find(std::path::Path::new("."))?;
                let repo = libnoa::repo::Repository::open(&root)?;
                cli::workspace_cmd::run_create(&repo, &name, agent.as_deref()).await?;
            }
            WorkspaceSub::Switch { name } => {
                let root = libnoa::repo::Repository::find(std::path::Path::new("."))?;
                let repo = libnoa::repo::Repository::open(&root)?;
                cli::workspace_cmd::run_switch(&repo, &name).await?;
            }
            WorkspaceSub::List { tui } => {
                let root = libnoa::repo::Repository::find(std::path::Path::new("."))?;
                let repo = libnoa::repo::Repository::open(&root)?;
                if tui {
                    let ws_mgr = repo.workspace_manager()?;
                    let branches = ws_mgr.list().await?;
                    let snap_store = repo.snapshot_store()?;
                    let snapshots = snap_store.list_all().await?;
                    let current = repo.read_head()?;
                    let app = libnoa::tui::App::for_branches(branches, snapshots, current);
                    let mut terminal = libnoa::tui::setup_terminal()?;
                    libnoa::tui::run_interactive(&mut terminal, app)?;
                    libnoa::tui::cleanup_terminal(&mut terminal)?;
                } else {
                    cli::workspace_cmd::run_list(&repo).await?;
                }
            }
            WorkspaceSub::Delete { name } => {
                let root = libnoa::repo::Repository::find(std::path::Path::new("."))?;
                let repo = libnoa::repo::Repository::open(&root)?;
                cli::workspace_cmd::run_delete(&repo, &name).await?;
            }
            WorkspaceSub::Merge { from } => {
                let root = libnoa::repo::Repository::find(std::path::Path::new("."))?;
                let repo = libnoa::repo::Repository::open(&root)?;
                cli::workspace_cmd::run_merge(&repo, &from).await?;
            }
        },
        Some(Commands::Remote { cmd }) => match cmd {
            RemoteSub::Add { name, url } => {
                let root = libnoa::repo::Repository::find(std::path::Path::new("."))?;
                let mut repo = libnoa::repo::Repository::open(&root)?;
                cli::remote_cmd::run_add(&mut repo, &name, &url).await?;
            }
            RemoteSub::Remove { name } => {
                let root = libnoa::repo::Repository::find(std::path::Path::new("."))?;
                let mut repo = libnoa::repo::Repository::open(&root)?;
                cli::remote_cmd::run_remove(&mut repo, &name).await?;
            }
            RemoteSub::List => {
                let root = libnoa::repo::Repository::find(std::path::Path::new("."))?;
                let repo = libnoa::repo::Repository::open(&root)?;
                cli::remote_cmd::run_list(&repo).await?;
            }
        },
        Some(Commands::Push { remote }) => {
            cli::pushpull::run_push(&remote).await?;
        }
        Some(Commands::Pull { remote }) => {
            cli::pushpull::run_pull(&remote).await?;
        }
        Some(Commands::Fetch { remote }) => {
            cli::pushpull::run_fetch(&remote).await?;
        }
        Some(Commands::Clone { url, path, svn }) => {
            if svn {
                cli::pushpull::run_clone_svn(&url, &path).await?;
            } else {
                cli::pushpull::run_clone(&url, &path).await?;
            }
        }
        Some(Commands::Resolve { strategy, path }) => {
            let root = libnoa::repo::Repository::find(std::path::Path::new("."))?;
            let repo = libnoa::repo::Repository::open(&root)?;
            cli::resolve_cmd::run_resolve(&repo, &strategy, path.as_deref()).await?;
        }
    }

    Ok(())
}
