use std::sync::Arc;

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use tower::ServiceExt;

use libnoa::server::{router, AppState};

async fn make_app() -> (tempfile::TempDir, axum::Router) {
    let tmp = tempfile::TempDir::new().unwrap();
    let db = Arc::new(
        redb::Database::builder()
            .create(tmp.path().join("server-test.redb"))
            .unwrap(),
    );
    let state = AppState::new(db);
    let app = router(state);
    (tmp, app)
}

fn make_request(method: Method, uri: &str, body: Option<String>) -> Request<Body> {
    let mut builder = Request::builder().method(method).uri(uri);
    if let Some(b) = body {
        builder = builder.header("content-type", "application/json");
        builder.body(Body::from(b)).unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    }
}

#[tokio::test]
async fn test_list_refs_empty() {
    let (_tmp, app) = make_app().await;
    let req = make_request(Method::GET, "/api/v1/refs", None);
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_push_ref() {
    let (_tmp, app) = make_app().await;
    let body = r#"{"name": "main", "id": "noa_test123"}"#.to_string();
    let req = make_request(Method::POST, "/api/v1/refs", Some(body));
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_upload_blob_and_get() {
    let (_tmp, app) = make_app().await;

    use base64::Engine;
    let content = base64::engine::general_purpose::STANDARD.encode(b"hello noa server");
    let upload_body = format!(r#"{{"blobs": [{{"content": "{}"}}]}}"#, content);
    let req = make_request(Method::POST, "/api/v1/blobs", Some(upload_body));
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body_bytes = axum::body::to_bytes(resp.into_body(), 1024 * 1024)
        .await
        .unwrap();
    let result: serde_json::Value = serde_json::from_slice(&body_bytes).unwrap();
    let hash = result["ids"][0].as_str().unwrap().to_string();
    assert!(!hash.is_empty());
}

#[tokio::test]
async fn test_get_blob_not_found() {
    let (_tmp, app) = make_app().await;
    let req = make_request(Method::GET, "/api/v1/blob/deadbeef00000000", None);
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_get_tree_not_found() {
    let (_tmp, app) = make_app().await;
    let req = make_request(Method::GET, "/api/v1/tree/deadbeef00000000", None);
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn test_list_workspaces() {
    let (_tmp, app) = make_app().await;
    let req = make_request(Method::GET, "/api/v1/workspaces", None);
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_list_snapshots() {
    let (_tmp, app) = make_app().await;
    let req = make_request(Method::GET, "/api/v1/snapshots", None);
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_create_workspace() {
    let (_tmp, app) = make_app().await;
    let body = r#"{"workspace": {"name": "test-ws", "head": "noa_base", "base": "noa_base", "agent_id": null, "last_seq": 0, "created_at": 1000, "updated_at": 1000}}"#.to_string();
    let req = make_request(Method::POST, "/api/v1/workspaces", Some(body));
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_upload_trees() {
    let (_tmp, app) = make_app().await;
    let body =
        r#"{"trees": [{"entries": [{"name": "main.rs", "kind": "Blob", "id": "hash123"}]}]}"#
            .to_string();
    let req = make_request(Method::POST, "/api/v1/trees", Some(body));
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn test_create_snapshot() {
    let (_tmp, app) = make_app().await;
    let body = r#"{"snapshot": {"id": "noa_snap001", "tree_hash": "tree123", "parents": [], "workspace": "default", "author": "test", "timestamp": 1000, "message": "test snapshot"}}"#.to_string();
    let req = make_request(Method::POST, "/api/v1/snapshots", Some(body));
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn test_upload_blobs_invalid_base64() {
    let (_tmp, app) = make_app().await;
    let body = r#"{"blobs": [{"content": "not-valid-base64!!!"}]}"#.to_string();
    let req = make_request(Method::POST, "/api/v1/blobs", Some(body));
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::INTERNAL_SERVER_ERROR);
}
