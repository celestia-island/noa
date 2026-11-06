//! Git Data API lane for the mirror engine (W1a-2).
//!
//! When the git transport to GitHub is dead (the workspace's L168 family:
//! `git push` over HTTPS TLS fails while `api.github.com` stays reachable)
//! the canary ref can still be advanced by constructing blobs/tree/commit
//! through the REST Git Data API — the same bypass the noa repo itself used
//! operationally on 2026-11-05 (`33fc1bcb`).
//!
//! The client is deliberately dumb: no retries, no batching; every call
//! surfaces HTTP status + truncated body on failure. `base_url` is
//! injectable so tests run against a local mock.

use anyhow::Result;
use base64::Engine;
use serde::{Deserialize, Serialize};

pub struct GitDataClient {
    base_url: String,
    token: String,
    client: reqwest::Client,
}

/// One entry of a GitHub `POST /git/trees` payload.
#[derive(Serialize)]
pub struct ApiTreeEntry {
    pub path: String,
    /// `100644` | `100755` | `120000` | `160000`.
    pub mode: &'static str,
    /// `blob` | `commit` (gitlink).
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub sha: String,
}

#[derive(Serialize)]
struct CreateBlob<'a> {
    content: String,
    encoding: &'a str,
}

#[derive(Serialize)]
struct CreateTree<'a> {
    tree: &'a [ApiTreeEntry],
}

#[derive(Serialize)]
struct CreateCommit<'a> {
    message: &'a str,
    tree: &'a str,
    parents: &'a [String],
}

#[derive(Serialize)]
struct UpdateRef<'a> {
    sha: &'a str,
    force: bool,
}

#[derive(Serialize)]
struct CreateRef<'a> {
    #[serde(rename = "ref")]
    ref_name: &'a str,
    sha: &'a str,
}

#[derive(Deserialize)]
struct ShaResponse {
    sha: String,
}

#[derive(Deserialize)]
struct RefResponse {
    object: RefObject,
}

#[derive(Deserialize)]
struct RefObject {
    sha: String,
}

impl GitDataClient {
    pub fn new(base_url: impl Into<String>, token: impl Into<String>) -> Self {
        GitDataClient {
            base_url: base_url.into(),
            token: token.into(),
            client: reqwest::Client::new(),
        }
    }

    async fn request(
        &self,
        method: reqwest::Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<(reqwest::StatusCode, String)> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.client.request(method, &url).bearer_auth(&self.token);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await?;
        let status = resp.status();
        let text = resp.text().await?;
        Ok((status, text))
    }

    fn expect_sha(status: reqwest::StatusCode, text: &str, what: &str) -> Result<String> {
        if !status.is_success() {
            anyhow::bail!("git-data {what} failed ({status}): {}", truncate(text));
        }
        let parsed: ShaResponse = serde_json::from_str(text)
            .map_err(|e| anyhow::anyhow!("git-data {what} bad response: {e}"))?;
        Ok(parsed.sha)
    }

    /// Uploads blob content, returns the GitHub blob sha.
    pub async fn put_blob(&self, slug: &str, content: &[u8]) -> Result<String> {
        let body = serde_json::to_value(CreateBlob {
            content: base64::engine::general_purpose::STANDARD.encode(content),
            encoding: "base64",
        })?;
        let (status, text) = self
            .request(
                reqwest::Method::POST,
                &format!("/repos/{slug}/git/blobs"),
                Some(body),
            )
            .await?;
        Self::expect_sha(status, &text, "create blob")
    }

    /// Creates a tree from complete entries (no base_tree), returns tree sha.
    pub async fn put_tree(&self, slug: &str, entries: &[ApiTreeEntry]) -> Result<String> {
        let body = serde_json::to_value(CreateTree { tree: entries })?;
        let (status, text) = self
            .request(
                reqwest::Method::POST,
                &format!("/repos/{slug}/git/trees"),
                Some(body),
            )
            .await?;
        Self::expect_sha(status, &text, "create tree")
    }

    /// Creates a commit object, returns commit sha.
    pub async fn put_commit(
        &self,
        slug: &str,
        tree_sha: &str,
        parents: &[String],
        message: &str,
    ) -> Result<String> {
        let body = serde_json::to_value(CreateCommit {
            message,
            tree: tree_sha,
            parents,
        })?;
        let (status, text) = self
            .request(
                reqwest::Method::POST,
                &format!("/repos/{slug}/git/commits"),
                Some(body),
            )
            .await?;
        Self::expect_sha(status, &text, "create commit")
    }

    /// Current sha of a ref (`heads/<name>` form), `None` when missing.
    pub async fn get_ref(&self, slug: &str, ref_path: &str) -> Result<Option<String>> {
        let (status, text) = self
            .request(
                reqwest::Method::GET,
                &format!("/repos/{slug}/git/ref/{ref_path}"),
                None,
            )
            .await?;
        match status {
            s if s.is_success() => {
                let parsed: RefResponse = serde_json::from_str(&text)
                    .map_err(|e| anyhow::anyhow!("git-data get ref bad response: {e}"))?;
                Ok(Some(parsed.object.sha))
            }
            reqwest::StatusCode::NOT_FOUND => Ok(None),
            s => anyhow::bail!("git-data get ref failed ({s}): {}", truncate(&text)),
        }
    }

