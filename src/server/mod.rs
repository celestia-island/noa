mod handlers;

use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::Response,
    routing::{get, post},
    Router,
};
pub use handlers::AppState;

async fn auth_middleware(
    axum::extract::State(state): axum::extract::State<AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let expected = &state.api_token;
    if expected.is_empty() {
        return Ok(next.run(req).await);
    }

    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok());

    match auth_header {
        Some(val) if val.starts_with("Bearer ") => {
            let token = &val[7..];
            if token == expected {
                Ok(next.run(req).await)
            } else {
                Err(StatusCode::UNAUTHORIZED)
            }
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

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
        .layer(axum::extract::DefaultBodyLimit::max(50 * 1024 * 1024))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .with_state(state)
}
