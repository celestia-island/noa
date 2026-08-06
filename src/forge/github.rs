//! GitHub forge backend (v1) — REST API client.
//!
//! Requires a token via `GH_TOKEN` / `GITHUB_TOKEN` or `ForgeConfig.token_env`.
//! The API base defaults to `https://api.github.com` and can be overridden
//! through `ForgeConfig.base_url` (used by the mock-server tests).

use async_trait::async_trait;
use serde::Deserialize;

use crate::error::{forge_err, Result};

use super::{
    CreatePrRequest, ForgeBackend, ForgeConfig, ForgeKind, PrMetadata, PrState, PullRequest,
};

pub struct GithubBackend {
    client: reqwest::Client,
}

impl GithubBackend {
    #[must_use]
    pub fn new() -> Self {
        let _ = rustls::crypto::ring::default_provider().install_default();
        GithubBackend {
            client: reqwest::Client::new(),
        }
    }

    fn api_base(cfg: &ForgeConfig) -> String {
        cfg.base_url
            .clone()
            .unwrap_or_else(|| "https://api.github.com".to_string())
    }

    fn token(cfg: &ForgeConfig) -> Result<String> {
        let configured = cfg.token_env.as_deref().unwrap_or("GH_TOKEN");
        for name in [configured, "GH_TOKEN", "GITHUB_TOKEN"] {
            if let Ok(v) = std::env::var(name) {
                if !v.is_empty() {
                    return Ok(v);
                }
            }
        }
        Err(forge_err(
            "github",
            format!("no API token found: set {configured} (or GH_TOKEN / GITHUB_TOKEN)"),
        ))
    }

    fn repo_parts(cfg: &ForgeConfig) -> Result<(String, String)> {
        let repo = cfg
            .repo
            .as_deref()
            .ok_or_else(|| forge_err("github", "missing repository identity (owner/repo)"))?;
        let (owner, name) = repo
            .split_once('/')
            .ok_or_else(|| forge_err("github", format!("invalid repository identity: {repo}")))?;
        if owner.is_empty() || name.is_empty() {
            return Err(forge_err(
                "github",
                format!("invalid repository identity: {repo}"),
            ));
        }
        Ok((owner.to_string(), name.to_string()))
    }
}

impl Default for GithubBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Parses a git remote URL into a GitHub `owner/repo` slug. Supports the ssh,
/// https and `ssh://` forms, with or without a trailing `.git`.
pub fn parse_github_slug(url: &str) -> Result<(String, String)> {
    let url = url.trim().trim_end_matches('/');
    let tail = if let Some(t) = url.strip_prefix("git@github.com:") {
        t
    } else if let Some(t) = url.strip_prefix("ssh://git@github.com/") {
        t
    } else if let Some(t) = url.strip_prefix("https://github.com/") {
        t
    } else if let Some(t) = url.strip_prefix("http://github.com/") {
        t
    } else {
        return Err(forge_err(
            "github",
            format!("unsupported GitHub remote URL: {url}"),
        ));
    };
    let tail = tail.strip_suffix(".git").unwrap_or(tail);
    let (owner, repo) = tail
        .split_once('/')
        .ok_or_else(|| forge_err("github", format!("cannot parse GitHub remote URL: {url}")))?;
    if owner.is_empty() || repo.is_empty() || repo.contains('/') {
        return Err(forge_err(
            "github",
            format!("cannot parse GitHub remote URL: {url}"),
        ));
    }
    Ok((owner.to_string(), repo.to_string()))
}

#[derive(Debug, Deserialize)]
struct GithubBranchRef {
    #[serde(rename = "ref")]
    r#ref: String,
}

#[derive(Debug, Deserialize)]
struct GithubUser {
    login: String,
}

#[derive(Debug, Deserialize)]
struct GithubPullResponse {
    number: u64,
    title: String,
    #[serde(default)]
    body: Option<String>,
    state: String,
    #[serde(default)]
    merged: bool,
    base: GithubBranchRef,
    head: GithubBranchRef,
    user: GithubUser,
    html_url: String,
    #[serde(default)]
    created_at: String,
}

impl GithubPullResponse {
    fn into_pr(self) -> PullRequest {
        let state = if self.merged {
            PrState::Merged
        } else if self.state == "open" {
            PrState::Open
        } else {
            PrState::Closed
        };
        let created_at = chrono::DateTime::parse_from_rfc3339(&self.created_at)
            .map(|d| d.timestamp())
            .unwrap_or(0);
        PullRequest {
            id: self.number.to_string(),
            number: self.number,
            title: self.title,
            body: self.body.unwrap_or_default(),
            state,
            base: self.base.r#ref,
            head: self.head.r#ref,
            author: self.user.login,
            url: self.html_url,
            created_at,
            metadata: PrMetadata::default(),
        }
    }
}

