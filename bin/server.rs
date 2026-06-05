use std::sync::Arc;

use noa::server::{router, AppState};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let db_path = std::env::var("NOA_DB_PATH").unwrap_or_else(|_| "noa-server.redb".to_string());
    let db = Arc::new(redb::Database::builder().create(&db_path)?);

    let state = AppState::new(db);
    let app = router(state);

    let addr = std::net::SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("noa-server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
