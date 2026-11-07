//! Read-only git smart-HTTP serving (W2a) — proxies `git-http-backend`
//! against the mirror scratch clones, so a stock `git clone/fetch` can read
//! `http://server:port/git/<job>` with no noa-specific tooling.
//!
//! Scope (deliberate):
//! - **upload-pack + receive-pack** (clone/fetch/push — the W2a2
//!   authority-flip wave: pushes land in the same scratch clone the read
//!   path serves, and the mirror wave + back-flow lane close the loop).
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

/// Extracts the raw credential git presented: Basic password or Bearer
/// token (both carry the same secret in our scheme — username is ignored).
fn git_credential(headers: &HeaderMap) -> Option<String> {
    let v = headers.get(header::AUTHORIZATION)?.to_str().ok()?;
    if let Some(pass) = v.strip_prefix("Basic ") {
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(pass)
            .ok()?;
        let creds = String::from_utf8(decoded).ok()?;
        return creds
            .split_once(':')
            .map(|(_, password)| password.to_string());
    }
    v.strip_prefix("Bearer ").map(ToString::to_string)
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
    headers: &HeaderMap,
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
    // W2b: the repo catalog self-heals from clone traffic — a served repo
    // that somehow lacks a catalog entry gets one (origin=clone-served).
    // W2b: the repo catalog self-heals from clone traffic — a served repo
    // that lacks a catalog entry gets one (origin=clone-served). Uses the
    // AppState-cached registry (no per-request write txn for table setup);
    // write fires only on a catalog miss (R2 S1).
    if matches!(state.repo_registry.get(repo).await, Ok(None)) {
        let _ = state
            .repo_registry
            .register(repo, None, None, "clone-served")
            .await;
    }

    let wants_advertisement = suffix == "info/refs"
        && query
            .map(|q| {
                q.contains("service=git-upload-pack") || q.contains("service=git-receive-pack")
            })
            .unwrap_or(false);
    let is_post_pack = method == "POST" && suffix == "git-upload-pack";
    let is_post_receive = method == "POST" && suffix == "git-receive-pack";
    if !wants_advertisement && !is_post_pack && !is_post_receive {
        return Ok(json_error_response(
            StatusCode::BAD_REQUEST,
            format!("unsupported git http path '{suffix}' — smart HTTP only"),
        ));
    }

    // Backend selection: upload-pack serves reads; receive-pack serves
    // pushes into the SAME scratch clone (the mirror wave closes the loop —
    // importing the advanced scratch refs; the back-flow lane pushes them
    // on to GitHub). Both are bare `git` subcommands with --stateless-rpc.
    let backend = if is_post_receive
        || wants_advertisement
            && query
                .map(|q| q.contains("service=git-receive-pack"))
                .unwrap_or(false)
    {
        "receive-pack"
    } else {
        "upload-pack"
    };
    let mut cmd = tokio::process::Command::new("git");
    // kill_on_drop: a child abandoned by the 60s deadline below must die,
    // not linger as an orphan holding .git locks (Round A S2).
    cmd.kill_on_drop(true);
    cmd.arg(backend).arg("--stateless-rpc");
    if wants_advertisement {
        cmd.arg("--advertise-refs");
    }
    cmd.arg(&repo_path);
    // Deterministic env: no ambient GIT_*/proxy/config leakage into the
    // served refs (base had env_clear; Round A S3).
    cmd.env_clear();
    cmd.env("PATH", "/usr/bin:/usr/local/bin");
    // Protocol negotiation: honour the CLIENT's Git-Protocol header
    // (gitprotocol-v2 forbids the server from speaking v2 unrequested).
    // Empty default = v0. This is the env git-http-backend was dropping
    // on the POST path (root cause of #140).
    cmd.env(
        "GIT_PROTOCOL",
        headers
            .get("Git-Protocol")
            .and_then(|v| v.to_str().ok())
            .unwrap_or(""),
    );
    // Both pack POSTs carry the client's stream on stdin (S0: receive-pack
    // with a null stdin reads EOF and silently lands nothing).
    cmd.stdin(if is_post_pack || is_post_receive {
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

    // Smart advertisement MUST carry the `# service=` preamble + flush
    // (git-http-backend emitted it; raw upload-pack --advertise-refs does
    // not — D2 S1-class). Only the advertisement path; RPC responses carry
    // no preamble. Emitted as v0-shaped lines; a v2 client (GIT_PROTOCOL
    // forwarded) skips them per gitprotocol-v2.
    let stdout = if wants_advertisement {
        let service = if backend == "receive-pack" {
            "git-receive-pack"
        } else {
            "git-upload-pack"
        };
        // pkt-line length header = 4 (header bytes) + payload length — the
        // hardcoded upload-pack value broke receive-pack advertisements
        // ("bad line length character").
        let payload = format!("# service={service}\n");
        let preamble = format!("{:04x}{}0000", payload.len() + 4, payload);
        let mut full = preamble.into_bytes();
        full.extend_from_slice(&output.stdout);
        full
    } else {
        output.stdout
    };
    let content_type = if wants_advertisement {
        if backend == "receive-pack" {
            "application/x-git-receive-pack-advertisement".to_string()
        } else {
            "application/x-git-upload-pack-advertisement".to_string()
        }
    } else if backend == "receive-pack" {
        "application/x-git-receive-pack-result".to_string()
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
        .body(Body::from(stdout))
        .expect("static response parts"))
}

/// Entry point for every `/git/{repo}/*` request.
pub async fn handle_git_http(State(state): State<AppState>, req: Request) -> Response {
    // W2c: the credential resolves to the bootstrap identity (NOA_API_TOKEN)
    // or a registered user token — any of them may read. Pushes are refused
    // downstream regardless of identity (write direction is a later slice).
    // W2a2: the push path needs can_write — resolve the credential to an
    // identity (bootstrap token compares directly; user tokens via the
    // registry) and carry the WRITE permission separately from the read
    // authorization, so a reader-role user cannot push through /git
    // (the REST write role's git-direction twin).
    let mut can_write = false;
    let authorized = match git_credential(req.headers()) {
        // Empty credential vs empty configured token compares equal — a
        // server started with `NOA_API_TOKEN=""` must NOT become anonymous
        // readable (S1: the pre-W2c guard was lost in the refactor).
        Some(token) if !token.is_empty() && !state.api_token.is_empty() => {
            if crate::server::constant_time_eq(token.as_bytes(), state.api_token.as_bytes()) {
                can_write = true;
                true
            } else if let Some(identity) = state.user_registry.resolve(&token).await {
                can_write = identity.can_write();
                true
            } else {
                false
            }
        }
        _ => false,
    };
    if !authorized {
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
    // One acquisition covers the whole request including the spawned
    // upload-pack child; run_backend deliberately does not re-acquire.
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
    // Git compresses larger request bodies (`Content-Encoding: gzip` — seen
    // on the real noa.git fetch and the reason a small-repo e2e was green
    // while real clones 500'd): decompress before handing to upload-pack.
    let body = if method == "POST" {
        let is_gzip = req
            .headers()
            .get(header::CONTENT_ENCODING)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|v| v.eq_ignore_ascii_case("gzip"));
        match axum::body::to_bytes(req.into_body(), 50 * 1024 * 1024).await {
            Ok(b) if is_gzip => {
                // Decompression-bomb guard (R2 S2): cap the inflated size —
                // gzip ratios above 1000:1 mean a 50 MiB body can claim GBs.
                // MultiGzDecoder also fails loudly on trailing garbage
                // instead of silently truncating (single-member GzDecoder
                // returns Ok(0) mid-stream).
                const PLAIN_MAX: u64 = 200 * 1024 * 1024;
                let mut decoder = flate2::read::MultiGzDecoder::new(&b[..]).take(PLAIN_MAX + 1);
                use std::io::Read;
                let mut plain = Vec::with_capacity(b.len());
                if let Err(e) = decoder.read_to_end(&mut plain) {
                    return json_error_response(
                        StatusCode::BAD_REQUEST,
                        format!("gzip request body decode failed: {e}"),
                    );
                }
                if plain.len() as u64 > PLAIN_MAX {
                    return json_error_response(
                        StatusCode::BAD_REQUEST,
                        format!("decompressed body too large: {} > {PLAIN_MAX}", plain.len()),
                    );
                }
                Some(plain)
            }
            Ok(b) => Some(b.to_vec()),
            Err(_) => return json_error_response(StatusCode::BAD_REQUEST, "body read failed"),
        }
    } else {
        None
    };
    // W2a2 (authority-flip wave): receive-pack (push) is served — the
    // scratch clone is the write target, and the mirror wave closes the
    // loop. Pushing requires an authenticated identity (the same bar as
    // the REST write role); reads stay any-identity.
    let is_receive_request = git_path.suffix.contains("receive-pack") && method == "POST"
        || query
            .as_deref()
            .map(|q| q.contains("service=git-receive-pack"))
            .unwrap_or(false);
    if is_receive_request {
        if !authorized {
            return json_error_response(StatusCode::UNAUTHORIZED, "push requires authentication");
        }
        if !can_write {
            return json_error_response(StatusCode::FORBIDDEN, "push requires the write role");
        }
    }
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
            tracing::error!("git backend failed: {e:#}");
            json_error_response(StatusCode::INTERNAL_SERVER_ERROR, "git backend failed")
        }
    }
}