#[derive(Debug, serde::Serialize)]
struct GithubCreatePull {
    title: String,
    head: String,
    base: String,
    body: String,
}

#[derive(Debug, serde::Serialize)]
struct GithubMergePull {
    squash: bool,
    merge_method: String,
}

fn truncate(s: &str) -> String {
    let max = 500;
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max).collect();
        out.push_str("... (truncated)");
        out
    }
}

#[async_trait]
impl ForgeBackend for GithubBackend {
    fn kind(&self) -> ForgeKind {
        ForgeKind::Github
    }

    async fn create_pr(&self, cfg: &ForgeConfig, req: &CreatePrRequest) -> Result<PullRequest> {
        let (owner, repo) = Self::repo_parts(cfg)?;
        let api = Self::api_base(cfg);
        let token = Self::token(cfg)?;

        let mut body = req.body.clone();
        if let Some(meta) = &req.metadata {
            body.push_str(&meta.render_markdown());
        }

        let resp = self
            .client
            .post(format!("{api}/repos/{owner}/{repo}/pulls"))
            .bearer_auth(&token)
            .json(&GithubCreatePull {
                title: req.title.clone(),
                head: req.head.clone(),
                base: req.base.clone(),
                body,
            })
            .send()
            .await
            .map_err(|e| forge_err("github", format!("create PR request failed: {e}")))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| forge_err("github", format!("read create PR response failed: {e}")))?;
        if !status.is_success() {
            return Err(forge_err(
                "github",
                format!("create PR failed ({status}): {}", truncate(&text)),
            ));
        }
        let parsed: GithubPullResponse = serde_json::from_str(&text)
            .map_err(|e| forge_err("github", format!("bad response: {e}")))?;
        Ok(parsed.into_pr())
    }

    async fn list_prs(
        &self,
        cfg: &ForgeConfig,
        base: Option<&str>,
        state: Option<PrState>,
    ) -> Result<Vec<PullRequest>> {
        let (owner, repo) = Self::repo_parts(cfg)?;
        let api = Self::api_base(cfg);
        let token = Self::token(cfg)?;

        let mut params: Vec<(&str, String)> = Vec::new();
        match state {
            Some(PrState::Open) => params.push(("state", "open".to_string())),
            Some(PrState::Closed) | Some(PrState::Merged) => {
                params.push(("state", "closed".to_string()))
            }
            None => params.push(("state", "all".to_string())),
        }
        if let Some(b) = base {
            params.push(("base", b.to_string()));
        }

        let resp = self
            .client
            .get(format!("{api}/repos/{owner}/{repo}/pulls"))
            .query(&params)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| forge_err("github", format!("list PRs request failed: {e}")))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| forge_err("github", format!("read list PRs response failed: {e}")))?;
        if !status.is_success() {
            return Err(forge_err(
                "github",
                format!("list PRs failed ({status}): {}", truncate(&text)),
            ));
        }
        let parsed: Vec<GithubPullResponse> = serde_json::from_str(&text)
            .map_err(|e| forge_err("github", format!("bad response: {e}")))?;
        Ok(parsed
            .into_iter()
            .map(GithubPullResponse::into_pr)
            .collect())
    }

    async fn get_pr(&self, cfg: &ForgeConfig, id: &str) -> Result<PullRequest> {
        let (owner, repo) = Self::repo_parts(cfg)?;
        let api = Self::api_base(cfg);
        let token = Self::token(cfg)?;

        let resp = self
            .client
            .get(format!("{api}/repos/{owner}/{repo}/pulls/{id}"))
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| forge_err("github", format!("get PR request failed: {e}")))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| forge_err("github", format!("read get PR response failed: {e}")))?;
        if !status.is_success() {
            return Err(forge_err(
                "github",
                format!("get PR failed ({status}): {}", truncate(&text)),
            ));
        }
        let parsed: GithubPullResponse = serde_json::from_str(&text)
            .map_err(|e| forge_err("github", format!("bad response: {e}")))?;
        Ok(parsed.into_pr())
    }

    async fn merge_pr(&self, cfg: &ForgeConfig, id: &str, squash: bool) -> Result<()> {
        let (owner, repo) = Self::repo_parts(cfg)?;
        let api = Self::api_base(cfg);
        let token = Self::token(cfg)?;

        let resp = self
            .client
            .put(format!("{api}/repos/{owner}/{repo}/pulls/{id}/merge"))
            .bearer_auth(&token)
            .json(&GithubMergePull {
                squash,
                merge_method: if squash {
                    "squash".to_string()
                } else {
                    "merge".to_string()
                },
            })
            .send()
            .await
            .map_err(|e| forge_err("github", format!("merge PR request failed: {e}")))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| forge_err("github", format!("read merge PR response failed: {e}")))?;
        if !status.is_success() {
            return Err(forge_err(
                "github",
                format!("merge PR failed ({status}): {}", truncate(&text)),
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        extract::State,
        routing::{get, post, put},
        Json, Router,
    };
    use reqwest::StatusCode;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn gh_pull_json(number: u64, state: &str, merged: bool) -> serde_json::Value {
        serde_json::json!({
            "id": number * 1000,
            "number": number,
            "title": "✨ Add feature.",
            "body": "desc\n\n<!-- noa-pr-metadata -->\nmodel: m\n<!-- /noa-pr-metadata -->",
            "state": state,
            "merged": merged,
            "base": {"ref": "master"},
            "head": {"ref": "feat/x"},
            "user": {"login": "lab"},
            "html_url": format!("https://github.com/owner/repo/pull/{number}"),
            "created_at": "2026-08-04T00:00:00Z"
        })
    }

    async fn spawn_mock(router: Router) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        format!("http://{addr}")
    }

    fn cfg_for(base_url: &str, token_env: &str) -> ForgeConfig {
        ForgeConfig {
            kind: ForgeKind::Github,
            base_url: Some(base_url.to_string()),
            token_env: Some(token_env.to_string()),
            repo: Some("owner/repo".to_string()),
        }
    }

    #[test]
    fn test_parse_github_slug_ssh() {
        assert_eq!(
            parse_github_slug("git@github.com:celestia-island/noa.git").unwrap(),
            ("celestia-island".to_string(), "noa".to_string())
        );
        assert_eq!(
            parse_github_slug("git@github.com:owner/repo").unwrap(),
            ("owner".to_string(), "repo".to_string())
        );
    }

    #[test]
    fn test_parse_github_slug_https() {
        assert_eq!(
            parse_github_slug("https://github.com/owner/repo.git").unwrap(),
            ("owner".to_string(), "repo".to_string())
        );
        assert_eq!(
            parse_github_slug("http://github.com/owner/repo/").unwrap(),
            ("owner".to_string(), "repo".to_string())
        );
    }

    #[test]
    fn test_parse_github_slug_ssh_url_scheme() {
        assert_eq!(
            parse_github_slug("ssh://git@github.com/owner/repo.git").unwrap(),
            ("owner".to_string(), "repo".to_string())
        );
    }

    #[test]
    fn test_parse_github_slug_rejects_non_github() {
        assert!(parse_github_slug("https://gitlab.com/owner/repo.git").is_err());
        assert!(parse_github_slug("https://example.com/owner/repo.git").is_err());
        assert!(parse_github_slug("git@gitlab.com:owner/repo.git").is_err());
    }

    #[test]
    fn test_parse_github_slug_rejects_malformed() {
        assert!(parse_github_slug("git@github.com:only-owner").is_err());
        assert!(parse_github_slug("https://github.com/a/b/c").is_err());
        assert!(parse_github_slug("").is_err());
    }

    #[test]
    fn test_missing_token_is_explicit() {
        std::env::remove_var("NOA_TEST_GH_TOKEN_MISSING");
        let cfg = cfg_for("http://127.0.0.1:1", "NOA_TEST_GH_TOKEN_MISSING");
        let backend = GithubBackend::new();
        let err = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(backend.create_pr(
                &cfg,
                &CreatePrRequest {
                    base: "master".to_string(),
                    head: "feat/x".to_string(),
                    title: "t".to_string(),
                    body: String::new(),
                    metadata: None,
                },
            ))
            .unwrap_err();
        assert!(err.to_string().contains("no API token found"), "got: {err}");
    }

    #[tokio::test]
    async fn test_create_pr_sends_body_and_metadata() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let store = captured.clone();
        let router = Router::new()
            .route(
                "/repos/{owner}/{repo}/pulls",
                post(
                    move |State(s): State<Arc<Mutex<Vec<serde_json::Value>>>>,
                          Json(v): Json<serde_json::Value>| async move {
                        s.lock().await.push(v);
                        (StatusCode::CREATED, Json(gh_pull_json(42, "open", false)))
                    },
                ),
            )
            .with_state(store);
        let base_url = spawn_mock(router).await;

        std::env::set_var("NOA_TEST_GH_TOKEN_CREATE", "tkn-123");
        let cfg = cfg_for(&base_url, "NOA_TEST_GH_TOKEN_CREATE");
        let backend = GithubBackend::new();
        let meta = PrMetadata {
            model: Some("deepseek/deepseek-chat".to_string()),
            input_tokens: Some(100),
            output_tokens: Some(50),
            cost_usd: Some(0.001),
        };
        let pr = backend
            .create_pr(
                &cfg,
                &CreatePrRequest {
                    base: "master".to_string(),
                    head: "feat/x".to_string(),
                    title: "✨ Add x.".to_string(),
                    body: "desc".to_string(),
                    metadata: Some(meta),
                },
            )
            .await
            .unwrap();

        assert_eq!(pr.number, 42);
        assert_eq!(pr.state, PrState::Open);
        assert_eq!(pr.base, "master");
        assert_eq!(pr.head, "feat/x");
        assert_eq!(pr.url, "https://github.com/owner/repo/pull/42");

        let sent = captured.lock().await[0].clone();
        assert_eq!(sent["title"], "✨ Add x.");
        assert_eq!(sent["base"], "master");
        assert_eq!(sent["head"], "feat/x");
        let sent_body = sent["body"].as_str().unwrap();
        assert!(sent_body.starts_with("desc"));
        assert!(sent_body.contains("<!-- noa-pr-metadata -->"));
        assert!(sent_body.contains("model: deepseek/deepseek-chat"));
        assert!(sent_body.contains("cost_usd: 0.001"));
    }

    #[tokio::test]
    async fn test_create_pr_error_is_explicit() {
        let router = Router::new().route(
            "/repos/{owner}/{repo}/pulls",
            post(|| async { (StatusCode::UNPROCESSABLE_ENTITY, "Validation Failed") }),
        );
        let base_url = spawn_mock(router).await;

        std::env::set_var("NOA_TEST_GH_TOKEN_ERR", "tkn");
        let cfg = cfg_for(&base_url, "NOA_TEST_GH_TOKEN_ERR");
        let backend = GithubBackend::new();
        let err = backend
            .create_pr(
                &cfg,
                &CreatePrRequest {
                    base: "master".to_string(),
                    head: "feat/x".to_string(),
                    title: "t".to_string(),
                    body: String::new(),
                    metadata: None,
                },
            )
            .await
            .unwrap_err();
        assert!(
            err.to_string().contains("create PR failed (422"),
            "got: {err}"
        );
    }

    #[tokio::test]
    async fn test_list_prs_parses_merged_state() {
        let router = Router::new().route(
            "/repos/{owner}/{repo}/pulls",
            get(|| async {
                Json(serde_json::json!([
                    gh_pull_json(1, "open", false),
                    gh_pull_json(2, "closed", true)
                ]))
            }),
        );
        let base_url = spawn_mock(router).await;

        std::env::set_var("NOA_TEST_GH_TOKEN_LIST", "tkn");
        let cfg = cfg_for(&base_url, "NOA_TEST_GH_TOKEN_LIST");
        let backend = GithubBackend::new();
        let prs = backend
            .list_prs(&cfg, Some("master"), Some(PrState::Merged))
            .await
            .unwrap();
        assert_eq!(prs.len(), 2);
        assert_eq!(prs[0].state, PrState::Open);
        assert_eq!(prs[1].state, PrState::Merged);
    }

    #[tokio::test]
    async fn test_get_pr() {
        let router = Router::new().route(
            "/repos/{owner}/{repo}/pulls/{number}",
            get(|| async { Json(gh_pull_json(7, "closed", true)) }),
        );
        let base_url = spawn_mock(router).await;

        std::env::set_var("NOA_TEST_GH_TOKEN_GET", "tkn");
        let cfg = cfg_for(&base_url, "NOA_TEST_GH_TOKEN_GET");
        let backend = GithubBackend::new();
        let pr = backend.get_pr(&cfg, "7").await.unwrap();
        assert_eq!(pr.number, 7);
        assert_eq!(pr.state, PrState::Merged);
    }

    #[tokio::test]
    async fn test_merge_pr_sends_squash() {
        let captured = Arc::new(Mutex::new(None::<serde_json::Value>));
        let store = captured.clone();
        let router = Router::new()
            .route(
                "/repos/{owner}/{repo}/pulls/{number}/merge",
                put(
                    move |State(s): State<Arc<Mutex<Option<serde_json::Value>>>>,
                          Json(v): Json<serde_json::Value>| async move {
                        *s.lock().await = Some(v);
                        StatusCode::OK
                    },
                ),
            )
            .with_state(store);
        let base_url = spawn_mock(router).await;

        std::env::set_var("NOA_TEST_GH_TOKEN_MERGE", "tkn");
        let cfg = cfg_for(&base_url, "NOA_TEST_GH_TOKEN_MERGE");
        let backend = GithubBackend::new();
        backend.merge_pr(&cfg, "7", true).await.unwrap();

        let sent = captured.lock().await.clone().unwrap();
        assert_eq!(sent["squash"], true);
        assert_eq!(sent["merge_method"], "squash");
    }
}
