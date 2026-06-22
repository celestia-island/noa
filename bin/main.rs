use clap::{CommandFactory, Parser, Subcommand};
use std::path::PathBuf;

use libnoa::{cli, snapshot::SnapshotStore};

static VERSION_TEXT: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    "\n\n",
    "An AI-native distributed version control system with per-agent workspace\n",
    "isolation, JSONL append-only logs, snapshot-based history, and full git\n",
    "protocol compatibility.\n\n",
    "Authors:  ",
    env!("CARGO_PKG_AUTHORS"),
    "\n",
    "License:  ",
    env!("CARGO_PKG_LICENSE"),
    "\n",
    "Repository: ",
    env!("CARGO_PKG_REPOSITORY"),
    "\n",
    "Documentation: https://docs.rs/libnoa",
);

#[derive(Parser)]
#[command(name = "noa")]
#[command(about = "AI-native distributed version control system")]
#[command(version = VERSION_TEXT)]
#[command(after_help = "Run 'noa <COMMAND> --help' for more information on a command.")]
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
        no_git: bool,
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
    CoAuthor {
        #[command(subcommand)]
        cmd: CoauthorSub,
    },
    Hook {
        #[command(subcommand)]
        cmd: HookSub,
    },
    Sync {
        #[arg(short, long, default_value = "/tmp/noa-sync.sock")]
        socket: String,
        /// Path to the workspace root directory (default: current directory)
        #[arg(short, long, default_value = ".")]
        path: String,
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
        #[arg(short, long, default_value = "ours")]
        strategy: String,
    },
}

#[derive(Subcommand)]
enum RemoteSub {
    Add { name: String, url: String },
    Remove { name: String },
    List,
}

#[derive(Subcommand)]
enum CoauthorSub {
    /// Resolve and print the co-author trailer block for the current agent session.
    Resolve {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long)]
        chat_log_dir: Option<PathBuf>,
        #[arg(long)]
        aporia_config: Option<PathBuf>,
        #[arg(long, default_value_t = 0)]
        lookback_secs: u64,
    },
}

#[derive(Subcommand)]
enum HookSub {
    /// Install noa-managed git hooks (commit-msg + pre-commit) into a repository.
    Install {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long, default_value_t = false)]
        force: bool,
        #[arg(long)]
        noa_bin: Option<String>,
    },
    /// Run the pre-commit gate (secret scan + cargo check).
    ///
    /// Invoked by the installed `.git/hooks/pre-commit` wrapper. Not normally
    /// called by humans directly.
    PreCommit,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt().try_init();
    let app = App::parse();

    match app.command {
        None => {
            let mut cmd = App::command();
            cmd.print_help()?;
        }
        Some(Commands::Init {
            path,
            noa_remote,
            no_git,
        }) => {
            cli::init::run(&cli::init::InitArgs {
                path,
                noa_remote,
                no_git,
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
            WorkspaceSub::Merge { from, strategy } => {
                let root = libnoa::repo::Repository::find(std::path::Path::new("."))?;
                let repo = libnoa::repo::Repository::open(&root)?;
                cli::workspace_cmd::run_merge(&repo, &from, &strategy).await?;
            }
        },
        Some(Commands::Remote { cmd }) => match cmd {
            RemoteSub::Add { name, url } => {
                let root = libnoa::repo::Repository::find(std::path::Path::new("."))?;
                let mut repo = libnoa::repo::Repository::open(&root)?;
                cli::remote_cmd::run_add(&mut repo, &name, &url)?;
            }
            RemoteSub::Remove { name } => {
                let root = libnoa::repo::Repository::find(std::path::Path::new("."))?;
                let mut repo = libnoa::repo::Repository::open(&root)?;
                cli::remote_cmd::run_remove(&mut repo, &name)?;
            }
            RemoteSub::List => {
                let root = libnoa::repo::Repository::find(std::path::Path::new("."))?;
                let repo = libnoa::repo::Repository::open(&root)?;
                cli::remote_cmd::run_list(&repo)?;
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
        Some(Commands::CoAuthor { cmd }) => match cmd {
            CoauthorSub::Resolve {
                repo,
                chat_log_dir,
                aporia_config,
                lookback_secs,
            } => {
                cli::coauthor_cmd::run(cli::coauthor_cmd::ResolveArgs {
                    repo,
                    chat_log_dir,
                    aporia_config,
                    lookback_secs,
                })?;
            }
        },
        Some(Commands::Hook { cmd }) => match cmd {
            HookSub::Install {
                repo,
                force,
                noa_bin,
            } => {
                cli::hook_cmd::run(cli::hook_cmd::InstallArgs {
                    repo,
                    force,
                    noa_bin,
                })?;
            }
            HookSub::PreCommit => {
                cli::hook_cmd::run_pre_commit()?;
            }
        },
        Some(Commands::Sync { socket, path }) => {
            let root = std::path::Path::new(&path)
                .canonicalize()
                .unwrap_or_else(|_| std::path::PathBuf::from(&path));
            let ws_root = libnoa::repo::Repository::find(&root)?;
            let server =
                libnoa::sync::SyncServer::new(std::path::Path::new(&socket), &ws_root, "default")?;
            server.listen().await?;
        }
    }

    Ok(())
}

// The previous `tests::test_try_init_called_twice_does_not_panic` test was
// removed: it called `tracing_subscriber::fmt().try_init()` (a standard
// library method that is documented to never panic) twice, discarded both
// results, and asserted nothing about project behaviour. The production
// mitigation it claimed to lock in (`let _ = try_init()` on line 188 above)
// is itself a one-line idiom that does not warrant a regression test — any
// regression to `init()` would surface as a panic the first time a test in
// this binary's process tree initialised the global subscriber twice.
