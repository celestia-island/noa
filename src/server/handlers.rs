use base64::Engine;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};

use crate::{
    error::{is_object_not_found, is_workspace_already_exists},
    object::{BlobId, ObjectStore, RedbObjectStore, TreeEntries, TreeId},
    refs::{RedbRefStore, RefStore},
    server::RateLimiter,
    snapshot::{
        content_addressed_snapshot_id_with_ts, RedbSnapshotStore, Snapshot, SnapshotId, SnapshotStore,
    },
    workspace::{Workspace, WorkspaceManager},
};

#[derive(Clone)]
pub struct AppState {
    pub db: Arc<redb::Database>,
    pub api_token: String,
    pub rate_limiter: RateLimiter,
}

impl AppState {
    #[must_use]
    pub fn new(db: Arc<redb::Database>) -> Self {
        AppState {
            db,
            api_token: String::new(),
            rate_limiter: RateLimiter::new(1000, 60),
        }
    }

    #[must_use]
    pub fn with_api_token(mut self, token: String) -> Self {
        self.api_token = token;
        self
    }

    #[must_use]
    pub fn with_rate_limit(mut self, max_requests: u32, window_secs: u64) -> Self {
        self.rate_limiter = RateLimiter::new(max_requests, window_secs);
        self
    }

    pub fn object_store(&self) -> anyhow::Result<RedbObjectStore> {
        RedbObjectStore::new(Arc::clone(&self.db))
    }

    pub fn snapshot_store(&self) -> anyhow::Result<RedbSnapshotStore> {
        RedbSnapshotStore::new(Arc::clone(&self.db))
    }

    pub fn ref_store(&self) -> anyhow::Result<RedbRefStore> {
        RedbRefStore::new(Arc::clone(&self.db))
    }

    pub fn workspace_manager(&self) -> anyhow::Result<WorkspaceManager> {
        WorkspaceManager::new(Arc::clone(&self.db))
    }
}

#[derive(Serialize)]
pub struct ApiError {
    pub error: String,
}

#[derive(Deserialize)]
pub struct PaginationParams {
    #[serde(default = "default_limit")]
    pub limit: usize,
    #[serde(default)]
    pub offset: usize,
}

fn default_limit() -> usize {
    100
}

fn err_json(msg: impl std::fmt::Display) -> (StatusCode, Json<ApiError>) {
    tracing::error!("internal server error: {}", msg);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(ApiError {
            error: "internal server error".to_string(),
        }),
    )
}

fn not_found_json(msg: impl ToString) -> (StatusCode, Json<ApiError>) {
    let msg = msg.to_string();
    tracing::debug!("resource not found: {}", msg);
    (
        StatusCode::NOT_FOUND,
        Json(ApiError {
            error: "resource not found".to_string(),
        }),
    )
}


fn validate_object_hash_id(id: &str) -> Result<(), (StatusCode, Json<ApiError>)> {
    if id.len() != 64 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "invalid hash id: must be exactly 64 hex characters".to_string(),
            }),
        ));
    }
    if !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "invalid hash id format".to_string(),
            }),
        ));
    }
    if id != id.to_ascii_lowercase() {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "invalid hash id: must be lowercase".to_string(),
            }),
        ));
    }
    Ok(())
}

fn validate_ref_name(name: &str) -> Result<(), (StatusCode, Json<ApiError>)> {
    if name.is_empty() || name.len() > 128 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "ref name must be 1-128 characters".to_string(),
            }),
        ));
    }
    if name.contains('\0') || name.contains('\n') || name.contains('\r') {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "ref name contains control characters".to_string(),
            }),
        ));
    }
    if name.starts_with('.') || name.starts_with('-') || name.ends_with('.') {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "ref name has invalid start/end character".to_string(),
            }),
        ));
    }
    if name.contains("..") || name.contains('~') || name.contains('^') || name.contains(':') {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "ref name contains forbidden sequences".to_string(),
            }),
        ));
    }
    for component in name.split('/') {
        if component.is_empty() {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    error: "ref name contains empty component".to_string(),
                }),
            ));
        }
        if component == "." || component == ".." {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(ApiError {
                    error: "ref name contains '.' or '..' component".to_string(),
                }),
            ));
        }
    }
    Ok(())
}

