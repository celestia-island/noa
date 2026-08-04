//! `noa pr` command surface (P6#B) — forge-agnostic PR lifecycle.

use crate::error::{NoaError, Result};
use crate::forge::github::parse_github_slug;
use crate::forge::{
    create_forge_backend, CreatePrRequest, ForgeConfig, ForgeKind, PrMetadata, PrState,
};
use crate::repo::Repository;

pub struct PrCreateArgs {
    pub remote: String,
    pub base: String,
    pub head: String,
    pub title: String,
    pub body: Option<String>,
    pub forge: Option<String>,
    pub metadata: Option<String>,
}

pub struct PrListArgs {
    pub remote: String,
    pub base: Option<String>,
    pub state: Option<String>,
    pub forge: Option<String>,
}

pub struct PrShowArgs {
    pub remote: String,
    pub id: String,
    pub forge: Option<String>,
}

pub struct PrMergeArgs {
    pub remote: String,
    pub id: String,
    pub squash: bool,
    pub forge: Option<String>,
}

fn parse_forge_kind(s: &str) -> Result<ForgeKind> {
    match s.to_ascii_lowercase().replace('-', "_").as_str() {
        "github" => Ok(ForgeKind::Github),
        "gitlab" => Ok(ForgeKind::Gitlab),
        "gitea" => Ok(ForgeKind::Gitea),
        "selfhosted" | "self_hosted" => Ok(ForgeKind::SelfHosted),
        other => Err(anyhow::anyhow!("unknown forge kind: {other}")),
    }
}

fn parse_pr_state(s: &str) -> Result<PrState> {
    match s.to_ascii_lowercase().as_str() {
        "open" => Ok(PrState::Open),
        "closed" => Ok(PrState::Closed),
        "merged" => Ok(PrState::Merged),
        other => Err(anyhow::anyhow!(
            "unknown PR state: {other} (expected open|closed|merged)"
        )),
    }
}

/// Resolves the effective forge configuration for a repo's remote:
/// explicit `--for` flag > `[remote.<name>.pr]` config > URL derivation.
fn resolve_config(
    repo: &Repository,
    remote_name: &str,
    forge_flag: Option<&str>,
) -> Result<ForgeConfig> {
    let remote = repo
        .config
        .get_remote(remote_name)
        .ok_or_else(|| anyhow::anyhow!("remote '{remote_name}' not found"))?;

    let flag_kind = forge_flag.map(parse_forge_kind).transpose()?;
    let kind = if let Some(k) = flag_kind {
        k
    } else if let Some(p) = &remote.pr {
        p.kind
    } else if remote.url.contains("github.com") {
        ForgeKind::Github
    } else {
        return Err(NoaError::UnsupportedForge {
            kind: "unknown (remote URL does not match a known forge; set [remote.<name>.pr])"
                .to_string(),
        }
        .into());
    };

    let mut cfg = ForgeConfig {
        kind,
        base_url: None,
        token_env: None,
        repo: None,
    };
    if let Some(p) = &remote.pr {
        cfg.base_url = p.base_url.clone();
        cfg.token_env = p.token_env.clone();
    }
    if kind == ForgeKind::Github {
        let (owner, name) = parse_github_slug(&remote.url)?;
        cfg.repo = Some(format!("{owner}/{name}"));
        if cfg.token_env.is_none() {
            cfg.token_env = Some("GH_TOKEN".to_string());
        }
    }
    Ok(cfg)
}

fn print_json<T: serde::Serialize>(value: &T) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

pub async fn run_create(repo: &Repository, args: &PrCreateArgs) -> Result<()> {
    let cfg = resolve_config(repo, &args.remote, args.forge.as_deref())?;
    let backend = create_forge_backend(cfg.kind)?;
    let metadata = args
        .metadata
        .as_deref()
        .map(PrMetadata::from_json)
        .transpose()?;
    let pr = backend
        .create_pr(
            &cfg,
            &CreatePrRequest {
                base: args.base.clone(),
                head: args.head.clone(),
                title: args.title.clone(),
                body: args.body.clone().unwrap_or_default(),
                metadata,
            },
        )
        .await?;
    print_json(&pr)
}

