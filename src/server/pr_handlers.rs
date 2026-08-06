//! HTTP handlers for the self-hosted PR API (P6#B2).

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Json,
};

use crate::{
    error::{is_snapshot_not_found, Result},
    forge::PrMetadata,
    merge::{extract_conflicts, merge_trees_recursive, ConflictResolution},
    object::{ObjectStore, TreeId},
    server::pr_store::{PrRecord, PrStore},
    snapshot::{
        content_addressed_snapshot_id_with_ts, RedbSnapshotStore, Snapshot, SnapshotId,
        SnapshotStore,
    },
};

use super::handlers::{AppState, PaginationParams};

#[derive(serde::Deserialize)]
pub struct CreatePrBody {
    pub title: String,
    #[serde(default)]
    pub body: String,
    pub base: String,
    pub head: String,
    pub author: String,
    #[serde(default)]
    pub repo: Option<String>,
    #[serde(default)]
    pub metadata: Option<PrMetadata>,
}

#[derive(serde::Deserialize)]
pub struct MergePrBody {
    #[serde(default)]
    pub squash: bool,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub struct ConflictResponse {
    pub error: String,
    pub conflicts: Vec<String>,
}

type ApiResult<T> = Result<Json<T>, (StatusCode, Json<serde_json::Value>)>;

fn err_json(msg: impl std::fmt::Display) -> (StatusCode, Json<serde_json::Value>) {
    tracing::error!("internal server error: {}", msg);
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(serde_json::json!({ "error": "internal server error" })),
    )
}

fn bad_request(msg: impl ToString) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::BAD_REQUEST,
        Json(serde_json::json!({ "error": msg.to_string() })),
    )
}

fn not_found(msg: impl ToString) -> (StatusCode, Json<serde_json::Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(serde_json::json!({ "error": msg.to_string() })),
    )
}

fn pr_store(state: &AppState) -> Result<PrStore> {
    PrStore::new(std::sync::Arc::clone(&state.db))
}

pub async fn create_pr(
    State(state): State<AppState>,
    Json(body): Json<CreatePrBody>,
) -> Result<(StatusCode, Json<PrRecord>), (StatusCode, Json<serde_json::Value>)> {
    if body.title.trim().is_empty() {
        return Err(bad_request("title must not be empty"));
    }
    if body.base.trim().is_empty() || body.head.trim().is_empty() {
        return Err(bad_request("base and head workspace names are required"));
    }

    let mgr = state.workspace_manager().map_err(err_json)?;
    let base_ws = mgr
        .get(&body.base)
        .await
        .map_err(err_json)?
        .ok_or_else(|| not_found(format!("base workspace not found: {}", body.base)))?;
    mgr.get(&body.head)
        .await
        .map_err(err_json)?
        .ok_or_else(|| not_found(format!("head workspace not found: {}", body.head)))?;

    let store = pr_store(&state).map_err(err_json)?;
    let record = store
        .create(PrRecord {
            number: 0,
            repo: body.repo.clone().unwrap_or_else(|| "default".to_string()),
            title: body.title,
            body: body.body,
            state: "open".to_string(),
            base: body.base,
            head: body.head,
            base_snapshot: base_ws.head.0.clone(),
            author: body.author,
            created_at: chrono::Utc::now().timestamp(),
            merge_snapshot: None,
            metadata: body.metadata,
        })
        .await
        .map_err(err_json)?;
    Ok((StatusCode::CREATED, Json(record)))
}

#[derive(serde::Deserialize)]
pub struct ListPrsParams {
    #[serde(flatten)]
    pub pagination: PaginationParams,
    pub repo: Option<String>,
    pub base: Option<String>,
    pub state: Option<String>,
}

pub async fn list_prs(
    State(state): State<AppState>,
    Query(params): Query<ListPrsParams>,
) -> ApiResult<Vec<PrRecord>> {
    if params.pagination.limit == 0 || params.pagination.limit > 1000 {
        return Err(bad_request("limit must be between 1 and 1000"));
    }
    let store = pr_store(&state).map_err(err_json)?;
    let mut records = store
        .list(
            params.repo.as_deref(),
            params.base.as_deref(),
            params.state.as_deref(),
        )
        .await
        .map_err(err_json)?;
    let start = params.pagination.offset.min(records.len());
    let end = (start + params.pagination.limit).min(records.len());
    records = records.into_iter().skip(start).take(end - start).collect();
    Ok(Json(records))
}

pub async fn get_pr(State(state): State<AppState>, Path(number): Path<u64>) -> ApiResult<PrRecord> {
    let store = pr_store(&state).map_err(err_json)?;
    match store.get(number).await.map_err(err_json)? {
        Some(record) => Ok(Json(record)),
        None => Err(not_found(format!("PR not found: #{number}"))),
    }
}

