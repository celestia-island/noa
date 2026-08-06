//! Self-hosted forge backend (v1b) — PRs hosted on your own noa-server.
//!
//! Talks to the `/api/v1/prs` endpoints served by noa-server
//! (`crate::server::pr_handlers`), so PRs can be created and merged on the
//! user's own platform with no external forge.

use async_trait::async_trait;
use reqwest::StatusCode;
use serde::Deserialize;

use crate::error::{forge_err, Result};

use super::{
    CreatePrRequest, ForgeBackend, ForgeConfig, ForgeKind, PrMetadata, PrState, PullRequest,
};

pub struct SelfHostedBackend {
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct ServerPr {
    number: u64,
    title: String,
    #[serde(default)]
    body: String,
    state: String,
    base: String,
    head: String,
    author: String,
    created_at: i64,
    #[serde(default)]
    metadata: Option<PrMetadata>,
}

impl ServerPr {
    fn into_pr(self) -> PullRequest {
        let state = match self.state.as_str() {
            "open" => PrState::Open,
            "merged" => PrState::Merged,
            _ => PrState::Closed,
        };
        PullRequest {
            id: self.number.to_string(),
            number: self.number,
            title: self.title,
            body: self.body,
            state,
            base: self.base,
            head: self.head,
            author: self.author,
            url: String::new(),
            created_at: self.created_at,
            metadata: self.metadata.unwrap_or_default(),
        }
    }
}

#[derive(serde::Serialize)]
struct CreatePrBody {
    title: String,
    body: String,
    base: String,
    head: String,
    author: String,
    repo: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    metadata: Option<PrMetadata>,
}

#[derive(serde::Serialize)]
struct MergePrBody {
    squash: bool,
}

impl SelfHostedBackend {
    #[must_use]
    pub fn new() -> Self {
        SelfHostedBackend {
            client: reqwest::Client::new(),
        }
    }

    fn base_url(cfg: &ForgeConfig) -> Result<String> {
        cfg.base_url.clone().ok_or_else(|| {
            forge_err(
                "self-hosted",
                "missing base_url: configure [remotes.<name>.pr] base_url pointing at a noa-server instance",
            )
        })
    }

    fn token(cfg: &ForgeConfig) -> Result<String> {
        let configured = cfg.token_env.as_deref().unwrap_or("NOA_API_TOKEN");
        for name in [configured, "NOA_API_TOKEN"] {
            if let Ok(v) = std::env::var(name) {
                if !v.is_empty() {
                    return Ok(v);
                }
            }
        }
        Err(forge_err(
            "self-hosted",
            format!("no API token found: set {configured} (or NOA_API_TOKEN)"),
        ))
    }

    fn repo(cfg: &ForgeConfig) -> String {
        cfg.repo.clone().unwrap_or_else(|| "default".to_string())
    }

    fn state_param(state: Option<PrState>) -> Option<&'static str> {
        match state {
            Some(PrState::Open) => Some("open"),
            Some(PrState::Closed) => Some("closed"),
            Some(PrState::Merged) => Some("merged"),
            None => None,
        }
    }
}

impl Default for SelfHostedBackend {
    fn default() -> Self {
        Self::new()
    }
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

/// Workspace names on noa-server cannot contain `/`; git branch names can.
/// Map branch-style names (`feat/x`) to flat workspace names (`feat-x`) so the
/// self-hosted PR store can round-trip real branches.
fn sanitize_workspace_name(name: &str) -> String {
    name.replace('/', "-")
}

#[async_trait]
impl ForgeBackend for SelfHostedBackend {
    fn kind(&self) -> ForgeKind {
        ForgeKind::SelfHosted
    }

    async fn create_pr(&self, cfg: &ForgeConfig, req: &CreatePrRequest) -> Result<PullRequest> {
        let base = Self::base_url(cfg)?;
        let token = Self::token(cfg)?;

        let mut body = req.body.clone();
        if let Some(meta) = &req.metadata {
            body.push_str(&meta.render_markdown());
        }

        let resp = self
            .client
            .post(format!("{base}/api/v1/prs"))
            .bearer_auth(&token)
            .json(&CreatePrBody {
                title: req.title.clone(),
                body,
                base: sanitize_workspace_name(&req.base),
                head: sanitize_workspace_name(&req.head),
                author: "noa".to_string(),
                repo: Self::repo(cfg),
                metadata: req.metadata.clone(),
            })
            .send()
            .await
            .map_err(|e| forge_err("self-hosted", format!("create PR request failed: {e}")))?;

        let status = resp.status();
        let text = resp.text().await.map_err(|e| {
            forge_err(
                "self-hosted",
                format!("read create PR response failed: {e}"),
            )
        })?;
        if !status.is_success() {
            return Err(forge_err(
                "self-hosted",
                format!("create PR failed ({status}): {}", truncate(&text)),
            ));
        }
        let parsed: ServerPr = serde_json::from_str(&text)
            .map_err(|e| forge_err("self-hosted", format!("bad response: {e}")))?;
        Ok(parsed.into_pr())
    }

