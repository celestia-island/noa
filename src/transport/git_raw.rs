use async_trait::async_trait;
use std::path::PathBuf;

use crate::{
    config::TransportConfig,
    error::{NoaError, Result},
    object::{sha256_hex, BlobId, ObjectStore, TreeEntries, TreeId},
    transport::Credentials,
};

pub struct GitRawObjectStore {
    work_dir: PathBuf,
}

impl GitRawObjectStore {
    pub async fn new(url: &str, config: &TransportConfig) -> Result<Self> {
        let creds = Credentials::from_config(config);
        let authed_url = creds.for_git_url(url);
        let hash = &sha256_hex(url.as_bytes())[..16];
        let work_dir = std::env::temp_dir().join("noa-git-raw").join(hash);

        if !work_dir.join(".git").exists() {
            let url_owned = authed_url;
            let wd = work_dir.clone();
            let env = creds.git_env();
            tokio::task::spawn_blocking(move || -> Result<()> {
                std::fs::create_dir_all(&wd)?;
                let mut cmd = std::process::Command::new("git");
                cmd.args(["clone", &url_owned, &wd.to_string_lossy()]);
                for (k, v) in &env {
                    cmd.env(k, v);
                }
                let status = cmd.status()?;
                if !status.success() {
                    anyhow::bail!("git clone failed for {url_owned}");
                }
                Ok(())
            })
            .await??;
        }

        std::fs::create_dir_all(work_dir.join("blobs"))?;
        std::fs::create_dir_all(work_dir.join("trees"))?;

        Ok(Self { work_dir })
    }

    fn blob_path(&self, id: &BlobId) -> PathBuf {
        self.work_dir.join("blobs").join(&id.0)
    }

    fn tree_path(&self, id: &TreeId) -> PathBuf {
        self.work_dir.join("trees").join(&id.0)
    }
}

#[async_trait]
impl ObjectStore for GitRawObjectStore {
    async fn put_blob(&self, content: &[u8]) -> Result<BlobId> {
        let id = BlobId(sha256_hex(content));
        let path = self.blob_path(&id);
        let data = content.to_vec();
        tokio::fs::create_dir_all(path.parent().unwrap()).await?;
        tokio::fs::write(&path, &data).await?;
        Ok(id)
    }

    async fn get_blob(&self, id: &BlobId) -> Result<Vec<u8>> {
        let path = self.blob_path(id);
        match tokio::fs::read(&path).await {
            Ok(data) => Ok(data),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(NoaError::ObjectNotFound { id: id.0.clone() }.into())
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn has_blob(&self, id: &BlobId) -> Result<bool> {
        Ok(self.blob_path(id).exists())
    }

    async fn put_tree(&self, entries: &TreeEntries) -> Result<TreeId> {
        let data = rmp_serde::to_vec(entries)?;
        let id = TreeId(sha256_hex(&data));
        let path = self.tree_path(&id);
        tokio::fs::create_dir_all(path.parent().unwrap()).await?;
        tokio::fs::write(&path, &data).await?;
        Ok(id)
    }

    async fn get_tree(&self, id: &TreeId) -> Result<TreeEntries> {
        let path = self.tree_path(id);
        match tokio::fs::read(&path).await {
            Ok(data) => Ok(rmp_serde::from_slice::<TreeEntries>(&data)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(NoaError::ObjectNotFound { id: id.0.clone() }.into())
            }
            Err(e) => Err(e.into()),
        }
    }

    async fn has_tree(&self, id: &TreeId) -> Result<bool> {
        Ok(self.tree_path(id).exists())
    }
}

pub async fn commit_and_push(url: &str, creds: &Credentials) -> Result<()> {
    let hash = &sha256_hex(url.as_bytes())[..16];
    let work_dir = std::env::temp_dir().join("noa-git-raw").join(hash);

    if !work_dir.exists() {
        return Ok(());
    }

    let authed_url = creds.for_git_url(url);
    let wd = work_dir.clone();
    let env = creds.git_env();
    tokio::task::spawn_blocking(move || -> Result<()> {
        for args in [
            vec!["add", "-A"],
            vec!["commit", "-m", "noa object sync"],
            vec!["push", &authed_url],
        ] {
            let mut cmd = std::process::Command::new("git");
            cmd.args(&args).current_dir(&wd);
            for (k, v) in &env {
                cmd.env(k, v);
            }
            let status = cmd.status()?;
            if !status.success() && args[0] != "commit" {
                anyhow::bail!("git {} failed", args.join(" "));
            }
        }
        Ok(())
    })
    .await??;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_work_dir_derivation() {
        let hash = &sha256_hex("https://github.com/user/repo.git".as_bytes())[..16];
        let expected = std::env::temp_dir().join("noa-git-raw").join(hash);
        assert!(expected.starts_with(std::env::temp_dir()));
    }
}
