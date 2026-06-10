pub mod export;
pub mod import;

use async_trait::async_trait;
use std::process::Command;

pub use export::{
    clone_git_to_noa, detect_lfs_available, export_noa_to_git, has_lfs_tracking, lfs_install,
    lfs_pull, lfs_push_all,
};
pub use import::{import_git_to_noa, is_lfs_pointer};

use crate::{
    error::Result,
    remote::{FetchResult, FetchSpec, PushResult, PushSpec, RemoteBackend, RemoteRef},
};

pub struct GitBackend;

impl Default for GitBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl GitBackend {
    #[must_use]
    pub fn new() -> Self {
        GitBackend
    }
}

#[async_trait]
impl RemoteBackend for GitBackend {
    fn protocol(&self) -> &'static str {
        "git"
    }

    async fn push(&self, url: &str, _: &[PushSpec]) -> Result<PushResult> {
        export::validate_git_url(url)?;
        let output = Command::new("git")
            .args(["push", url])
            .output()
            .with_context(|| "git push failed")?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            Ok(PushResult {
                ok: true,
                message: format!("{stdout}\n{stderr}").trim().to_string(),
            })
        } else {
            anyhow::bail!(format!("git push failed: {stderr}"))
        }
    }

    async fn fetch(&self, url: &str, _: &[FetchSpec]) -> Result<FetchResult> {
        export::validate_git_url(url)?;
        let output = Command::new("git")
            .args(["fetch", url])
            .output()
            .with_context(|| "git fetch failed")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return anyhow::bail!(format!("git fetch failed: {stderr}"));
        }

        self.list_refs(url).await.map(|refs| FetchResult { refs })
    }

    async fn list_refs(&self, url: &str) -> Result<Vec<RemoteRef>> {
        export::validate_git_url(url)?;
        let output = Command::new("git")
            .args(["ls-remote", "--refs", url])
            .output()
            .with_context(|| "git ls-remote failed")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return anyhow::bail!(format!("git ls-remote failed: {stderr}"));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut refs = Vec::new();
        for line in stdout.lines() {
            let parts: Vec<&str> = line.splitn(2, '\t').collect();
            if parts.len() == 2 {
                refs.push(RemoteRef {
                    commit_hash: parts[0].to_string(),
                    name: parts[1].to_string(),
                });
            }
        }
        Ok(refs)
    }
}