pub async fn merge_pr(
    State(state): State<AppState>,
    Path(number): Path<u64>,
    Json(body): Json<MergePrBody>,
) -> ApiResult<PrRecord> {
    let store = pr_store(&state).map_err(err_json)?;
    let record = store
        .get(number)
        .await
        .map_err(err_json)?
        .ok_or_else(|| not_found(format!("PR not found: #{number}")))?;

    if record.state != "open" {
        return Err(bad_request(format!(
            "PR #{number} is not open (state: {})",
            record.state
        )));
    }

    let mgr = state.workspace_manager().map_err(err_json)?;
    let base_ws = mgr
        .get(&record.base)
        .await
        .map_err(err_json)?
        .ok_or_else(|| not_found(format!("base workspace not found: {}", record.base)))?;
    let head_ws = mgr
        .get(&record.head)
        .await
        .map_err(err_json)?
        .ok_or_else(|| not_found(format!("head workspace not found: {}", record.head)))?;

    let snap_store: RedbSnapshotStore = state.snapshot_store().map_err(err_json)?;
    let object_store = state.object_store().map_err(err_json)?;

    let base_snap = match snap_store
        .get(&SnapshotId(record.base_snapshot.clone()))
        .await
    {
        Ok(s) => s,
        Err(e) if is_snapshot_not_found(&e) => {
            return Err(not_found(format!(
                "base snapshot not found: {}",
                record.base_snapshot
            )))
        }
        Err(e) => return Err(err_json(e)),
    };
    let ours_snap = match snap_store.get(&base_ws.head).await {
        Ok(s) => s,
        Err(e) if is_snapshot_not_found(&e) => {
            return Err(not_found(format!(
                "base head snapshot not found: {}",
                base_ws.head
            )))
        }
        Err(e) => return Err(err_json(e)),
    };
    let theirs_snap = match snap_store.get(&head_ws.head).await {
        Ok(s) => s,
        Err(e) if is_snapshot_not_found(&e) => {
            return Err(not_found(format!(
                "head snapshot not found: {}",
                head_ws.head
            )))
        }
        Err(e) => return Err(err_json(e)),
    };

    let base_tree = object_store
        .get_tree(&TreeId(base_snap.tree_hash.clone()))
        .await
        .map_err(err_json)?;
    let ours_tree = object_store
        .get_tree(&TreeId(ours_snap.tree_hash.clone()))
        .await
        .map_err(err_json)?;
    let theirs_tree = object_store
        .get_tree(&TreeId(theirs_snap.tree_hash.clone()))
        .await
        .map_err(err_json)?;

    let merged = merge_trees_recursive(
        base_tree,
        ours_tree,
        theirs_tree,
        object_store.clone(),
        &ConflictResolution::Ours,
    )
    .await
    .map_err(err_json)?;

    if merged.has_conflicts() {
        let conflicts: Vec<String> = extract_conflicts(&merged.output)
            .into_iter()
            .map(|c| c.path)
            .collect();
        return Err((
            StatusCode::CONFLICT,
            Json(
                serde_json::to_value(ConflictResponse {
                    error: format!("merge conflict in PR #{number}"),
                    conflicts,
                })
                .unwrap(),
            ),
        ));
    }

    let entries = merged.into_tree_entries(&ConflictResolution::Ours);
    let merged_tree_id = object_store.put_tree(&entries).await.map_err(err_json)?;

    let parents = if body.squash {
        vec![base_ws.head.clone()]
    } else {
        vec![base_ws.head.clone(), head_ws.head.clone()]
    };
    let timestamp = crate::now_micros();
    let message = format!("Merge PR #{number}: {}", record.title);
    let id = content_addressed_snapshot_id_with_ts(
        &merged_tree_id.0,
        &parents,
        &base_ws.name,
        &record.author,
        &message,
        timestamp,
    );
    let merged_snapshot = Snapshot {
        id: id.clone(),
        tree_hash: merged_tree_id.0,
        parents,
        workspace: base_ws.name.clone(),
        author: record.author.clone(),
        timestamp,
        message,
    };
    snap_store.store(&merged_snapshot).await.map_err(err_json)?;

    let mut updated_ws = base_ws.clone();
    updated_ws.head = merged_snapshot.id.clone();
    updated_ws.updated_at = timestamp;
    mgr.put(&updated_ws).await.map_err(err_json)?;

    let mut updated = record;
    updated.state = "merged".to_string();
    updated.merge_snapshot = Some(id.0);
    store.put(&updated).await.map_err(err_json)?;

    Ok(Json(updated))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use axum::{
        body::Body,
        http::{Method, Request},
        Router,
    };
    use tower::ServiceExt;

    use crate::server::{router, AppState};
    use crate::snapshot::content_addressed_snapshot_id_with_ts;

    const TOKEN: &str = "test-token-pr";

    async fn make_app() -> (tempfile::TempDir, Arc<redb::Database>, Router) {
        let _ = tracing_subscriber::fmt().try_init();
        let tmp = tempfile::TempDir::new().unwrap();
        let db = Arc::new(
            redb::Database::builder()
                .create(tmp.path().join("pr-test.redb"))
                .unwrap(),
        );
        let state = AppState::new(Arc::clone(&db)).with_api_token(TOKEN.to_string());
        let app = router(state);
        (tmp, db, app)
    }

    fn request(method: Method, uri: &str, body: Option<String>) -> Request<Body> {
        let mut builder = Request::builder()
            .method(method)
            .uri(uri)
            .header("Authorization", format!("Bearer {TOKEN}"));
        if let Some(b) = body {
            builder = builder.header("content-type", "application/json");
            builder.body(Body::from(b)).unwrap()
        } else {
            builder.body(Body::empty()).unwrap()
        }
    }

    fn tree_entries(path: &str, id: &str) -> serde_json::Value {
        serde_json::json!([{"name": path, "id": id, "kind": "Blob"}])
    }

    async fn upload_tree(app: &Router, entries: serde_json::Value) -> String {
        let body = format!(r#"{{"trees": [{{"entries": {}}}]}}"#, entries);
        let resp = app
            .clone()
            .oneshot(request(Method::POST, "/api/v1/trees", Some(body)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        json["ids"][0].as_str().unwrap().to_string()
    }

    /// Stores a snapshot + creates a workspace at it, both via the HTTP API.
    async fn seed_workspace(
        app: &Router,
        name: &str,
        tree_hash: &str,
        author: &str,
        msg: &str,
        timestamp: u64,
    ) -> String {
        let snap_id =
            content_addressed_snapshot_id_with_ts(tree_hash, &[], name, author, msg, timestamp);
        let snap_body = format!(
            r#"{{"snapshot": {{"id": "{}", "tree_hash": "{}", "parents": [], "workspace": "{}", "author": "{}", "timestamp": {}, "message": "{}"}}}}"#,
            snap_id, tree_hash, name, author, timestamp, msg
        );
        let resp = app
            .clone()
            .oneshot(request(Method::POST, "/api/v1/snapshots", Some(snap_body)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        let ws_body = format!(
            r#"{{"workspace": {{"name": "{}", "head": "{}", "base": "{}", "agent_id": null, "last_seq": 0, "created_at": {}, "updated_at": {}}}}}"#,
            name, snap_id, snap_id, timestamp, timestamp
        );
        let resp = app
            .clone()
            .oneshot(request(Method::POST, "/api/v1/workspaces", Some(ws_body)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        snap_id.0
    }

    async fn advance_workspace_head(
        db: &Arc<redb::Database>,
        name: &str,
        tree_hash: &str,
        parents: Vec<&str>,
        msg: &str,
        timestamp: u64,
    ) {
        let snap_store = AppState::new(Arc::clone(db)).snapshot_store().unwrap();
        let snap_id = content_addressed_snapshot_id_with_ts(
            tree_hash,
            &parents
                .iter()
                .map(|p| crate::snapshot::SnapshotId(p.to_string()))
                .collect::<Vec<_>>(),
            name,
            "lab",
            msg,
            timestamp,
        );
        snap_store
            .store(&crate::snapshot::Snapshot {
                id: snap_id.clone(),
                tree_hash: tree_hash.to_string(),
                parents: parents
                    .iter()
                    .map(|p| crate::snapshot::SnapshotId(p.to_string()))
                    .collect(),
                workspace: name.to_string(),
                author: "lab".to_string(),
                timestamp,
                message: msg.to_string(),
            })
            .await
            .unwrap();

        let mgr = AppState::new(Arc::clone(db)).workspace_manager().unwrap();
        let mut ws = mgr.get(name).await.unwrap().unwrap();
        ws.head = snap_id.clone();
        ws.updated_at = timestamp;
        mgr.put(&ws).await.unwrap();
    }

    #[tokio::test]
    async fn test_pr_crud_and_merge() {
        let (_tmp, _db, app) = make_app().await;

        let base_tree = upload_tree(&app, tree_entries("a.txt", "111")).await;
        let head_tree = upload_tree(
            &app,
            serde_json::json!([
                {"name": "a.txt", "id": "111", "kind": "Blob"},
                {"name": "b.txt", "id": "222", "kind": "Blob"}
            ]),
        )
        .await;
        seed_workspace(&app, "master", &base_tree, "lab", "base", 1000).await;
        seed_workspace(&app, "feat-x", &head_tree, "lab", "head", 1000).await;

        // create PR
        let create_body = serde_json::json!({
            "title": "✨ Add b.txt.",
            "body": "desc",
            "base": "master",
            "head": "feat-x",
            "author": "lab",
            "metadata": {"model": "deepseek/deepseek-chat", "input_tokens": 5}
        })
        .to_string();
        let resp = app
            .clone()
            .oneshot(request(Method::POST, "/api/v1/prs", Some(create_body)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(created["number"], 1);
        assert_eq!(created["state"], "open");
        assert_eq!(created["metadata"]["model"], "deepseek/deepseek-chat");

        // list
        let resp = app
            .clone()
            .oneshot(request(Method::GET, "/api/v1/prs?state=open", None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let listed: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(listed.as_array().unwrap().len(), 1);

        // merge (squash)
        let resp = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/api/v1/prs/1/merge",
                Some(r#"{"squash": true}"#.to_string()),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let merged: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(merged["state"], "merged");
        let merge_snapshot = merged["merge_snapshot"].as_str().unwrap().to_string();

        // base workspace advanced to the merged snapshot
        let resp = app
            .clone()
            .oneshot(request(Method::GET, "/api/v1/workspaces", None))
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let workspaces: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        let master = workspaces
            .as_array()
            .unwrap()
            .iter()
            .find(|w| w["name"] == "master")
            .unwrap();
        assert_eq!(master["head"], merge_snapshot);

        // merged tree contains b.txt
        let db_handle = AppState::new(Arc::clone(&_db));
        let snap = db_handle
            .snapshot_store()
            .unwrap()
            .get(&crate::snapshot::SnapshotId(merge_snapshot))
            .await
            .unwrap();
        let tree = db_handle
            .object_store()
            .unwrap()
            .get_tree(&TreeId(snap.tree_hash))
            .await
            .unwrap();
        let names: Vec<&str> = tree.0.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"a.txt"));
        assert!(names.contains(&"b.txt"));

        // re-merge must fail (not open)
        let resp = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/api/v1/prs/1/merge",
                Some(r#"{"squash": false}"#.to_string()),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn test_merge_conflict_returns_409() {
        let (_tmp, db, app) = make_app().await;

        // common ancestor: a.txt=111; both sides modify it differently after the PR
        let base_tree = upload_tree(&app, tree_entries("a.txt", "111")).await;
        let base_snap = seed_workspace(&app, "master", &base_tree, "lab", "base", 1000).await;
        seed_workspace(&app, "feat-x", &base_tree, "lab", "head-base", 1000).await;

        let create_body = serde_json::json!({
            "title": "✏️ Change a.txt.",
            "body": "",
            "base": "master",
            "head": "feat-x",
            "author": "lab"
        })
        .to_string();
        let resp = app
            .clone()
            .oneshot(request(Method::POST, "/api/v1/prs", Some(create_body)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // advance both workspaces in conflicting directions
        let ours_tree = upload_tree(&app, tree_entries("a.txt", "333")).await;
        let theirs_tree = upload_tree(&app, tree_entries("a.txt", "444")).await;
        advance_workspace_head(&db, "master", &ours_tree, vec![&base_snap], "ours", 2000).await;
        advance_workspace_head(
            &db,
            "feat-x",
            &theirs_tree,
            vec![&base_snap],
            "theirs",
            2000,
        )
        .await;

        let resp = app
            .clone()
            .oneshot(request(
                Method::POST,
                "/api/v1/prs/1/merge",
                Some(r#"{"squash": true}"#.to_string()),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CONFLICT);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let conflict: ConflictResponse = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(conflict.conflicts, vec!["a.txt"]);

        // PR stays open
        let resp = app
            .clone()
            .oneshot(request(Method::GET, "/api/v1/prs/1", None))
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let record: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(record["state"], "open");
    }

    #[tokio::test]
    async fn test_pr_requires_existing_workspaces() {
        let (_tmp, _db, app) = make_app().await;
        let create_body = serde_json::json!({
            "title": "t",
            "base": "missing",
            "head": "also-missing",
            "author": "lab"
        })
        .to_string();
        let resp = app
            .clone()
            .oneshot(request(Method::POST, "/api/v1/prs", Some(create_body)))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }
}
