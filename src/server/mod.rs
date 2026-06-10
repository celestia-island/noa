mod handlers;

use axum::{
    extract::{ConnectInfo, Request},
    http::StatusCode,
    middleware::Next,
    response::Response,
    routing::{get, post},
    Router,
};
pub use handlers::AppState;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

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
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    {
        let key = req
            .headers()
            .get("x-forwarded-for")
            .or_else(|| req.headers().get("x-real-ip"))
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                req.extensions()
                    .get::<ConnectInfo<SocketAddr>>()
                    .map(|c| c.0.ip().to_string())
                    .unwrap_or_else(|| "default".to_string())
            });

        if !state.rate_limiter.check(&key).await {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
    }

    if state.api_token.is_empty() {
        tracing::warn!("NOA_API_TOKEN not set — rejecting all API requests");
        return Err(StatusCode::UNAUTHORIZED);
    }

    let auth_header = req
        .headers()
        .get("Authorization")
        .and_then(|v| v.to_str().ok());

    match auth_header {
        Some(val) if val.starts_with("Bearer ") => {
            let token = &val[7..];
            if constant_time_eq(token.as_bytes(), state.api_token.as_bytes()) {
                Ok(next.run(req).await)
            } else {
                Err(StatusCode::UNAUTHORIZED)
            }
        }
        _ => Err(StatusCode::UNAUTHORIZED),
    }
}

pub fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    let max_len = a.len().max(b.len());
    let mut result: u8 = 0;
    for i in 0..max_len {
        let x = a.get(i).copied().unwrap_or(0);
        let y = b.get(i).copied().unwrap_or(0);
        result |= x ^ y;
        std::hint::black_box(result);
    }
    let ok = result.wrapping_sub(1) >> 7;
    ok == 1
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
