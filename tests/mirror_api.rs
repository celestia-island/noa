//! Integration tests for the W1 mirror lane API surface: job CRUD, a
//! triggered run against a local `file://` origin, and the resulting
//! namespaced refs. The engine-level tests (fidelity, parent chaining,
//! idempotence) live in `src/server/mirror.rs`.

use std::{path::Path, process::Command, sync::Arc, time::Duration};

use axum::{
    body::Body,
    http::{Method, Request, StatusCode},
};
use tower::ServiceExt;

use libnoa::server::{router, AppState};

const TEST_API_TOKEN: &str = "test-token-for-ci";

fn git(cwd: &Path, args: &[&str]) {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "noa-test")
        .env("GIT_AUTHOR_EMAIL", "t@noa.local")
        .env("GIT_COMMITTER_NAME", "noa-test")
        .env("GIT_COMMITTER_EMAIL", "t@noa.local")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
}

struct Fixture {
    _tmp: tempfile::TempDir,
    app: axum::Router,
    origin_url: String,
    scratch: std::path::PathBuf,
}

async fn make_fixture() -> Fixture {
    let tmp = tempfile::tempdir().unwrap();
    let origin = tmp.path().join("origin.git");
    git(
        tmp.path(),
        &[
            "init",
            "--bare",
            "--initial-branch=master",
            origin.to_str().unwrap(),
        ],
    );
    let work = tmp.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    git(&work, &["clone", origin.to_str().unwrap(), "."]);
    std::fs::write(work.join("hello.txt"), b"mirror lane\n").unwrap();
    git(&work, &["add", "-A"]);
    git(&work, &["commit", "-m", "first"]);
    git(&work, &["push", "origin", "master"]);

    let db_dir = tempfile::TempDir::new().unwrap();
    let db = Arc::new(
        redb::Database::builder()
            .create(db_dir.path().join("mirror-api.redb"))
            .unwrap(),
    );
    let mirror_cfg = libnoa::server::mirror::MirrorConfig {
        interval_secs: 900,
        scratch_dir: tmp.path().join("scratch"),
        token_env: String::new(),
        github_api_base: String::new(),
    };
    let state = AppState::new(db)
        .with_api_token(TEST_API_TOKEN.to_string())
        .with_mirror_config(mirror_cfg.clone());
    Fixture {
        _tmp: tmp,
        app: router(state),
        origin_url: format!("file://{}", origin.display()),
        scratch: mirror_cfg.scratch_dir,
    }
}

