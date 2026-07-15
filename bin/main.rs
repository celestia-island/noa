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
    #[cfg(unix)]
    Sync {
        #[arg(short, long, default_value = "/tmp/noa-sync.sock")]
        socket: String,
        #[arg(short, long, default_value = ".")]
        path: String,
    },
    Storage {
        #[command(subcommand)]
        cmd: StorageSub,
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
    Install {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long, default_value_t = false)]
        force: bool,
        #[arg(long)]
        noa_bin: Option<String>,
    },
    PreCommit,
<<<<<<< HEAD
=======
    ValidateMsg {
        #[arg(long, short)]
        message: String,
        #[arg(long, short, default_value = "celestia")]
        preset: String,
        #[arg(long, default_value = ".")]
        repo: PathBuf,
    },
    InstallAction {
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        #[arg(long, default_value_t = false)]
        force: bool,
    },
>>>>>>> origin/dev
}
#[derive(Subcommand)]
enum StorageSub {
    Add {
        name: String,
        #[arg(short, long)]
        r#type: String,
        #[arg(long)]
        endpoint: Option<String>,
        #[arg(long)]
        gateway: Option<String>,
        #[arg(long)]
        auth_token: Option<String>,
        #[arg(long)]
        auto_pin: bool,
        #[arg(long)]
        bucket: Option<String>,
        #[arg(long)]
        access_key: Option<String>,
        #[arg(long)]
        secret_key: Option<String>,
        #[arg(long)]
        region: Option<String>,
        #[arg(long)]
        username: Option<String>,
        #[arg(long)]
        password: Option<String>,
        #[arg(long)]
        port: u16,
        #[arg(long)]
        tls: bool,
    },
    Remove {
        name: String,
    },
    List,
    Status {
        name: Option<String>,
    },
    Push {
        #[arg(long)]
        target: Option<String>,
        #[arg(long)]
        snapshot: Option<String>,
        #[arg(short, long)]
        workspace: Option<String>,
        #[arg(long)]
        pin: bool,
    },
    Fetch {
        target: String,
        hash_or_cid: String,
    },
}

fn find_repo() -> anyhow::Result<libnoa::repo::Repository> {
    let root = libnoa::repo::Repository::find(std::path::Path::new("."))?;
    libnoa::repo::Repository::open(&root)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = tracing_subscriber::fmt().try_init();
    let app = App::parse();

    match app.command {
        None => {
            App::command().print_help()?;
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
        Some(Commands::Status) => cli::status::run().await?,
        Some(Commands::Log {
            workspace,
            limit,
            tui,
        }) => {
            if tui {
                let repo = find_repo()?;
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
                let repo = find_repo()?;
                cli::snapshot_cmd::run_create(&repo, &message, &author).await?;
            }
            SnapshotSub::List => {
                let repo = find_repo()?;
                cli::snapshot_cmd::run_list(&repo).await?;
            }
            SnapshotSub::Diff { a, b } => {
                let repo = find_repo()?;
                cli::snapshot_cmd::run_diff(&repo, &a, &b).await?;
            }
        },
        Some(Commands::Workspace { cmd }) => match cmd {
            WorkspaceSub::Create { name, agent } => {
                let repo = find_repo()?;
                cli::workspace_cmd::run_create(&repo, &name, agent.as_deref()).await?;
            }
            WorkspaceSub::Switch { name } => {
                let repo = find_repo()?;
                cli::workspace_cmd::run_switch(&repo, &name).await?;
            }
            WorkspaceSub::List { tui } => {
                let repo = find_repo()?;
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
                let repo = find_repo()?;
                cli::workspace_cmd::run_delete(&repo, &name).await?;
            }
            WorkspaceSub::Merge { from, strategy } => {
                let repo = find_repo()?;
                cli::workspace_cmd::run_merge(&repo, &from, &strategy).await?;
            }
        },
        Some(Commands::Remote { cmd }) => match cmd {
            RemoteSub::Add { name, url } => {
                let mut repo = find_repo()?;
                cli::remote_cmd::run_add(&mut repo, &name, &url)?;
            }
            RemoteSub::Remove { name } => {
                let mut repo = find_repo()?;
                cli::remote_cmd::run_remove(&mut repo, &name)?;
            }
            RemoteSub::List => {
                let repo = find_repo()?;
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
            let repo = find_repo()?;
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
            HookSub::PreCommit => cli::hook_cmd::run_pre_commit()?,
<<<<<<< HEAD
=======
            HookSub::ValidateMsg {
                message,
                preset,
                repo,
            } => {
                cli::hook_cmd::validate_msg(cli::hook_cmd::ValidateMsgArgs {
                    message,
                    preset: Some(preset),
                    repo,
                })?;
                println!("OK");
            }
            HookSub::InstallAction { repo, force } => {
                cli::hook_cmd::install_action(cli::hook_cmd::InstallActionArgs {
                    repo,
                    force,
                })?;
            }
>>>>>>> origin/dev
        },
        #[cfg(unix)]
        Some(Commands::Sync { socket, path }) => {
            let root = std::path::Path::new(&path)
                .canonicalize()
                .unwrap_or_else(|_| PathBuf::from(&path));
            let ws_root = libnoa::repo::Repository::find(&root)?;
            let server =
                libnoa::sync::SyncServer::new(std::path::Path::new(&socket), &ws_root, "default")?;
            server.listen().await?;
        }
        Some(Commands::Storage { cmd }) => match cmd {
            StorageSub::Add {
                name,
                r#type,
                endpoint,
                gateway,
                auth_token,
                auto_pin,
                bucket,
                access_key,
                secret_key,
                region,
                username,
                password,
                port,
                tls,
            } => {
                let mut repo = find_repo()?;
                cli::storage_cmd::run_add(
                    &mut repo,
                    &name,
                    &r#type,
                    cli::storage_cmd::StorageAddOptions {
                        endpoint,
                        gateway,
                        auth_token,
                        auto_pin,
                        bucket,
                        access_key,
                        secret_key,
                        region,
                        username,
                        password,
                        port,
                        use_tls: tls,
                    },
                )?;
            }
            StorageSub::Remove { name } => {
                let mut repo = find_repo()?;
                cli::storage_cmd::run_remove(&mut repo, &name)?;
            }
            StorageSub::List => {
                let repo = find_repo()?;
                cli::storage_cmd::run_list(&repo)?;
            }
            StorageSub::Status { name } => {
                let repo = find_repo()?;
                cli::storage_cmd::run_status(&repo, name.as_deref()).await?;
            }
            StorageSub::Push {
                target,
                snapshot,
                workspace,
                pin,
            } => {
                let repo = find_repo()?;
                cli::storage_cmd::run_push(
                    &repo,
                    target.as_deref(),
                    snapshot.as_deref(),
                    workspace.as_deref(),
                    pin,
                )
                .await?;
            }
            StorageSub::Fetch {
                target,
                hash_or_cid,
            } => {
                let repo = find_repo()?;
                cli::storage_cmd::run_fetch(&repo, &target, &hash_or_cid).await?;
            }
        },
    }
    Ok(())
}
