use clap::Parser;
use std::sync::Arc;

use libnoa::server::github_broker::GithubBroker;
use libnoa::server::{router, AppState};

static VERSION_TEXT: &str = concat!(
    env!("CARGO_PKG_VERSION"),
    "\n\n",
    "Server component of noa, providing HTTP API for remote synchronization\n",
    "of workspaces, snapshots, and objects.\n\n",
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
#[command(name = "noa-server")]
#[command(about = "Server for the noa distributed version control system")]
#[command(version = VERSION_TEXT)]
#[command(
    after_help = "Set NOA_API_TOKEN environment variable to enable Bearer token authentication."
)]
struct ServerApp {
    #[arg(
        long,
        default_value = "target/noa-server.redb",
        help = "Path to the database file"
    )]
    db_path: String,
    /// Live export (running state) — same as POST /api/v1/admin/export
    /// but as a one-shot: start, export, exit.
    #[arg(long, value_name = "DIR")]
    export_state: Option<String>,
    /// Offline restore from a backup directory — runs BEFORE the database
    /// is ever opened, then continues a normal startup against it.
    #[arg(long, value_name = "DIR")]
    import_state: Option<String>,
    #[arg(short, long, default_value_t = 3000, help = "Port to listen on")]
    port: u16,
    #[arg(
        short = 'H',
        long,
        default_value = "127.0.0.1",
        help = "Host address to bind to"
    )]
    host: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app = ServerApp::parse();

    if let Some(parent) = std::path::Path::new(&app.db_path).parent() {
        std::fs::create_dir_all(parent)?;
    }
    // Offline restore FIRST — never open the live database before the
    // files are in place.
    if let Some(backup_dir) = &app.import_state {
        let scratch_dir = libnoa::server::mirror::MirrorConfig::from_env(
            std::path::Path::new(&app.db_path)
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("mirror-cache"),
        )
        .scratch_dir;
        libnoa::server::backup::import_state(
            std::path::Path::new(backup_dir),
            std::path::Path::new(&app.db_path),
            &scratch_dir,
        )
        .map_err(anyhow::Error::msg)?;
        println!("state restored from {backup_dir}");
    }
    let db = Arc::new(redb::Database::builder().create(&app.db_path)?);

    let api_token = std::env::var("NOA_API_TOKEN")
        .map_err(|_| {
            anyhow::anyhow!(
                "NOA_API_TOKEN environment variable is required. The server will not start without authentication configured."
            )
        })?
        .trim()
        .to_string();
    if api_token.is_empty() {
        // An empty token compares equal to an empty Basic password — the
        // /git surface would become anonymously readable (S1, #146 review).
        anyhow::bail!("NOA_API_TOKEN is set but empty — set a real token");
    }

    let github_broker = GithubBroker::from_env().unwrap_or_else(|e| {
        eprintln!("github broker misconfigured (routes stay disabled): {e}");
        None
    });

    let default_scratch = std::path::Path::new(&app.db_path)
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("mirror-cache");
    let mirror_config = libnoa::server::mirror::MirrorConfig::from_env(default_scratch);
    let state = AppState::new(std::sync::Arc::clone(&db))
        .with_db_path(&app.db_path)
        .with_api_token(api_token)
        .with_github_broker(github_broker)
        .with_mirror_config(mirror_config.clone());

    if std::env::var("NOA_MIRROR_ENABLED").ok().as_deref() == Some("1") {
        libnoa::server::mirror::spawn_scheduler(state.db.clone(), mirror_config.clone());
        println!(
            "mirror scheduler enabled (interval {}s, scratch {})",
            mirror_config.interval_secs,
            mirror_config.scratch_dir.display()
        );
    }

    let router = router(state);

    let addr = format!("{}:{}", app.host, app.port)
        .parse::<std::net::SocketAddr>()
        .map_err(|e| anyhow::anyhow!("invalid address '{}:{}': {}", app.host, app.port, e))?;

    println!("noa-server listening on {addr} (API token authentication enabled)");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(
        listener,
        router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;

    Ok(())
}