    /// Updates a ref when it exists, creates it otherwise.
    pub async fn upsert_ref(&self, slug: &str, ref_path: &str, sha: &str) -> Result<()> {
        let patch = serde_json::to_value(UpdateRef { sha, force: true })?;
        let result = self
            .request(
                reqwest::Method::PATCH,
                &format!("/repos/{slug}/git/refs/{ref_path}"),
                Some(patch),
            )
            .await;
        match result {
            Ok((s, _t)) if s.is_success() => Ok(()),
            // Missing ref (or GitHub's 422 on non-fast-forward races) falls
            // through to creation.
            Ok((reqwest::StatusCode::NOT_FOUND, _))
            | Ok((reqwest::StatusCode::UNPROCESSABLE_ENTITY, _)) => {
                let create = serde_json::to_value(CreateRef {
                    ref_name: &format!("refs/{ref_path}"),
                    sha,
                })?;
                let (s, t) = self
                    .request(
                        reqwest::Method::POST,
                        &format!("/repos/{slug}/git/refs"),
                        Some(create),
                    )
                    .await?;
                if s.is_success() {
                    Ok(())
                } else {
                    anyhow::bail!("git-data create ref failed ({s}): {}", truncate(&t));
                }
            }
            Ok((s, t)) => anyhow::bail!("git-data update ref failed ({s}): {}", truncate(&t)),
            Err(e) => anyhow::bail!("git-data update ref request failed: {e}"),
        }
    }
}

fn truncate(s: &str) -> String {
    if s.len() <= 200 {
        s.to_string()
    } else {
        format!("{}…", &s[..200])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{routing::post, Router};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    async fn mock_github() -> (String, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_for_blob = Arc::clone(&calls);
        let app = Router::new()
            .route(
                "/repos/{owner}/{repo}/git/blobs",
                post(
                    move |axum::Json(body): axum::Json<serde_json::Value>| async move {
                        calls_for_blob.fetch_add(1, Ordering::SeqCst);
                        assert_eq!(body["encoding"], "base64");
                        axum::Json(serde_json::json!({ "sha": "b105ha1" }))
                    },
                ),
            )
            .route(
                "/repos/{owner}/{repo}/git/trees",
                post(
                    |axum::Json(body): axum::Json<serde_json::Value>| async move {
                        let tree = body["tree"].as_array().unwrap();
                        assert_eq!(tree.len(), 2);
                        assert_eq!(tree[0]["path"], "a.txt");
                        assert_eq!(tree[0]["mode"], "100644");
                        assert_eq!(tree[1]["mode"], "100755");
                        axum::Json(serde_json::json!({ "sha": "7ee5ha" }))
                    },
                ),
            )
            .route(
                "/repos/{owner}/{repo}/git/commits",
                post(
                    |axum::Json(body): axum::Json<serde_json::Value>| async move {
                        assert_eq!(body["tree"], "7ee5ha");
                        assert_eq!(body["message"], "noa mirror canary");
                        axum::Json(serde_json::json!({ "sha": "c0mm171" }))
                    },
                ),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}"), calls)
    }

    #[tokio::test]
    async fn test_blob_tree_commit_roundtrip() {
        let (base, calls) = mock_github().await;
        let client = GitDataClient::new(base, "tok");
        let blob = client.put_blob("o/r", b"hello\n").await.unwrap();
        assert_eq!(blob, "b105ha1");
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let tree = client
            .put_tree(
                "o/r",
                &[
                    ApiTreeEntry {
                        path: "a.txt".into(),
                        mode: "100644",
                        kind: "blob",
                        sha: blob.clone(),
                    },
                    ApiTreeEntry {
                        path: "run.sh".into(),
                        mode: "100755",
                        kind: "blob",
                        sha: blob.clone(),
                    },
                ],
            )
            .await
            .unwrap();
        assert_eq!(tree, "7ee5ha");
        let commit = client
            .put_commit("o/r", &tree, &[], "noa mirror canary")
            .await
            .unwrap();
        assert_eq!(commit, "c0mm171");
    }
    #[tokio::test]
    async fn test_ref_missing_then_created() {
        let app = Router::new()
            .route(
                "/repos/{owner}/{repo}/git/ref/heads/noa-mirror-canary",
                axum::routing::get(|| async { axum::http::StatusCode::NOT_FOUND }),
            )
            .route(
                "/repos/{owner}/{repo}/git/refs/heads/noa-mirror-canary",
                axum::routing::patch(|| async { axum::http::StatusCode::NOT_FOUND }),
            )
            .route(
                "/repos/{owner}/{repo}/git/refs",
                post(
                    |axum::Json(body): axum::Json<serde_json::Value>| async move {
                        assert_eq!(body["ref"], "refs/heads/noa-mirror-canary");
                        assert_eq!(body["sha"], "c0mm171");
                        axum::Json(serde_json::json!({ "object": { "sha": "c0mm171" } }))
                    },
                ),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = GitDataClient::new(format!("http://{addr}"), "tok");
        assert!(client
            .get_ref("o/r", "heads/noa-mirror-canary")
            .await
            .unwrap()
            .is_none());
        client
            .upsert_ref("o/r", "heads/noa-mirror-canary", "c0mm171")
            .await
            .unwrap();
    }
}