    async fn list_prs(
        &self,
        cfg: &ForgeConfig,
        base: Option<&str>,
        state: Option<PrState>,
    ) -> Result<Vec<PullRequest>> {
        let api = Self::base_url(cfg)?;
        let token = Self::token(cfg)?;

        let mut params: Vec<(&str, String)> = vec![("repo", Self::repo(cfg))];
        if let Some(b) = base {
            params.push(("base", sanitize_workspace_name(b)));
        }
        if let Some(s) = Self::state_param(state) {
            params.push(("state", s.to_string()));
        }

        let resp = self
            .client
            .get(format!("{api}/api/v1/prs"))
            .query(&params)
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| forge_err("self-hosted", format!("list PRs request failed: {e}")))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| forge_err("self-hosted", format!("read list PRs response failed: {e}")))?;
        if !status.is_success() {
            return Err(forge_err(
                "self-hosted",
                format!("list PRs failed ({status}): {}", truncate(&text)),
            ));
        }
        let parsed: Vec<ServerPr> = serde_json::from_str(&text)
            .map_err(|e| forge_err("self-hosted", format!("bad response: {e}")))?;
        Ok(parsed.into_iter().map(ServerPr::into_pr).collect())
    }

    async fn get_pr(&self, cfg: &ForgeConfig, id: &str) -> Result<PullRequest> {
        let api = Self::base_url(cfg)?;
        let token = Self::token(cfg)?;

        let resp = self
            .client
            .get(format!("{api}/api/v1/prs/{id}"))
            .bearer_auth(&token)
            .send()
            .await
            .map_err(|e| forge_err("self-hosted", format!("get PR request failed: {e}")))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| forge_err("self-hosted", format!("read get PR response failed: {e}")))?;
        if !status.is_success() {
            return Err(forge_err(
                "self-hosted",
                format!("get PR failed ({status}): {}", truncate(&text)),
            ));
        }
        let parsed: ServerPr = serde_json::from_str(&text)
            .map_err(|e| forge_err("self-hosted", format!("bad response: {e}")))?;
        Ok(parsed.into_pr())
    }

    async fn merge_pr(&self, cfg: &ForgeConfig, id: &str, squash: bool) -> Result<()> {
        let api = Self::base_url(cfg)?;
        let token = Self::token(cfg)?;

        let resp = self
            .client
            .post(format!("{api}/api/v1/prs/{id}/merge"))
            .bearer_auth(&token)
            .json(&MergePrBody { squash })
            .send()
            .await
            .map_err(|e| forge_err("self-hosted", format!("merge PR request failed: {e}")))?;

        let status = resp.status();
        let text = resp
            .text()
            .await
            .map_err(|e| forge_err("self-hosted", format!("read merge PR response failed: {e}")))?;
        if status == StatusCode::CONFLICT {
            return Err(forge_err(
                "self-hosted",
                format!("merge conflict: {}", truncate(&text)),
            ));
        }
        if !status.is_success() {
            return Err(forge_err(
                "self-hosted",
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
        routing::{get, post},
        Json, Router,
    };
    use std::sync::Arc;
    use tokio::sync::Mutex;

    fn server_pr_json(number: u64, state: &str) -> serde_json::Value {
        serde_json::json!({
            "number": number,
            "title": "✨ Add x.",
            "body": "desc",
            "state": state,
            "base": "master",
            "head": "feat/x",
            "author": "noa",
            "created_at": 100,
            "merge_snapshot": null,
            "metadata": {"model": "deepseek/deepseek-chat"}
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

    fn cfg_for(base_url: &str) -> ForgeConfig {
        ForgeConfig {
            kind: ForgeKind::SelfHosted,
            base_url: Some(base_url.to_string()),
            token_env: Some("NOA_TEST_SH_TOKEN".to_string()),
            repo: Some("default".to_string()),
        }
    }

    #[test]
    fn test_missing_base_url_fails_explicitly() {
        std::env::set_var("NOA_TEST_SH_TOKEN_CFG", "t");
        let cfg = ForgeConfig {
            kind: ForgeKind::SelfHosted,
            base_url: None,
            token_env: Some("NOA_TEST_SH_TOKEN_CFG".to_string()),
            repo: None,
        };
        let backend = SelfHostedBackend::new();
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
        assert!(err.to_string().contains("missing base_url"), "got: {err}");
    }

    #[tokio::test]
    async fn test_create_pr_roundtrip() {
        let captured = Arc::new(Mutex::new(Vec::new()));
        let store = captured.clone();
        let router = Router::new()
            .route(
                "/api/v1/prs",
                post(
                    move |State(s): State<Arc<Mutex<Vec<serde_json::Value>>>>,
                          Json(v): Json<serde_json::Value>| async move {
                        s.lock().await.push(v);
                        (StatusCode::CREATED, Json(server_pr_json(5, "open")))
                    },
                ),
            )
            .with_state(store);
        let base_url = spawn_mock(router).await;

        std::env::set_var("NOA_TEST_SH_TOKEN", "tkn");
        let cfg = cfg_for(&base_url);
        let backend = SelfHostedBackend::new();
        let meta = PrMetadata {
            model: Some("deepseek/deepseek-chat".to_string()),
            input_tokens: Some(10),
            output_tokens: Some(5),
            cost_usd: None,
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

        assert_eq!(pr.number, 5);
        assert_eq!(pr.state, PrState::Open);
        assert_eq!(pr.metadata.model.as_deref(), Some("deepseek/deepseek-chat"));

        let sent = captured.lock().await[0].clone();
        assert_eq!(sent["title"], "✨ Add x.");
        assert_eq!(sent["repo"], "default");
        assert_eq!(sent["base"], "master");
        assert_eq!(sent["head"], "feat-x");
        let sent_body = sent["body"].as_str().unwrap();
        assert!(sent_body.contains("<!-- noa-pr-metadata -->"));
        assert!(sent_body.contains("input_tokens: 10"));
    }

    #[test]
    fn test_sanitize_workspace_name_flattens_slashes() {
        assert_eq!(sanitize_workspace_name("feat/x"), "feat-x");
        assert_eq!(
            sanitize_workspace_name("dogfood/pr-1"),
            "dogfood-pr-1"
        );
        assert_eq!(sanitize_workspace_name("master"), "master");
    }

    #[tokio::test]
    async fn test_list_and_get_and_merge() {
        let router = Router::new()
            .route(
                "/api/v1/prs",
                get(|| async {
                    Json(serde_json::json!([
                        server_pr_json(1, "open"),
                        server_pr_json(2, "merged")
                    ]))
                }),
            )
            .route(
                "/api/v1/prs/{number}",
                get(|| async { Json(server_pr_json(2, "merged")) }),
            )
            .route(
                "/api/v1/prs/{number}/merge",
                post(|| async { StatusCode::OK }),
            );
        let base_url = spawn_mock(router).await;

        std::env::set_var("NOA_TEST_SH_TOKEN", "tkn");
        let cfg = cfg_for(&base_url);
        let backend = SelfHostedBackend::new();

        let prs = backend
            .list_prs(&cfg, Some("master"), Some(PrState::Merged))
            .await
            .unwrap();
        assert_eq!(prs.len(), 2);
        assert_eq!(prs[1].state, PrState::Merged);

        let pr = backend.get_pr(&cfg, "2").await.unwrap();
        assert_eq!(pr.number, 2);
        assert_eq!(pr.state, PrState::Merged);

        backend.merge_pr(&cfg, "2", true).await.unwrap();
    }

    #[tokio::test]
    async fn test_merge_conflict_error() {
        let router = Router::new().route(
            "/api/v1/prs/{number}/merge",
            post(|| async {
                (
                    StatusCode::CONFLICT,
                    Json(serde_json::json!({"error": "merge conflict in PR #1", "conflicts": ["a.txt"]})),
                )
            }),
        );
        let base_url = spawn_mock(router).await;

        std::env::set_var("NOA_TEST_SH_TOKEN", "tkn");
        let cfg = cfg_for(&base_url);
        let backend = SelfHostedBackend::new();
        let err = backend.merge_pr(&cfg, "1", true).await.unwrap_err();
        assert!(err.to_string().contains("merge conflict"), "got: {err}");
    }
}