pub async fn list_refs(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, (StatusCode, Json<ApiError>)> {
    let ref_store = state.ref_store().map_err(err_json)?;
    let refs = ref_store.list().await.map_err(err_json)?;
    let result: Vec<serde_json::Value> = refs
        .into_iter()
        .map(|(n, id)| serde_json::json!({"name": n, "id": id.0}))
        .collect();
    Ok(Json(result))
}

#[derive(Deserialize)]
pub struct PushRefsRequest {
    pub name: String,
    pub id: String,
    #[serde(default)]
    pub expected_id: Option<String>,
}

pub async fn push_refs(
    State(state): State<AppState>,
    Json(body): Json<PushRefsRequest>,
) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    validate_ref_name(&body.name)?;
    let ref_store = state.ref_store().map_err(err_json)?;
    let id = SnapshotId(body.id);
    if !id.is_empty() {
        let snap_store = state.snapshot_store().map_err(err_json)?;
        if snap_store.get(&id).await.is_err() {
            return Err(not_found_json("snapshot not found"));
        }
    }
    let old = body.expected_id.as_ref().map(|s| SnapshotId(s.clone()));
    let ok = ref_store
        .cas(&body.name, old.as_ref(), &id)
        .await
        .map_err(err_json)?;
    if ok {
        Ok(StatusCode::CREATED)
    } else {
        Err((
            StatusCode::CONFLICT,
            Json(ApiError {
                error: "CAS conflict: ref value has changed".to_string(),
            }),
        ))
    }
}

#[derive(Deserialize)]
pub struct UploadBlobsRequest {
    pub blobs: Vec<BlobUpload>,
}

#[derive(Deserialize)]
pub struct BlobUpload {
    pub content: String,
}

#[derive(Serialize)]
pub struct UploadResult {
    pub ids: Vec<String>,
}

pub async fn upload_blobs(
    State(state): State<AppState>,
    Json(body): Json<UploadBlobsRequest>,
) -> Result<Json<UploadResult>, (StatusCode, Json<ApiError>)> {
    if body.blobs.len() > 1000 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "too many blobs in single request (max 1000)".to_string(),
            }),
        ));
    }
    let store = state.object_store().map_err(err_json)?;
    let mut ids = Vec::new();
    for blob in &body.blobs {
        let content = base64::engine::general_purpose::STANDARD
            .decode(&blob.content)
            .map_err(err_json)?;
        let id = store.put_blob(&content).await.map_err(err_json)?;
        ids.push(id.0);
    }
    Ok(Json(UploadResult { ids }))
}

pub async fn get_blob(
    State(state): State<AppState>,
    Path(hash): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    validate_object_hash_id(&hash)?;
    let store = state.object_store().map_err(err_json)?;
    match store.get_blob(&BlobId(hash)).await {
        Ok(data) => {
            let encoded = base64::engine::general_purpose::STANDARD.encode(&data);
            Ok(Json(serde_json::json!({ "content": encoded })))
        }
        Err(e) if is_object_not_found(&e) => Err(not_found_json("blob not found")),
        Err(e) => Err(err_json(e)),
    }
}

#[derive(Deserialize)]
pub struct UploadTreesRequest {
    pub trees: Vec<TreeUpload>,
}

#[derive(Deserialize)]
pub struct TreeUpload {
    pub entries: serde_json::Value,
}

pub async fn upload_trees(
    State(state): State<AppState>,
    Json(body): Json<UploadTreesRequest>,
) -> Result<Json<UploadResult>, (StatusCode, Json<ApiError>)> {
    if body.trees.len() > 1000 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "too many trees in single request (max 1000)".to_string(),
            }),
        ));
    }
    let store = state.object_store().map_err(err_json)?;
    let mut ids = Vec::new();
    for tree in &body.trees {
        let entries: TreeEntries =
            serde_json::from_value(tree.entries.clone()).map_err(err_json)?;
        let id = store.put_tree(&entries).await.map_err(err_json)?;
        ids.push(id.0);
    }
    Ok(Json(UploadResult { ids }))
}

