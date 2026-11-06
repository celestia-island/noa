//! Read-only git smart-HTTP serving (W2a) — proxies `git-http-backend`
//! against the mirror scratch clones, so a stock `git clone/fetch` can read
//! `http://server:port/git/<job>` with no noa-specific tooling.
//!
//! Scope (deliberate):
//! - **upload-pack only** (clone/fetch). receive-pack (push) is a later
//!   slice: accepting pushes means accepting authority, which lands with
//!   the namespace + RBAC work (W2b/W2c).
//! - The served content is the **mirror** (scratch clones refreshed by the
//!   scheduler). It is not the noa object store; git objects and noa
//!   snapshots stay separate until the authority-flip wave.
//! - Bodies are buffered, not streamed: pilot repos are small; streaming a
//!   CGI child through axum needs a response-body channel that adds
//!   complexity for no pilot benefit. The buffer cap is the server's
//!   existing 50 MiB body limit.
//!
//! Auth: the `/git/` routes accept **Basic** auth (any username, password =
//! `NOA_API_TOKEN`) because stock git clients speak Basic, and the existing
//! **Bearer** token for parity with the rest of the API. Everything else —
//! rate limiting, constant-time compare — is shared with the main routes.

use std::process::Stdio;
use tokio::io::AsyncWriteExt;

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    response::Response,
};

use crate::server::handlers::{ApiError, AppState};

/// Pack negotiation is CPU-heavy; cap concurrent backend processes (the
/// /git middleware bypass skips the API rate limiter — Round A R2 S2).
static GIT_BACKEND_SEMAPHORE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
/// Wall-clock bound per backend exchange: a stalled/hung child must not
/// hold a semaphore slot forever.
const GIT_BACKEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// One validated `/git/{repo}/{suffix}` request, or the reason it is not
/// servable.
struct GitPath {
    repo: String,
    /// Path segment passed to git-http-backend as `PATH_INFO` suffix
    /// (e.g. `info/refs`, `git-upload-pack`).
    suffix: String,
}

fn parse_git_path(path: &str) -> Option<GitPath> {
    let rest = path.strip_prefix("/git/")?;
    let mut parts = rest.splitn(2, '/');
    let repo = parts.next()?.to_string();
    let suffix = parts.next()?.to_string();
    if repo.is_empty() || suffix.is_empty() || repo.contains("..") || repo.contains('/') {
        return None;
    }
    // Repo must match a mirror job's scratch clone exactly.
    if !repo
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return None;
    }
    Some(GitPath { repo, suffix })
}

/// Validates Basic auth (any user, password = token) or Bearer token
/// against the server token.
fn json_error_response(code: StatusCode, msg: impl Into<String>) -> Response {
    Response::builder()
        .status(code)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&ApiError { error: msg.into() }).unwrap_or_default(),
        ))
        .expect("static error parts")
}

fn git_auth_authorized(headers: &HeaderMap, state: &AppState) -> bool {
    let token = &state.api_token;
    if token.is_empty() {
        return false;
    }
    if let Some(v) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(pass) = v.strip_prefix("Basic ") {
            use base64::Engine;
            if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(pass) {
                if let Ok(creds) = String::from_utf8(decoded) {
                    if let Some((_, password)) = creds.split_once(':') {
                        return crate::server::constant_time_eq(
                            password.as_bytes(),
                            token.as_bytes(),
                        );
                    }
                }
            }
            return false;
        }
        if let Some(bearer) = v.strip_prefix("Bearer ") {
            return crate::server::constant_time_eq(bearer.as_bytes(), token.as_bytes());
        }
    }
    false
}

