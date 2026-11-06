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
async fn test_repo_catalog_routes() {
    let f = make_fixture().await;

    // Register a repo manually.
    let register = serde_json::json!({
        "name": "pilot",
        "description": "the pilot repo",
        "default_branch": "master",
    });
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/repos",
            Some(register.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Detail returns the fields we set.
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/repos/pilot", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let detail = body_json(resp).await;
    assert_eq!(detail["name"], "pilot");
    assert_eq!(detail["default_branch"], "master");

    // Unknown repo → 404.
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/repos/ghost", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // A mirror job upsert auto-registers the repo entry (W2b lockstep).
    let job_create = serde_json::json!({
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
            Some(job_create.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/repos/pilot", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_repo_scoped_pr_routes() {
    let f = make_fixture().await;

    // Register the repo first.
    let register = serde_json::json!({"name": "pilot"});
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/repos",
            Some(register.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // base/head workspaces must exist (create_pr validates them) — use the
    // mirror-imported workspaces (a job run materialises mirror.pilot.master
    // and friends).
    let job_create = serde_json::json!({
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
            Some(job_create.to_string()),
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

    // Create a PR with repo="pilot" via the flat API.
    let create = serde_json::json!({
        "title": "pilot pr",
        "body": "context",
        "base": "mirror.pilot.master",
        "head": "mirror.pilot.master",
        "author": "tester",
        "repo": "pilot",
    });
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::POST, "/api/v1/prs", Some(create.to_string())))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED, "{:?}", resp);

    // A second PR in the default repo must NOT leak into the pilot scope.
    let create2 = serde_json::json!({
        "title": "default repo pr",
        "body": "context",
        "base": "mirror.pilot.master",
        "head": "mirror.pilot.master",
        "author": "tester",
    });
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::POST, "/api/v1/prs", Some(create2.to_string())))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Repo-scoped list shows exactly the pilot PR (scoping works).
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/repos/pilot/prs", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let prs = body_json(resp).await;
    assert_eq!(prs.as_array().unwrap().len(), 1);
    assert_eq!(prs[0]["repo"], "pilot");

    // Pagination honoured: limit=0 is rejected.
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/repos/pilot/prs?limit=0", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // A SECOND pilot PR so skip/take is actually regress-locked (with only
    // one PR a dropped limit/offset would look identical).
    let create3 = serde_json::json!({
        "title": "second pilot pr",
        "body": "context",
        "base": "mirror.pilot.master",
        "head": "mirror.pilot.master",
        "author": "tester",
        "repo": "pilot",
    });
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::POST, "/api/v1/prs", Some(create3.to_string())))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // limit=1 → exactly one record (a dropped take would return 2).
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/repos/pilot/prs?limit=1", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let one = body_json(resp).await;
    assert_eq!(one.as_array().unwrap().len(), 1);

    // offset=1 → one record, and a DIFFERENT one than limit=1 returned
    // (a dropped skip would return the same first record).
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::GET,
            "/api/v1/repos/pilot/prs?limit=1&offset=1",
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let page2 = body_json(resp).await;
    assert_eq!(page2.as_array().unwrap().len(), 1);
    assert_ne!(
        one[0]["number"], page2[0]["number"],
        "offset must skip the first record"
    );

    // Filters are wired, not just accepted: values that match nothing must
    // return [] (a dropped base/state wiring would return the 2 pilot PRs).
    // Positive AND negative per filter: the negative pins kill a dropped
    // wiring, the positive pins kill a SWAPPED one (base<->state are
    // adjacent same-typed Option<&str> args, so a swap compiles silently).
    for (q, want_len, why) in [
        ("?base=nope", 0, "base dropped"),
        ("?state=closed", 0, "state dropped"),
        ("?base=mirror.pilot.master", 2, "base swapped into state"),
        ("?state=open", 2, "state swapped into base"),
    ] {
        let path = format!("/api/v1/repos/pilot/prs{q}");
        let resp = f
            .app
            .clone()
            .oneshot(req(Method::GET, &path, None))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "{why}");
        let listed = body_json(resp).await;
        assert_eq!(
            listed.as_array().unwrap().len(),
            want_len,
            "filter mis-wired ({why})"
        );
    }

    // Flat route honours pagination too (same latent serde_urlencoded bug
    // would have 400'd any ?limit= request).
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/prs?limit=1", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let flat = body_json(resp).await;
    assert_eq!(flat.as_array().unwrap().len(), 1, "flat limit honoured");

    // Ghost repo → 404.
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/repos/ghost/prs", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);
}

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
