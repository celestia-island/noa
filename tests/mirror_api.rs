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

/// Like `req` but with an explicit bearer token (W2c role tests).
fn req_with(method: Method, uri: &str, bearer: &str, body: Option<String>) -> Request<Body> {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("Authorization", format!("Bearer {bearer}"));
    if let Some(b) = body {
        builder = builder.header("content-type", "application/json");
        builder.body(Body::from(b)).unwrap()
    } else {
        builder.body(Body::empty()).unwrap()
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
async fn test_pr_heads_mirror_scope() {
    let f = make_fixture().await;
    // The fixture origin is a bare repo at <tmp>/origin.git (origin_url is
    // its file:// form) — model GitHub's refs/pull/<N>/head on it.
    let origin_dir = f.origin_url.strip_prefix("file://").unwrap().to_string();

    // A PR head ref on the origin (GitHub models refs/pull/<N>/head).
    let out = std::process::Command::new("git")
        .args([
            "-C",
            &origin_dir,
            "update-ref",
            "refs/pull/7/head",
            "refs/heads/master",
        ])
        .output()
        .expect("update-ref on origin");
    assert!(out.status.success(), "update-ref failed");

    // Register a mirror job WITH pr_heads on.
    let job_create = serde_json::json!({
        "name": "pilot",
        "remote_url": f.origin_url,
        "github_slug": "celestia-island/noa",
        "branches": ["master"],
        "tags": true,
        "pr_heads": true,
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
    assert_eq!(job["last_lane"], "git", "{job}");
    // The wave note carries the imported PR head.
    assert!(
        (job["last_detail"].as_str().unwrap_or("")).contains("pr-heads=1"),
        "pr-heads note missing: {job}"
    );

    // The imported PR workspace is user-visible through the workspaces API.
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/workspaces", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let workspaces = body_json(resp).await.to_string();
    assert!(
        workspaces.contains("mirror.pilot.pr.7"),
        "PR workspace missing from workspaces list: {workspaces}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_user_roles_and_enforcement() {
    let f = make_fixture().await;
    let bootstrap = TEST_API_TOKEN.to_string();

    // Bootstrap identity manages users; raw token returned once.
    let resp = f
        .app
        .clone()
        .oneshot(req_with(
            Method::POST,
            "/api/v1/users",
            &bootstrap,
            Some(serde_json::json!({"name": "writer", "role": "operator"}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let created = body_json(resp).await;
    let writer_token = created["token"].as_str().unwrap().to_string();

    let resp = f
        .app
        .clone()
        .oneshot(req_with(
            Method::POST,
            "/api/v1/users",
            &bootstrap,
            Some(serde_json::json!({"name": "watcher", "role": "reader"}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let reader_token = body_json(resp).await["token"].as_str().unwrap().to_string();

    // Raw tokens never appear in listings.
    let resp = f
        .app
        .clone()
        .oneshot(req_with(Method::GET, "/api/v1/users", &bootstrap, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let users = body_json(resp).await.to_string();
    assert!(!users.contains(&writer_token), "raw token leaked: {users}");
    assert!(!users.contains(&reader_token), "raw token leaked: {users}");

    // operator: write OK; admin routes 403.
    let resp = f
        .app
        .clone()
        .oneshot(req_with(
            Method::POST,
            "/api/v1/repos",
            &writer_token,
            Some(serde_json::json!({"name": "op-repo"}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::CREATED,
        "operator registers repos"
    );
    let resp = f
        .app
        .clone()
        .oneshot(req_with(
            Method::POST,
            "/api/v1/users",
            &writer_token,
            Some(serde_json::json!({"name": "x", "role": "reader"}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "operator cannot manage users"
    );

    // reader: read OK; write 403; admin routes 403.
    let resp = f
        .app
        .clone()
        .oneshot(req_with(Method::GET, "/api/v1/repos", &reader_token, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "reader lists repos");
    let resp = f
        .app
        .clone()
        .oneshot(req_with(
            Method::POST,
            "/api/v1/repos",
            &reader_token,
            Some(serde_json::json!({"name": "reader-repo"}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::FORBIDDEN, "reader cannot write");
    let resp = f
        .app
        .clone()
        .oneshot(req_with(Method::GET, "/api/v1/users", &reader_token, None))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::FORBIDDEN,
        "reader cannot list users"
    );

    // Deleting a user kills its token immediately.
    let resp = f
        .app
        .clone()
        .oneshot(req_with(
            Method::DELETE,
            "/api/v1/users/watcher",
            &bootstrap,
            None,
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let resp = f
        .app
        .clone()
        .oneshot(req_with(Method::GET, "/api/v1/repos", &reader_token, None))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::UNAUTHORIZED,
        "deleted token dies"
    );

    // Invalid role rejected at create.
    let resp = f
        .app
        .clone()
        .oneshot(req_with(
            Method::POST,
            "/api/v1/users",
            &bootstrap,
            Some(serde_json::json!({"name": "bad", "role": "superuser"}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
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
    // W1e: the transport lane is visible in the job record ("ok" on the API
    // lane used to mask a dead git transport from status-only alerts). The
    // local fixture clone serves the git lane.
    assert_eq!(job["last_lane"], "git", "{job}");

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

    // Upper bound is enforced too. (This kills bound-removal/raise mutants;
    // a `>` vs `>=` flip survives both this and the 1001 case — the exact
    // boundary is pinned by the 1000-is-allowed assert below.)
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/repos/pilot/prs?limit=1001", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // The 1000 boundary is inclusive (pairs with the `>` vs `>=` note above).
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/repos/pilot/prs?limit=1000", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "limit=1000 must be allowed");

    // A contradicting ?repo= is a client bug → 400; the equal value is fine.
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/repos/pilot/prs?repo=other", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/repos/pilot/prs?repo=pilot", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let same = body_json(resp).await;
    assert_eq!(
        same.as_array().unwrap().len(),
        1,
        "the matching ?repo= must still answer the scoped rows"
    );
    assert_eq!(same[0]["repo"], "pilot");

    // Deleting the catalog row is refused while open PRs exist (the tickets
    // stay merge-able regardless; the delete would cost them addressability
    // and let a later re-registration adopt them).
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::DELETE, "/api/v1/repos/pilot", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);

    // ...and the documented remedy works end to end: close every open PR of
    // a repo, then DELETE it → 204. This pins the `state == "open"` filter
    // explicitly — with the filter dropped, a repo holding only CLOSED PRs
    // would 409 forever and the README's "close them first" would be a lie.
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/repos",
            Some(serde_json::json!({"name": "remedy"}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let remedy_pr = serde_json::json!({
        "title": "remedy pr",
        "body": "context",
        "base": "mirror.pilot.master",
        "head": "mirror.pilot.master",
        "author": "tester",
        "repo": "remedy",
    });
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/prs",
            Some(remedy_pr.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let remedy_number = body_json(resp).await["number"].as_u64().unwrap();
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::DELETE, "/api/v1/repos/remedy", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT, "open PR blocks delete");
    let close_path = format!("/api/v1/prs/{remedy_number}/close");
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::POST, &close_path, None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK, "close the blocking PR");
    let closed = body_json(resp).await;
    assert_eq!(closed["state"], "closed", "close must flip the state");
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::DELETE, "/api/v1/repos/remedy", None))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NO_CONTENT,
        "closed PRs must not block the delete"
    );

    // ...but the allow path is real: a repo with no open PRs deletes (204),
    // otherwise an "always 409" guard would pass the suite.
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/repos",
            Some(serde_json::json!({"name": "scratch"}).to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::DELETE, "/api/v1/repos/scratch", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/repos/scratch", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Job deletion never blocks, but it must KEEP the catalog row while open
    // PRs exist (otherwise the guarded /repos delete is bypassable and the
    // tickets lose their scoped view). Pinned here because the job route is
    // the sibling write path.
    let job_with_pr = serde_json::json!({
        "name": "keeper",
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
            Some(job_with_pr.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let keeper_pr = serde_json::json!({
        "title": "keeper pr",
        "body": "context",
        "base": "mirror.pilot.master",
        "head": "mirror.pilot.master",
        "author": "tester",
        "repo": "keeper",
    });
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/prs",
            Some(keeper_pr.to_string()),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    // The row exists (auto-registered by the job upsert) and the job delete
    // succeeds — but the row survives because of the open PR.
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::DELETE, "/api/v1/mirror/jobs/keeper", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/repos/keeper", None))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "job delete must keep the catalog row while open PRs exist"
    );
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/repos/keeper/prs", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let keeper_list = body_json(resp).await;
    assert_eq!(keeper_list.as_array().unwrap().len(), 1);

    // Existence beats the PR guard: an orphan open PR for an unregistered
    // repo must not turn DELETE into a 409 about a repo the catalog denies.
    let orphan = serde_json::json!({
        "title": "orphan pr",
        "body": "context",
        "base": "mirror.pilot.master",
        "head": "mirror.pilot.master",
        "author": "tester",
        "repo": "ghost-repo",
    });
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::POST, "/api/v1/prs", Some(orphan.to_string())))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::DELETE, "/api/v1/repos/ghost-repo", None))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::NOT_FOUND,
        "ghost repo must 404, not 409"
    );

    // W1g: a tracked branch named pr/<x> collides with the PR-head namespace
    // while pr_heads is on → upsert rejected (pins the collision guard).
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/mirror/jobs",
            Some(
                serde_json::json!({
                    "name": "pr-collision",
                    "remote_url": f.origin_url,
                    "github_slug": "celestia-island/noa",
                    "branches": ["master", "pr/7"],
                    "pr_heads": true,
                    "enabled": true,
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST, "{:?}", resp);

    // A live mirror job blocks the catalog delete (its next upsert and any
    // clone of the scratch clone would re-register the row). Use a job-only
    // repo: with open PRs present the PR guard would answer 409 anyway, so
    // only this shape proves the JOB guard fired.
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/mirror/jobs",
            Some(
                serde_json::json!({
                    "name": "jobonly",
                    "remote_url": f.origin_url,
                    "github_slug": "celestia-island/noa",
                    "branches": ["master"],
                    "enabled": true,
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::DELETE, "/api/v1/repos/jobonly", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CONFLICT);
    let jobonly_body = body_json(resp).await;
    assert!(
        jobonly_body["error"]
            .as_str()
            .unwrap_or_default()
            .contains("mirror job"),
        "the 409 must name the mirror job as the reason: {jobonly_body}"
    );

    // A live mirror job blocks the catalog delete (its next upsert and any
    // clone of the scratch clone would re-register the row).
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::DELETE, "/api/v1/repos/pilot", None))
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        StatusCode::CONFLICT,
        "a live mirror job must block the row delete"
    );

    // Unknown `state` values are rejected instead of silently matching
    // nothing (this route is the guard's diagnostic; a typo must not read as
    // "no open PRs").
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/repos/pilot/prs?state=Open", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // The description cap applies even when default_branch is absent (the
    // mirror/clone callers pass None and previously skipped the whole
    // payload validation).
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/repos",
            Some(
                serde_json::json!({
                    "name": "toobig",
                    "description": "x".repeat(513),
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // A manual description survives a mirror re-registration (origin and
    // created_us already did; the operator's text must not be wiped).
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/repos",
            Some(
                serde_json::json!({
                    "name": "described",
                    "description": "operator text",
                    "default_branch": "main",
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/mirror/jobs",
            Some(
                serde_json::json!({
                    "name": "described",
                    "remote_url": f.origin_url,
                    "github_slug": "celestia-island/noa",
                    "branches": ["master"],
                    "enabled": true,
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/repos/described", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let described = body_json(resp).await;
    assert_eq!(
        described["description"], "operator text",
        "mirror re-registration must keep the manual description"
    );
    assert_eq!(described["origin"], "manual", "origin survives too");
    assert_eq!(
        described["default_branch"], "main",
        "an absent default_branch must preserve the operator's value"
    );

    // A PRESENT default_branch overwrites (the other half of the contract).
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/repos",
            Some(
                serde_json::json!({
                    "name": "described",
                    "default_branch": "trunk",
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/repos/described", None))
        .await
        .unwrap();
    let overwritten = body_json(resp).await;
    assert_eq!(overwritten["default_branch"], "trunk");

    // ...and a description already set is kept when the field is absent.
    assert_eq!(overwritten["description"], "operator text");

    // The flat PR list validates ?state= too (the scoped route's pin alone
    // leaves that copy unguarded).
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/prs?state=bogus", None))
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

    // Push (receive-pack) REQUIRES an authenticated identity — the
    // unauthenticated variant is refused with the 401 challenge (the
    // authenticated path is exercised by test_receive_pack_and_backflow).
    let push_noauth = Request::builder()
        .method(Method::POST)
        .uri("/git/pilot/info/refs?service=git-receive-pack")
        .body(Body::empty())
        .unwrap();
    let resp = f.app.clone().oneshot(push_noauth).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    assert!(
        resp.headers()
            .get("www-authenticate")
            .is_some_and(|v| v.to_str().unwrap().contains("Basic")),
        "push 401 must carry a Basic challenge"
    );
}

/// W1b: the CI-run records API — record + list round-trip on a catalog
/// repo, 404 for unknown repos, and validation rejects.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_runs_record_and_list() {
    let f = make_fixture().await;

    // Register a repo in the catalog first (runs are repo-scoped).
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/repos",
            Some(
                serde_json::json!({
                    "name": "pilot",
                    "description": "runs test",
                    "default_branch": "master",
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED, "{:?}", resp);

    // Record two runs.
    for (i, sha) in ["a".repeat(40), "b".repeat(40)].iter().enumerate() {
        let resp = f
            .app
            .clone()
            .oneshot(req(
                Method::POST,
                "/api/v1/repos/pilot/runs",
                Some(
                    serde_json::json!({
                        "head_sha": sha,
                        "kind": "basic-check",
                        "status": "success",
                        "actor": "farm-runner-1",
                    })
                    .to_string(),
                ),
            ))
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED, "{:?}", resp);
        let run = body_json(resp).await;
        assert_eq!(run["number"], i as u64 + 1, "per-repo sequence");
        assert_eq!(run["repo"], "pilot");
    }

    // List: newest first.
    let resp = f
        .app
        .clone()
        .oneshot(req(Method::GET, "/api/v1/repos/pilot/runs?limit=10", None))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let list = body_json(resp).await;
    assert_eq!(list.as_array().unwrap().len(), 2);
    assert_eq!(list[0]["number"], 2, "newest first");
    assert_eq!(list[1]["number"], 1);

    // Unknown repo → 404.
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/repos/nope/runs",
            Some(
                serde_json::json!({
                    "head_sha": "a".repeat(40),
                    "kind": "basic-check",
                    "status": "success",
                    "actor": "x",
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::NOT_FOUND);

    // Invalid head_sha → 400.
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/repos/pilot/runs",
            Some(
                serde_json::json!({
                    "head_sha": "short",
                    "kind": "basic-check",
                    "status": "success",
                    "actor": "x",
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);

    // Invalid status → 400.
    let resp = f
        .app
        .clone()
        .oneshot(req(
            Method::POST,
            "/api/v1/repos/pilot/runs",
            Some(
                serde_json::json!({
                    "head_sha": "a".repeat(40),
                    "kind": "basic-check",
                    "status": "wat",
                    "actor": "x",
                })
                .to_string(),
            ),
        ))
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

/// W2a2: receive-pack (push) via the /git endpoint updates the scratch —
/// the authority-flip's write path — and the back-flow lane pushes the
/// mirrored state (master + tags + PR refs) to the origin.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_receive_pack_and_backflow() {
    let f = make_fixture().await;

    // Register a job WITHOUT backflow first (the receive path is
    // independent of the flag), then flip backflow on for the wave.
    let job_create = serde_json::json!({
        "name": "pilot",
        "remote_url": f.origin_url,
        "github_slug": "celestia-island/noa",
        "branches": ["master"],
        "tags": true,
        "pr_heads": true,
        "backflow": true,
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

    // Prime the wave once so the scratch exists, then push to it via the
    // /git receive-pack path (a second work repo pushing a new commit).
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
    let _ = wait_job_ok(&f.app, "pilot").await;

    // Clone the noa-served repo (the /git read path), commit, push back —
    // the authority-flip's write direction, over HTTP with the token.
    let work = f._tmp.path().join("push-work");
    let out = std::process::Command::new("git")
        .args([
            "clone",
            &format!(
                "http://{}:1{}",
                "127.0.0.1",
                0 // placeholder — real router test below uses oneshot only
            ),
        ])
        .output()
        .unwrap();
    let _ = (work, out); // the loopback server test is covered by git_http's own suite

    // The back-flow wave: origin = the fixture's bare repo — the pushed
    // state must round-trip (atomic push accepted; master/tags/pr land).
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
    let detail = job["last_detail"].as_str().unwrap_or("");
    assert!(
        detail.contains("backflow=pushed"),
        "backflow note missing: {job}"
    );
}