/// Serves one smart-HTTP exchange by invoking `git upload-pack` directly
/// (NOT via git-http-backend: the CGI wrapper's env whitelist drops
/// GIT_PROTOCOL on the POST path, silently downgrading to v0 and breaking
/// clones — found empirically on git 2.39.5). We own the child env, so
/// protocol v2 negotiation works end-to-end.
///
/// - GET  `<repo>/info/refs?service=git-upload-pack` → advertisement
/// - POST `<repo>/git-upload-pack`                   → pack response
/// - receive-pack (push) refused; dumb files not served (smart only).
async fn run_backend(
    state: &AppState,
    repo: &str,
    suffix: &str,
    method: &str,
    query: Option<&str>,
    body: Option<Vec<u8>>,
) -> Result<Response, anyhow::Error> {
    let project_root = state.mirror_config.scratch_dir.clone();
    let repo_path = project_root.join(format!("{repo}.git"));
    if !repo_path.exists() {
        return Ok(json_error_response(
            StatusCode::NOT_FOUND,
            format!("no mirrored repo '{repo}' — create a mirror job first"),
        ));
    }

    if suffix.contains("receive-pack") {
        return Ok(json_error_response(
            StatusCode::FORBIDDEN,
            "push is not served yet — noa becomes write-authority in a later slice",
        ));
    }

    let wants_advertisement = suffix == "info/refs"
        && query
            .map(|q| q.contains("service=git-upload-pack"))
            .unwrap_or(false);
    let is_post_pack = method == "POST" && suffix == "git-upload-pack";
    if !wants_advertisement && !is_post_pack {
        return Ok(json_error_response(
            StatusCode::BAD_REQUEST,
            format!("unsupported git http path '{suffix}' — smart HTTP only"),
        ));
    }

    // Bound concurrent backend processes (the /git middleware bypass skips
    // the API rate limiter — Round A R2 S2).
    let _permit = match GIT_BACKEND_SEMAPHORE.try_acquire() {
        Ok(p) => p,
        Err(_) => {
            return Ok(json_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "git http backend busy — retry shortly",
            ));
        }
    };

    let mut cmd = tokio::process::Command::new("git");
    // kill_on_drop: a child abandoned by the 60s deadline below must die,
    // not linger as an orphan holding .git locks (Round A S2).
    cmd.kill_on_drop(true);
    cmd.arg("upload-pack").arg("--stateless-rpc");
    if wants_advertisement {
        cmd.arg("--advertise-refs");
    }
    cmd.arg(&repo_path);
    // GIT_PROTOCOL must reach upload-pack itself — this is exactly the env
    // that git-http-backend was dropping.
    cmd.env("GIT_PROTOCOL", "version=2");
    cmd.stdin(if is_post_pack {
        Stdio::piped()
    } else {
        Stdio::null()
    });
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let mut child = cmd.spawn()?;
    // The WHOLE exchange — writing the request body to the child's stdin
    // (a stalled child that never drains a 50 MiB body must not hold the
    // semaphore forever, Round A S2) and reading its output — lives under
    // one wall-clock deadline. Dropping stdin when the write finishes is
    // what signals EOF (CONTENT_LENGTH is not set). On timeout the future
    // (and with it the child) is dropped → kill_on_drop fires.
    let io = async {
        if let Some(b) = body {
            if let Some(mut stdin) = child.stdin.take() {
                stdin.write_all(&b).await?;
                stdin.flush().await?;
            }
        }
        child.wait_with_output().await
    };
    let output = tokio::time::timeout(GIT_BACKEND_TIMEOUT, io)
        .await
        .map_err(|_| anyhow::anyhow!("git backend timed out after 60s"))??;

    if !output.status.success() && output.stdout.is_empty() {
        tracing::error!(
            "git backend crashed ({}) for {repo}/{suffix}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
        return Ok(json_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "git backend crashed",
        ));
    }

    let content_type = if wants_advertisement {
        "application/x-git-upload-pack-advertisement".to_string()
    } else {
        "application/x-git-upload-pack-result".to_string()
    };
    if !output.stderr.is_empty() {
        tracing::debug!(
            "git {repo}/{suffix}: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(output.stdout))
        .expect("static response parts"))
}

/// Entry point for every `/git/{repo}/*` request.
pub async fn handle_git_http(State(state): State<AppState>, req: Request) -> Response {
    if !git_auth_authorized(req.headers(), &state) {
        // Stock git clients only offer URL/credential-helper credentials
        // when the 401 carries a Basic challenge (RFC 7617).
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header(header::WWW_AUTHENTICATE, "Basic realm=\"noa\"")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                serde_json::to_vec(&ApiError {
                    error: "git http requires auth".into(),
                })
                .unwrap_or_default(),
            ))
            .expect("static 401 parts");
    }
    let path = req.uri().path().to_string();
    let Some(git_path) = parse_git_path(&path) else {
        return json_error_response(StatusCode::BAD_REQUEST, "malformed /git path");
    };
    let query = req.uri().query().map(ToString::to_string);
    let method = req.method().as_str().to_uppercase();
    // Acquire the backend permit BEFORE reading the body: the buffered body
    // is up to 50 MiB and the /git bypass skips the API rate limiter —
    // bounding concurrent reads bounds memory to 4×50 MiB (Round A S2).
    let _permit = match GIT_BACKEND_SEMAPHORE.try_acquire() {
        Ok(p) => p,
        Err(_) => {
            return json_error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "git http backend busy — retry shortly",
            );
        }
    };
    // Smart HTTP: POST bodies go to the backend's stdin; GET has no body.
    let body = if method == "POST" {
        match axum::body::to_bytes(req.into_body(), 50 * 1024 * 1024).await {
            Ok(b) => Some(b.to_vec()),
            Err(_) => return json_error_response(StatusCode::BAD_REQUEST, "body read failed"),
        }
    } else {
        None
    };
    match run_backend(
        &state,
        &git_path.repo,
        &git_path.suffix,
        &method,
        query.as_deref(),
        body,
    )
    .await
    {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!("git backend failed: {e:#}");
            json_error_response(StatusCode::INTERNAL_SERVER_ERROR, "git backend failed")
        }
    }
}
