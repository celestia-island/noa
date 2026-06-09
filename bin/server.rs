use clap::Parser;
use std::sync::Arc;

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
    let db = Arc::new(redb::Database::builder().create(&app.db_path)?);

    let api_token = std::env::var("NOA_API_TOKEN").unwrap_or_default();

    let state = AppState::new(db).with_api_token(api_token);
    let router = router(state);

    let addr = format!("{}:{}", app.host, app.port)
        .parse::<std::net::SocketAddr>()
        .map_err(|e| anyhow::anyhow!("invalid address '{}:{}': {}", app.host, app.port, e))?;

    if std::env::var("NOA_API_TOKEN").is_ok() {
        println!("noa-server listening on {addr} (API token authentication enabled)");
    } else {
        println!(
            "noa-server listening on {addr} (WARNING: no authentication configured, set NOA_API_TOKEN)"
        );
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