fn req(method: Method, uri: &str, body: Option<String>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("Authorization", format!("Bearer {TEST_API_TOKEN}"));
    if let Some(b) = body {
        builder = builder.header("content-type", "application/json");
        builder.body(Body::from(b)).unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
    }
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

async fn wait_job_ok(app: &axum::Router, name: &str) -> serde_json::Value {
    for _ in 0..100 {
        let resp = app
            .clone()
            .oneshot(req(Method::GET, "/api/v1/mirror/jobs", None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let jobs = body_json(resp).await;
        let job = jobs
            .as_array()
            .unwrap()
            .iter()
            .find(|j| j["name"] == name)
            .expect("job present");
        if job["last_status"] == "ok" || job["last_status"] == "error" {
            return job.clone();
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    panic!("mirror job never finished");
}

#[tokio::test]
async fn test_job_lifecycle_and_run() {
    let f = make_fixture().await;
    let create = serde_json::json!({
        "name": "pilot",
        "remote_url": f.origin_url,
        "github_slug": "celestia-island/noa",
        "branches": ["master"],
        "enabled": true,
    });
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/mirror/jobs",
            Some(create.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED, "create must succeed");

    // Unauthenticated requests are rejected on mirror routes too.
    let noauth = Request::builder()
        .method(Method::GET)
        .uri("/api/v1/mirror/jobs")
        .body(Body::empty())
        .unwrap();
    let resp = f.app.clone().oneshot(noauth).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/mirror/run",
            Some(serde_json::json!({ "job": "pilot" }).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);

    let job = wait_job_ok(&f.app, "pilot").await;
    assert_eq!(job["last_status"], "ok", "job detail: {job}");
    assert!(
        job["last_detail"].as_str().unwrap().contains("fidelity ok"),
        "job detail: {job}"
    );

    // The replica is visible through the public refs API.
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/refs", None))
        .await
        .unwrap();
    let refs = body_json(resp).await;
    let names: Vec<&str> = refs
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"mirror/pilot/master"),
        "refs after run: {names:?}"
    );

    // Delete removes the job and its scratch clone (R2 S1/S2: a reused name
    // must not inherit the dead clone's origin).
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::DELETE, "/api/v1/mirror/jobs/pilot", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/mirror/jobs", None))
        .await
        .unwrap();
    assert_eq!(body_json(resp).await.as_array().unwrap().len(), 0);
    assert!(
        !f.scratch.join("pilot.git").exists(),
        "DELETE must remove the scratch clone"
    );
}

#[tokio::test]
async fn test_create_rejects_bad_jobs() {
    let f = make_fixture().await;
    for bad in [
        serde_json::json!({
            "name": "Bad Name",
            "remote_url": f.origin_url,
            "github_slug": "a/b",
            "branches": ["master"],
            "enabled": true,
        }),
        serde_json::json!({
            "name": "x",
            "remote_url": "http://plain.example/r.git",
            "github_slug": "a/b",
            "branches": ["master"],
            "enabled": true,
        }),
        serde_json::json!({
            "name": "x",
            "remote_url": "https://tok:tok@example/r.git",
            "github_slug": "a/b",
            "branches": ["master"],
            "enabled": true,
        }),
        serde_json::json!({
            "name": "x",
            "remote_url": f.origin_url,
            "github_slug": "a/b",
            "branches": [],
            "enabled": true,
        }),
    ] {
        let resp = f
            .app
            .clone()
            .oneshot(req(
                Method::POST,
                "/api/v1/mirror/jobs",
                Some(bad.to_string()),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "body: {bad}");
    }
}

// --- W2a: git smart-HTTP clone over the mirror scratch clones ---

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_git_http_clone_end_to_end() {
    let f = make_fixture().await;
    // Create + run the job so the scratch clone exists and is current.
    let create = serde_json::json!({
        "name": "pilot",
        "remote_url": f.origin_url,
        "github_slug": "celestia-island/noa",
        "branches": ["master"],
        "enabled": true,
    });
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/mirror/jobs",
            Some(create.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/mirror/run",
            Some(serde_json::json!({ "job": "pilot" }).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    let job = wait_job_ok(&f.app, "pilot").await;
    assert_eq!(job["last_status"], "ok", "{job}");

    // Serve the router on a real TCP listener so a stock git client can
    // hit it (smart HTTP needs a live socket, not oneshot).
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, f.app).await.unwrap() });

    let url = format!("http://mirror:{TEST_API_TOKEN}@{addr}/git/pilot");
    let dest = f._tmp.path().join("clone-out");
    let out = Command::new("git")
        .args(["clone", &url, dest.to_str().unwrap()])
        .current_dir("/tmp")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_AUTHOR_NAME", "noa-test")
        .env("GIT_AUTHOR_EMAIL", "t@noa.local")
        .env("GIT_COMMITTER_NAME", "noa-test")
        .env("GIT_COMMITTER_EMAIL", "t@noa.local")
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "clone failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let cloned = std::fs::read_to_string(dest.join("hello.txt")).unwrap();
    assert_eq!(cloned, "mirror lane\n");

    // Incremental fetch against the same clone must also work.
    git(&dest, &["fetch", "origin"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_git_http_accepts_gzip_body() {
    // Real git clients compress larger POST bodies (Content-Encoding: gzip
    // seen on a real noa.git fetch) — the handler must decompress before
    // handing the body to upload-pack (W2a2).
    let f = make_fixture().await;
    let create = serde_json::json!({
        "name": "pilot",
        "remote_url": f.origin_url,
        "github_slug": "celestia-island/noa",
        "branches": ["master"],
        "enabled": true,
    });
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/mirror/jobs",
            Some(create.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/mirror/run",
            Some(serde_json::json!({ "job": "pilot" }).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::ACCEPTED);
    let job = wait_job_ok(&f.app, "pilot").await;
    assert_eq!(job["last_status"], "ok", "{job}");

    // A gzip-compressed fetch body (v0 shape, small enough to handcraft).
    use base64::Engine;
    let creds =
        base64::engine::general_purpose::STANDARD.encode(format!("mirror:{TEST_API_TOKEN}"));
    let sha = "0000000000000000000000000000000000000000";
    let body_text = format!("want {sha} multi_ack_detailed side-band-64k\n");
    let mut body = format!("{:04x}", body_text.len() + 4).into_bytes();
    body.extend_from_slice(body_text.as_bytes());
    body.extend_from_slice(b"0000");
    body.extend_from_slice(b"0009done\n");

    let mut gz = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    use std::io::Write as _;
    gz.write_all(&body).unwrap();
    let gz_body = gz.finish().unwrap();

    let fetch = Request::builder()
        .method(Method::POST)
        .uri("/git/pilot/git-upload-pack")
        .header("authorization", format!("Basic {creds}"))
        .header("content-type", "application/x-git-upload-pack-request")
        .header("content-encoding", "gzip")
        .body(Body::from(gz_body))
        .unwrap();
    let resp = f.app.clone().oneshot(fetch).await.unwrap();
    // An all-zero want is rejected by upload-pack — the point is that the
    // request REACHES upload-pack (400/200/403-class, not 500 crash from
    // feeding gzip bytes as pkt-line).
    assert_ne!(
        resp.status(),
        StatusCode::INTERNAL_SERVER_ERROR,
        "gzip body must be decompressed before upload-pack"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_git_http_auth_and_404() {
    let f = make_fixture().await;

    // No credentials -> 401 with a Basic challenge.
    let noauth = Request::builder()
        .method(Method::GET)
        .uri("/git/pilot/info/refs?service=git-upload-pack")
        .body(Body::empty())
        .unwrap();
    let resp = f.app.clone().oneshot(noauth).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert!(
        resp.headers()
            .get("www-authenticate")
            .is_some_and(|v| v.to_str().unwrap().contains("Basic")),
        "401 must carry a Basic challenge"
    );

    use base64::Engine;
    let creds =
        base64::engine::general_purpose::STANDARD.encode(format!("mirror:{TEST_API_TOKEN}"));

    // Valid credentials + unknown repo -> 404 (no scratch clone).
    let ghost = Request::builder()
        .method(Method::GET)
        .uri("/git/ghost/info/refs?service=git-upload-pack")
        .header("authorization", format!("Basic {creds}"))
        .body(Body::empty())
        .unwrap();
    let resp = f.app.clone().oneshot(ghost).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // push must be refused outright (W2a is read-only).
    let push = Request::builder()
        .method(Method::POST)
        .uri("/git/pilot/git-receive-pack")
        .header("authorization", format!("Basic {creds}"))
        .header("content-type", "application/x-git-receive-pack-request")
        .body(Body::from(vec![0u8]))
        .unwrap();
    let resp = f.app.clone().oneshot(push).await.unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);
}
