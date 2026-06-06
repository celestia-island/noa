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
    after_help = "Environment variables NOA_DB_PATH and NOA_PORT are still supported as legacy defaults."
)]
struct ServerApp {
    #[arg(
        long,
        default_value = "noa-server.redb",
        help = "Path to the database file"
    )]
    db_path: String,
    #[arg(short, long, default_value_t = 3000, help = "Port to listen on")]
    port: u16,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let app = ServerApp::parse();

    let db = Arc::new(redb::Database::builder().create(&app.db_path)?);

    let state = AppState::new(db);
    let router = router(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], app.port));
    println!("noa-server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, router).await?;

    Ok(())
}
