pub mod backup;
pub mod git_http;
mod handlers;
pub mod mirror;
pub mod mirror_github;
pub mod mirror_store;
pub mod policy;
mod pr_handlers;
pub mod pr_store;
pub mod repo_registry;
pub mod run_registry;
pub mod snapshot_export;
pub mod user_registry;

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;

use axum::{
    extract::{ConnectInfo, Request},
    http::StatusCode,
    middleware::Next,
    response::Response,
    routing::{get, post},
    Router,
};
pub mod github_broker;
pub use handlers::AppState;

#[derive(Clone)]
struct RateLimitEntry {
    count: u32,
    window_start: Instant,
}

#[derive(Clone)]
pub struct RateLimiter {
    entries: Arc<Mutex<HashMap<String, RateLimitEntry>>>,
    max_requests: u32,
    window_secs: u64,
}

impl RateLimiter {
    pub fn new(max_requests: u32, window_secs: u64) -> Self {
        RateLimiter {
            entries: Arc::new(Mutex::new(HashMap::new())),
            max_requests,
            window_secs,
        }
    }

    fn is_expired(entry: &RateLimitEntry, window: Duration) -> bool {
        Instant::now().duration_since(entry.window_start) > window * 2
    }

    pub async fn check(&self, key: &str) -> bool {
        let mut entries = self.entries.lock().await;
        let now = Instant::now();
        let window = Duration::from_secs(self.window_secs);

        if entries.len() > 10000 {
            entries.retain(|_, entry| !Self::is_expired(entry, window));
        } else if entries.len() % 100 == 0 && !entries.is_empty() {
            entries.retain(|_, entry| !Self::is_expired(entry, window));
        }

        let entry = entries.entry(key.to_string()).or_insert(RateLimitEntry {
            count: 0,
            window_start: now,
        });

        if now.duration_since(entry.window_start) > window {
            entry.count = 0;
            entry.window_start = now;
        }

        entry.count += 1;
        entry.count <= self.max_requests
    }
}

async fn auth_middleware(
    axum::extract::State(state): axum::extract::State<AppState>,
    mut req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    // `/git/*` routes self-authorize (stock git clients speak Basic, not
    // Bearer) — the middleware would 401 them before handler logic runs.
    if req.uri().path().starts_with("/git/") {
        return Ok(next.run(req).await);
    }
    if state.api_token.is_empty() {
        tracing::warn!("NOA_API_TOKEN not set — rejecting all API requests");
        return Err(StatusCode::UNAUTHORIZED);
    }

    {
        // Rate limit FIRST (W2c review): identity resolution scans the user
        // table per attempt, so bad-token requests must be throttled too —
        // they are exactly the traffic an attacker can generate cheaply.
        let key = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip().to_string())
            .unwrap_or_else(|| {
                req.headers()
                    .get("x-forwarded-for")
                    .or_else(|| req.headers().get("x-real-ip"))
                    .and_then(|v| v.to_str().ok())
                    .map(|s| {
                        s.split(',')
                            .next()
                            .map(|s| s.trim().to_string())
                            .unwrap_or_default()
                    })
                    .unwrap_or_else(|| "unknown".to_string())
            });

        if !state.rate_limiter.check(&key).await {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
    }

    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok());

    // W2c: the bearer token resolves to EITHER the bootstrap identity
    // (NOA_API_TOKEN, role admin) or a registered user (role from the
    // user table). The resolved identity is inserted into the request
    // extensions; write/admin handlers extract it and enforce.
    let identity = match auth_header {
        Some(val) if val.starts_with("Bearer ") => {
            let token = &val[7..];
            if constant_time_eq(token.as_bytes(), state.api_token.as_bytes()) {
                Some(crate::server::user_registry::Identity::bootstrap())
            } else {
                state.user_registry.resolve(token).await
            }
        }
        _ => None,
    };

    let Some(identity) = identity else {
        return Err(StatusCode::UNAUTHORIZED);
    };

    // W2c role enforcement — centralized here so the policy is auditable in
    // one place. Reader can GET; operator+ can write; admin for user
    // management and destructive deletes.
    let method = req.method().as_str();
    let path = req.uri().path();
    let is_write = matches!(method, "POST" | "PUT" | "DELETE" | "PATCH");
    if is_write && !identity.can_write() {
        return Err(StatusCode::FORBIDDEN);
    }
    if path.starts_with("/api/v1/admin") && !identity.can_admin() {
        // The admin surface (backup/export) is admin-only entirely.
        return Err(StatusCode::FORBIDDEN);
    }
    if path.starts_with("/api/v1/users") && method != "GET" && !identity.can_admin() {
        return Err(StatusCode::FORBIDDEN);
    }
    let is_destructive_delete = method == "DELETE"
        && (path.starts_with("/api/v1/repos/") || path.starts_with("/api/v1/mirror/jobs/"));
    if is_destructive_delete && !identity.can_admin() {
        return Err(StatusCode::FORBIDDEN);
    }

    req.extensions_mut().insert(identity);

    Ok(next.run(req).await)
}

pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    use subtle::ConstantTimeEq;
    a.ct_eq(b).into()
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route(
            "/git/{repo}/{*suffix}",
            axum::routing::any(git_http::handle_git_http),
        )
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
        .route(
            "/api/v1/prs",
            get(pr_handlers::list_prs).post(pr_handlers::create_pr),
        )
        .route("/api/v1/prs/{number}", get(pr_handlers::get_pr))
        .route("/api/v1/prs/{number}/merge", post(pr_handlers::merge_pr))
        .route("/api/v1/prs/{number}/close", post(pr_handlers::close_pr))
        .route("/api/v1/github/config", get(github_broker::config))
        .route("/api/v1/github/token", post(github_broker::issue_token))
        .route(
            "/api/v1/github/installations",
            get(github_broker::installations),
        )
        .route(
            "/api/v1/users",
            get(user_registry::list_users).post(user_registry::create_user),
        )
        .route(
            "/api/v1/users/{name}",
            get(user_registry::get_user).delete(user_registry::delete_user),
        )
        .route(
            "/api/v1/admin/export",
            axum::routing::post(backup::export_handler),
        )
        .route(
            "/api/v1/repos",
            get(repo_registry::list_repos).post(repo_registry::register_repo),
        )
        .route(
            "/api/v1/repos/{name}",
            get(repo_registry::get_repo).delete(repo_registry::delete_repo),
        )
        .route("/api/v1/repos/{name}/prs", get(pr_handlers::list_repo_prs))
        .route(
            "/api/v1/repos/{name}/snapshots/{id}/export",
            axum::routing::post(snapshot_export::export_handler),
        )
        .route(
            "/api/v1/repos/{name}/runs",
            get(run_registry::list_runs).post(run_registry::record_run),
        )
        .route(
            "/api/v1/mirror/jobs",
            get(mirror::list_jobs).post(mirror::upsert_job),
        )
        .route(
            "/api/v1/mirror/jobs/{name}",
            axum::routing::delete(mirror::delete_job),
        )
        .route("/api/v1/mirror/run", post(mirror::run_jobs))
        .layer(axum::extract::DefaultBodyLimit::max(50 * 1024 * 1024))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constant_time_eq_equal() {
        assert!(constant_time_eq(b"hello", b"hello"));
        assert!(constant_time_eq(b"", b""));
        assert!(constant_time_eq(b"a", b"a"));
    }

    #[test]
    fn test_constant_time_eq_not_equal() {
        assert!(!constant_time_eq(b"hello", b"world"));
        assert!(!constant_time_eq(b"hello", b"hellp"));
    }

    #[test]
    fn test_constant_time_eq_different_lengths() {
        assert!(!constant_time_eq(b"short", b"longer"));
        assert!(!constant_time_eq(b"", b"a"));
        assert!(!constant_time_eq(b"a", b""));
    }

    #[test]
    fn test_constant_time_eq_single_bit_diff() {
        let a = b"test";
        let mut b = b"test".to_vec();
        b[3] ^= 1;
        assert!(!constant_time_eq(a, &b));
    }

    #[tokio::test]
    async fn test_rate_limiter_allows_within_limit() {
        let limiter = RateLimiter::new(3, 60);
        assert!(limiter.check("key").await);
        assert!(limiter.check("key").await);
        assert!(limiter.check("key").await);
        assert!(!limiter.check("key").await);
    }

    #[tokio::test]
    async fn test_rate_limiter_independent_keys() {
        let limiter = RateLimiter::new(1, 60);
        assert!(limiter.check("a").await);
        assert!(limiter.check("b").await);
        assert!(!limiter.check("a").await);
    }
}
