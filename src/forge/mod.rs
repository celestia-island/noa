//! Forge-agnostic pull-request platform abstraction (P6#B).
//!
//! Mirrors the [`crate::remote::RemoteBackend`] pattern: a small trait
//! implemented per git-host forge, selected through a factory. GitHub is the
//! v1 backend; self-hosted (noa-server), GitLab and Gitea follow.

pub mod github;
pub mod self_hosted;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::{forge_err, NoaError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ForgeKind {
    Github,
    Gitlab,
    Gitea,
    SelfHosted,
}

impl std::fmt::Display for ForgeKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            ForgeKind::Github => "github",
            ForgeKind::Gitlab => "gitlab",
            ForgeKind::Gitea => "gitea",
            ForgeKind::SelfHosted => "self-hosted",
        };
        f.write_str(s)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForgeConfig {
    pub kind: ForgeKind,
    /// API base URL. For GitHub this defaults to `https://api.github.com`.
    /// For self-hosted forges this is mandatory.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    /// Name of the environment variable holding the API token.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_env: Option<String>,
    /// Repository identity in forge terms, e.g. `owner/repo` for GitHub.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
}

impl ForgeConfig {
    #[must_use]
    pub fn github() -> Self {
        ForgeConfig {
            kind: ForgeKind::Github,
            base_url: None,
            token_env: None,
            repo: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PrState {
    Open,
    Closed,
    Merged,
}

impl std::fmt::Display for PrState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            PrState::Open => "open",
            PrState::Closed => "closed",
            PrState::Merged => "merged",
        };
        f.write_str(s)
    }
}

/// Platform-specific markers attached to a PR (dogfood requirement): which
/// model produced it and how much usage it cost.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct PrMetadata {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
}

impl PrMetadata {
    /// Parses a JSON object, e.g. `{"model":"x","input_tokens":1}`.
    pub fn from_json(s: &str) -> Result<Self> {
        serde_json::from_str(s).map_err(|e| forge_err("metadata", e))
    }

    /// Renders the metadata as a marker-fenced block appended to a PR body so
    /// it survives round-trips on any forge.
    pub fn render_markdown(&self) -> String {
        let mut out = String::from("\n\n<!-- noa-pr-metadata -->\n");
        if let Some(m) = &self.model {
            out.push_str(&format!("model: {m}\n"));
        }
        if let Some(n) = self.input_tokens {
            out.push_str(&format!("input_tokens: {n}\n"));
        }
        if let Some(n) = self.output_tokens {
            out.push_str(&format!("output_tokens: {n}\n"));
        }
        if let Some(c) = self.cost_usd {
            out.push_str(&format!("cost_usd: {c}\n"));
        }
        out.push_str("<!-- /noa-pr-metadata -->\n");
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequest {
    /// Forge-local identifier (GitHub number / self-hosted id).
    pub id: String,
    pub number: u64,
    pub title: String,
    pub body: String,
    pub state: PrState,
    /// Target branch.
    pub base: String,
    /// Source branch.
    pub head: String,
    pub author: String,
    /// Web URL of the PR.
    pub url: String,
    /// Unix seconds.
    pub created_at: i64,
    pub metadata: PrMetadata,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatePrRequest {
    pub base: String,
    pub head: String,
    pub title: String,
    pub body: String,
    pub metadata: Option<PrMetadata>,
}

#[async_trait]
pub trait ForgeBackend: Send + Sync {
    fn kind(&self) -> ForgeKind;
    async fn create_pr(&self, cfg: &ForgeConfig, req: &CreatePrRequest) -> Result<PullRequest>;
    async fn list_prs(
        &self,
        cfg: &ForgeConfig,
        base: Option<&str>,
        state: Option<PrState>,
    ) -> Result<Vec<PullRequest>>;
    async fn get_pr(&self, cfg: &ForgeConfig, id: &str) -> Result<PullRequest>;
    async fn merge_pr(&self, cfg: &ForgeConfig, id: &str, squash: bool) -> Result<()>;
}

/// Factory, mirroring `create_remote_store` in `crate::object`.
pub fn create_forge_backend(kind: ForgeKind) -> Result<Box<dyn ForgeBackend>> {
    match kind {
        ForgeKind::Github => Ok(Box::new(github::GithubBackend::new())),
        ForgeKind::SelfHosted => Ok(Box::new(self_hosted::SelfHostedBackend::new())),
        other => Err(NoaError::UnsupportedForge {
            kind: other.to_string(),
        }
        .into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_factory_dispatch() {
        assert_eq!(
            create_forge_backend(ForgeKind::Github).unwrap().kind(),
            ForgeKind::Github
        );
        assert_eq!(
            create_forge_backend(ForgeKind::SelfHosted).unwrap().kind(),
            ForgeKind::SelfHosted
        );
        for kind in [ForgeKind::Gitlab, ForgeKind::Gitea] {
            let err = match create_forge_backend(kind) {
                Ok(_) => panic!("expected UnsupportedForge for {kind}"),
                Err(e) => e,
            };
            assert!(
                err.downcast_ref::<NoaError>()
                    .is_some_and(|e| matches!(e, NoaError::UnsupportedForge { .. })),
                "expected UnsupportedForge for {kind}, got {err}"
            );
        }
    }

    #[test]
    fn test_metadata_json_roundtrip() {
        let meta = PrMetadata::from_json(
            r#"{"model":"deepseek/deepseek-chat","input_tokens":10,"output_tokens":20,"cost_usd":0.0012}"#,
        )
        .unwrap();
        assert_eq!(meta.model.as_deref(), Some("deepseek/deepseek-chat"));
        assert_eq!(meta.input_tokens, Some(10));
        assert_eq!(meta.output_tokens, Some(20));
        assert!((meta.cost_usd.unwrap() - 0.0012).abs() < 1e-9);
    }

    #[test]
    fn test_metadata_empty_json() {
        let meta = PrMetadata::from_json("{}").unwrap();
        assert_eq!(meta, PrMetadata::default());
    }

    #[test]
    fn test_metadata_invalid_json() {
        assert!(PrMetadata::from_json("not-json").is_err());
    }

    #[test]
    fn test_metadata_render_markdown() {
        let meta = PrMetadata {
            model: Some("deepseek/deepseek-chat".to_string()),
            input_tokens: Some(10),
            output_tokens: Some(20),
            cost_usd: None,
        };
        let rendered = meta.render_markdown();
        assert!(rendered.contains("<!-- noa-pr-metadata -->"));
        assert!(rendered.contains("model: deepseek/deepseek-chat"));
        assert!(rendered.contains("input_tokens: 10"));
        assert!(rendered.contains("output_tokens: 20"));
        assert!(!rendered.contains("cost_usd"));
        assert!(rendered.contains("<!-- /noa-pr-metadata -->"));
    }
}
