pub mod export;
pub mod import;
mod translate;

use async_trait::async_trait;
use std::process::Command;

pub use export::{
    clone_git_to_noa, detect_lfs_available, export_noa_to_git, has_lfs_tracking, lfs_install,
    lfs_pull, lfs_push_all,
};
pub use import::{import_git_to_noa, is_lfs_pointer};
pub use translate::GitTranslator;

use crate::{
    error::{NoaError, Result},
    remote::{FetchResult, FetchSpec, PushResult, PushSpec, RemoteBackend, RemoteRef},
};

pub struct GitBackend;

impl Default for GitBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl GitBackend {
    pub fn new() -> Self {
        GitBackend
    }
}

#[async_trait]
impl RemoteBackend for GitBackend {
    fn protocol(&self) -> &str {
        "git"
    }

    async fn push(&self, url: &str, _: &[PushSpec]) -> Result<PushResult> {
        export::validate_git_url(url)?;
        let output = Command::new("git")
            .args(["push", url])
            .output()
            .map_err(|e| NoaError::Remote(format!("git push failed: {}", e)))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            Ok(PushResult {
                ok: true,
                message: format!("{}\n{}", stdout, stderr).trim().to_string(),
            })
        } else {
            Ok(PushResult {
                ok: false,
                message: stderr,
            })
        }
    }

    async fn fetch(&self, url: &str, _: &[FetchSpec]) -> Result<FetchResult> {
        export::validate_git_url(url)?;
        let output = Command::new("git")
            .args(["fetch", url])
            .output()
            .map_err(|e| NoaError::Remote(format!("git fetch failed: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(NoaError::Remote(format!("git fetch failed: {}", stderr)));
        }

        self.list_refs(url).await.map(|refs| FetchResult { refs })
    }

    async fn list_refs(&self, url: &str) -> Result<Vec<RemoteRef>> {
        export::validate_git_url(url)?;
        let output = Command::new("git")
            .args(["ls-remote", "--refs", url])
            .output()
            .map_err(|e| NoaError::Remote(format!("git ls-remote failed: {}", e)))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(NoaError::Remote(format!(
                "git ls-remote failed: {}",
                stderr
            )));
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
