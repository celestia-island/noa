use std::sync::Arc;

use noa::server::{router, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db_path = std::env::var("NOA_DB_PATH").unwrap_or_else(|_| "noa-server.redb".to_string());
    let port: u16 = std::env::var("NOA_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);
    let db = Arc::new(redb::Database::builder().create(&db_path)?);

    let state = AppState::new(db);
    let app = router(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], port));
    println!("noa-server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
