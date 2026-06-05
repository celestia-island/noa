mod handlers;

use axum::{
    routing::{get, post},
    Router,
};
pub use handlers::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/api/v1/refs",
            get(handlers::list_refs).post(handlers::push_refs),
        )
        .route("/api/v1/blobs", post(handlers::upload_blobs))
        .route("/api/v1/blob/{hash}", get(handlers::get_blob))
        .route("/api/v1/trees", post(handlers::upload_trees))
        .route("/api/v1/tree/{hash}", get(handlers::get_tree))
        .route(
            "/api/v1/snapshots",
            get(handlers::list_snapshots).post(handlers::create_snapshot),
        )
        .route(
            "/api/v1/workspaces",
            get(handlers::list_workspaces).post(handlers::create_workspace),
        )
        .with_state(state)
}