pub async fn run_list(repo: &Repository, args: &PrListArgs) -> Result<()> {
    let cfg = resolve_config(repo, &args.remote, args.forge.as_deref())?;
    let backend = create_forge_backend(cfg.kind)?;
    let state = args.state.as_deref().map(parse_pr_state).transpose()?;
    let prs = backend.list_prs(&cfg, args.base.as_deref(), state).await?;
    print_json(&prs)
}

pub async fn run_show(repo: &Repository, args: &PrShowArgs) -> Result<()> {
    let cfg = resolve_config(repo, &args.remote, args.forge.as_deref())?;
    let backend = create_forge_backend(cfg.kind)?;
    let pr = backend.get_pr(&cfg, &args.id).await?;
    print_json(&pr)
}

pub async fn run_merge(repo: &Repository, args: &PrMergeArgs) -> Result<()> {
    let cfg = resolve_config(repo, &args.remote, args.forge.as_deref())?;
    let backend = create_forge_backend(cfg.kind)?;
    backend.merge_pr(&cfg, &args.id, args.squash).await?;
    println!("Merged PR {} (squash={})", args.id, args.squash);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{RemoteConfig, RemoteProtocol};

    fn repo_with_remote(url: &str) -> (Repository, tempfile::TempDir) {
        let tmp = tempfile::TempDir::new().unwrap();
        let mut repo = Repository::init(tmp.path()).unwrap();
        repo.config.add_remote(RemoteConfig {
            name: "origin".to_string(),
            url: url.to_string(),
            protocol: RemoteProtocol::Git,
            pr: None,
        });
        (repo, tmp)
    }

    #[test]
    fn test_resolve_github_from_url() {
        let (repo, _tmp) = repo_with_remote("git@github.com:celestia-island/noa.git");
        let cfg = resolve_config(&repo, "origin", None).unwrap();
        assert_eq!(cfg.kind, ForgeKind::Github);
        assert_eq!(cfg.repo.as_deref(), Some("celestia-island/noa"));
        assert_eq!(cfg.token_env.as_deref(), Some("GH_TOKEN"));
    }

    #[test]
    fn test_resolve_flag_wins_over_url() {
        let (repo, _tmp) = repo_with_remote("https://github.com/owner/repo.git");
        let cfg = resolve_config(&repo, "origin", Some("self-hosted")).unwrap();
        assert_eq!(cfg.kind, ForgeKind::SelfHosted);
    }

    #[test]
    fn test_resolve_unknown_url_fails_explicitly() {
        let (repo, _tmp) = repo_with_remote("git@gitlab.com:owner/repo.git");
        let err = resolve_config(&repo, "origin", None).unwrap_err();
        assert!(
            err.downcast_ref::<NoaError>()
                .is_some_and(|e| matches!(e, NoaError::UnsupportedForge { .. })),
            "got: {err}"
        );
    }

    #[test]
    fn test_resolve_missing_remote_fails() {
        let (repo, _tmp) = repo_with_remote("https://github.com/owner/repo.git");
        assert!(resolve_config(&repo, "nonexistent", None).is_err());
    }

    #[test]
    fn test_parse_forge_kind_variants() {
        assert_eq!(parse_forge_kind("github").unwrap(), ForgeKind::Github);
        assert_eq!(parse_forge_kind("GITHUB").unwrap(), ForgeKind::Github);
        assert_eq!(
            parse_forge_kind("self-hosted").unwrap(),
            ForgeKind::SelfHosted
        );
        assert_eq!(
            parse_forge_kind("self_hosted").unwrap(),
            ForgeKind::SelfHosted
        );
        assert!(parse_forge_kind("unknown").is_err());
    }

    #[test]
    fn test_parse_pr_state() {
        assert_eq!(parse_pr_state("open").unwrap(), PrState::Open);
        assert_eq!(parse_pr_state("MERGED").unwrap(), PrState::Merged);
        assert!(parse_pr_state("bogus").is_err());
    }
}
