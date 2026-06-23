use async_trait::async_trait;
use std::path::PathBuf;

use crate::{
    error::{NoaError, Result},
    object::{sha256_hex, BlobId, ObjectStore, TreeEntries, TreeId},
};

pub struct GitRawObjectStore {
    work_dir: PathBuf,
    url: String,
}

impl GitRawObjectStore {
    pub async fn new(url: &str, _config: &crate::config::TransportConfig) -> Result<Self> {
        let hash = &sha256_hex(url.as_bytes())[..16];
        let work_dir = std::env::temp_dir().join("noa-git-raw").join(hash);

        if !work_dir.join(".git").exists() {
            let url_owned = url.to_string();
            let wd = work_dir.clone();
            tokio::task::spawn_blocking(move || -> Result<()> {
                std::fs::create_dir_all(&wd)?;
                let status = std::process::Command::new("git")
                    .args(["clone", &url_owned, &wd.to_string_lossy()])
                    .status()?;
                if !status.success() {
                    anyhow::bail!("git clone failed for {url_owned}");
                }
                Ok(())
            })
            .await??;
        }

        std::fs::create_dir_all(work_dir.join("blobs"))?;
        std::fs::create_dir_all(work_dir.join("trees"))?;

        Ok(Self {
            work_dir,
            url: url.to_string(),
        })
    }

    fn blob_path(&self, id: &BlobId) -> PathBuf {
        self.work_dir.join("blobs").join(&id.0)
    }

    fn tree_path(&self, id: &TreeId) -> PathBuf {
        self.work_dir.join("trees").join(&id.0)
    }

    pub async fn sync(&self) -> Result<()> {
        let wd = self.work_dir.clone();
        let url = self.url.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            for args in [
                vec!["add", "-A"],
                vec!["commit", "-m", "noa object sync"],
                vec!["push", &url],
            ] {
                let status = std::process::Command::new("git")
                    .args(&args)
                    .current_dir(&wd)
                    .status()?;
                if !status.success() && args[0] != "commit" {
                    anyhow::bail!("git {} failed", args.join(" "));
                }
            }
            Ok(())
        })
        .await??;
        Ok(())
    }

    pub async fn pull(&self) -> Result<()> {
        let wd = self.work_dir.clone();
        let url = self.url.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            let status = std::process::Command::new("git")
                .args(["pull", &url])
                .current_dir(&wd)
                .status()?;
            if !status.success() {
                anyhow::bail!("git pull failed");
            }
            Ok(())
        })
        .await??;
        Ok(())
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

pub async fn commit_and_push(url: &str) -> Result<()> {
    let hash = &sha256_hex(url.as_bytes())[..16];
    let work_dir = std::env::temp_dir().join("noa-git-raw").join(hash);

    if !work_dir.exists() {
        return Ok(());
    }

    let url_owned = url.to_string();
    let wd = work_dir.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        for args in [
            vec!["add", "-A"],
            vec!["commit", "-m", "noa object sync"],
            vec!["push", &url_owned],
        ] {
            let status = std::process::Command::new("git")
                .args(&args)
                .current_dir(&wd)
                .status()?;
            if !status.success() && args[0] != "commit" {
                anyhow::bail!("git {} failed", args.join(" "));
            }
        }
        Ok(())
    })
    .await??;
    Ok(())
}

pub async fn pull_latest(url: &str) -> Result<()> {
    let hash = &sha256_hex(url.as_bytes())[..16];
    let work_dir = std::env::temp_dir().join("noa-git-raw").join(hash);

    if !work_dir.exists() {
        return Ok(());
    }

    let url_owned = url.to_string();
    let wd = work_dir.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let status = std::process::Command::new("git")
            .args(["pull", &url_owned])
            .current_dir(&wd)
            .status()?;
        if !status.success() {
            anyhow::bail!("git pull failed");
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
