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

const GIT_HTTP_BACKEND: &str = "/usr/lib/git-core/git-http-backend";

/// Pack negotiation is CPU-heavy; cap concurrent backend processes (the
/// /git middleware bypass skips the API rate limiter — Round A R2 S2).
static GIT_BACKEND_SEMAPHORE: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
/// Wall-clock bound per backend exchange: a stalled/hung child must not
/// hold a semaphore slot forever.
const GIT_BACKEND_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(60);

/// Suffixes git-http-backend may be invoked with. Everything else is
/// refused in-handler — path validation must not rest on git's own
/// `enter_repo` guard (Round A R2 S3).
fn suffix_allowed(suffix: &str) -> bool {
    matches!(
        suffix,
        "info/refs" | "git-upload-pack" | "HEAD" | "objects/info/packs" | "info/alternates"
    ) || suffix.starts_with("objects/")
}

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

/// Runs `git-http-backend` for one smart-HTTP exchange and converts its
/// output (CGI headers + body) into an axum `Response`.
async fn run_backend(
    state: &AppState,
    repo: &str,
    suffix: &str,
    method: &str,
    query: Option<&str>,
    headers: &HeaderMap,
    body: Option<Vec<u8>>,
) -> Result<Response, anyhow::Error> {
    // Only the read direction is served in W2a — refuse push BEFORE the
    // scratch-existence check so the answer never depends on mirror state.
    if suffix.contains("receive-pack") {
        return Ok(json_error_response(
            StatusCode::FORBIDDEN,
            "push is not served yet — noa becomes write-authority in a later slice",
        ));
    }
    if !suffix_allowed(suffix) {
        return Ok(json_error_response(
            StatusCode::BAD_REQUEST,
            format!("unsupported git http path '{suffix}'"),
        ));
    }
    let project_root = state.mirror_config.scratch_dir.clone();
    if !project_root.join(format!("{repo}.git")).exists() {
        return Ok(json_error_response(
            StatusCode::NOT_FOUND,
            format!("no mirrored repo '{repo}' — create a mirror job first"),
        ));
    }

    let mut cmd = tokio::process::Command::new(GIT_HTTP_BACKEND);
    cmd.env_clear()
        .env("PATH", "/usr/bin:/usr/local/bin")
        .env("GIT_PROJECT_ROOT", &project_root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("PATH_INFO", format!("/{repo}.git/{suffix}"))
        .env("REQUEST_METHOD", method)
        .env("REMOTE_ADDR", "127.0.0.1")
        .env("GIT_PROTOCOL", "")
        .env("CONTENT_TYPE", "")
        .env("QUERY_STRING", "");
    if let Some(q) = query {
        cmd.env("QUERY_STRING", q);
    }
    if let Some(ct) = headers
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
    {
        cmd.env("CONTENT_TYPE", ct);
    }
    if let Some(gp) = headers.get("Git-Protocol").and_then(|v| v.to_str().ok()) {
        cmd.env("GIT_PROTOCOL", gp);
    }
    // kill_on_drop: a child abandoned by the 60s timeout below must die,
    // not linger as an orphan holding .git locks (Round A S2).
    cmd.kill_on_drop(true);
    cmd.stdin(Stdio::piped()).stdout(Stdio::piped());
    if body.is_some() {
        // Capture POST stderr (upload-pack negotiation failures) into the
        // tracing buffer instead of dropping it.
        cmd.stderr(Stdio::piped());
    }

    let mut child = cmd.spawn()?;
    // The WHOLE exchange — writing the request body to the child's stdin
    // (a stalled child that never drains a 50 MiB body must not hold the
    // semaphore forever, Round A S2) and reading its output — lives under
    // one wall-clock deadline. Dropping stdin when the write finishes is
    // what signals EOF (CONTENT_LENGTH is not set).
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
        .map_err(|_| anyhow::anyhow!("git http backend timed out after 60s"))??;

    if !output.status.success() && output.stdout.is_empty() {
        tracing::error!(
            "git http backend crashed ({}) for {repo}/{suffix}",
            output.status
        );
        return Ok(json_error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            "git http backend crashed",
        ));
    }

    // CGI: headers, blank line, body. git-http-backend emits
    // `Status: <code>` + Content-Type for smart responses.
    let raw = &output.stdout;
    let (head, payload) = split_cgi(raw);
    let status = head
        .iter()
        .find_map(|l| l.strip_prefix("Status: ").map(ToString::to_string))
        .unwrap_or_else(|| "200".to_string());
    let code: StatusCode = status
        .split(' ')
        .next()
        .and_then(|s| s.parse().ok())
        .unwrap_or(StatusCode::OK);
    let content_type = head
        .iter()
        .find_map(|l| l.strip_prefix("Content-Type: ").map(ToString::to_string))
        .unwrap_or_else(|| "application/octet-stream".to_string());

    Ok(Response::builder()
        .status(code)
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(payload.to_vec()))
        .expect("static response parts"))
}

fn split_cgi(raw: &[u8]) -> (Vec<String>, Vec<u8>) {
    let sep = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|p| (p, 4))
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n").map(|p| (p, 2)));
    match sep {
        Some((pos, len)) => {
            let head = String::from_utf8_lossy(&raw[..pos])
                .lines()
                .map(|l| l.trim_end_matches('\r').to_string())
                .collect();
            (head, raw[pos + len..].to_vec())
        }
        None => (Vec::new(), raw.to_vec()),
    }
}

fn json_error_response(code: StatusCode, msg: impl Into<String>) -> Response {
    Response::builder()
        .status(code)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(
            serde_json::to_vec(&ApiError { error: msg.into() }).unwrap_or_default(),
        ))
        .unwrap()
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
    let headers = req.headers().clone();
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
        &headers,
        body,
    )
    .await
    {
        Ok(resp) => resp,
        Err(e) => {
            tracing::error!("git http backend failed: {e:#}");
            json_error_response(StatusCode::INTERNAL_SERVER_ERROR, "git http backend failed")
        }
    }
}