pub async fn get_tree(
    State(state): State<AppState>,
    Path(hash): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, Json<ApiError>)> {
    validate_object_hash_id(&hash)?;
    let store = state.object_store().map_err(err_json)?;
    match store.get_tree(&TreeId(hash)).await {
        Ok(entries) => match serde_json::to_value(&entries) {
            Ok(v) => Ok(Json(v)),
            Err(e) => Err(err_json(format!("TreeEntries serialization failed: {e}"))),
        },
        Err(e) if is_object_not_found(&e) => Err(not_found_json("tree not found")),
        Err(e) => Err(err_json(e)),
    }
}

pub async fn list_snapshots(
    State(state): State<AppState>,
    Query(page): Query<PaginationParams>,
) -> Result<Json<Vec<Snapshot>>, (StatusCode, Json<ApiError>)> {
    if page.limit == 0 || page.limit > 1000 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "limit must be between 1 and 1000".to_string(),
            }),
        ));
    }
    let store = state.snapshot_store().map_err(err_json)?;
    let mut snapshots = store.list_all().await.map_err(err_json)?;
    snapshots.sort_by_key(|b| std::cmp::Reverse(b.timestamp));
    let total = snapshots.len();
    let start = page.offset.min(total);
    let end = (start + page.limit).min(total);
    let page_data: Vec<Snapshot> = snapshots.into_iter().skip(start).take(end - start).collect();
    Ok(Json(page_data))
}

#[derive(Deserialize)]
pub struct CreateSnapshotRequest {
    pub snapshot: Snapshot,
}

pub async fn create_snapshot(
    State(state): State<AppState>,
    Json(body): Json<CreateSnapshotRequest>,
) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    let expected_id = content_addressed_snapshot_id_with_ts(
        &body.snapshot.tree_hash,
        &body.snapshot.parents,
        &body.snapshot.workspace,
        &body.snapshot.author,
        &body.snapshot.message,
        body.snapshot.timestamp,
    );
    if body.snapshot.id != expected_id {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: format!(
                    "snapshot id mismatch: expected {expected_id}, got {}",
                    body.snapshot.id
                ),
            }),
        ));
    }
    let store = state.snapshot_store().map_err(err_json)?;
    for parent in &body.snapshot.parents {
        if store.get(parent).await.is_err() {
            return Err(not_found_json(format!("parent snapshot not found: {parent}")));
        }
    }
    store.store(&body.snapshot).await.map_err(err_json)?;
    Ok(StatusCode::CREATED)
}

pub async fn list_workspaces(
    State(state): State<AppState>,
    Query(page): Query<PaginationParams>,
) -> Result<Json<Vec<Workspace>>, (StatusCode, Json<ApiError>)> {
    if page.limit == 0 || page.limit > 1000 {
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ApiError {
                error: "limit must be between 1 and 1000".to_string(),
            }),
        ));
    }
    let mgr = state.workspace_manager().map_err(err_json)?;
    let mut workspaces = mgr.list().await.map_err(err_json)?;
    workspaces.sort_by_key(|b| std::cmp::Reverse(b.updated_at));
    let start = page.offset.min(workspaces.len());
    let end = (start + page.limit).min(workspaces.len());
    let page_data: Vec<Workspace> = workspaces
        .into_iter()
        .skip(start)
        .take(end - start)
        .collect();
    Ok(Json(page_data))
}

#[derive(Deserialize)]
pub struct CreateWorkspaceRequest {
    pub workspace: Workspace,
}

pub async fn create_workspace(
    State(state): State<AppState>,
    Json(body): Json<CreateWorkspaceRequest>,
) -> Result<StatusCode, (StatusCode, Json<ApiError>)> {
    let mgr = state.workspace_manager().map_err(err_json)?;
    match mgr.create(&body.workspace).await {
        Ok(()) => Ok(StatusCode::CREATED),
        Err(e) if is_workspace_already_exists(&e) => Err((
            StatusCode::CONFLICT,
            Json(ApiError {
                error: "workspace already exists".to_string(),
            }),
        )),
        Err(e) => Err(err_json(e)),
    }
}
