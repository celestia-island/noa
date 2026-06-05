use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::error::{NoaError, Result};
use crate::object::ObjectStore;
use crate::refs::RefStore;
use crate::snapshot::{SnapshotId, SnapshotStore};
use crate::workspace::WorkspaceManager;

mod handlers;

pub use handlers::AppState;

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/api/v1/repo/{name}/refs", get(handlers::list_refs).post(handlers::push_refs))
        .route("/api/v1/repo/{name}/blobs", post(handlers::upload_blobs))
        .route("/api/v1/repo/{name}/blob/{hash}", get(handlers::get_blob))
        .route("/api/v1/repo/{name}/trees", post(handlers::upload_trees))
        .route("/api/v1/repo/{name}/tree/{hash}", get(handlers::get_tree))
        .route("/api/v1/repo/{name}/snapshots", get(handlers::list_snapshots).post(handlers::create_snapshot))
        .route("/api/v1/repo/{name}/workspaces", get(handlers::list_workspaces).post(handlers::create_workspace))
        .with_state(state)
}
