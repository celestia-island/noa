#[cfg(feature = "ftp")]
pub mod ftp_impl;
pub mod ipfs_impl;
pub mod minio_impl;
mod redb_impl;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[cfg(feature = "ftp")]
pub use ftp_impl::FtpObjectStore;
pub use ipfs_impl::IpfsObjectStore;
pub use minio_impl::MinioObjectStore;
pub use redb_impl::RedbObjectStore;
use sha2::{Digest, Sha256};

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlobId(pub String);

impl BlobId {
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl std::fmt::Display for BlobId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TreeId(pub String);

impl TreeId {
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl std::fmt::Display for TreeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TreeEntry {
    pub name: String,
    pub kind: EntryKind,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntryKind {
    Blob,
    Tree,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TreeEntries(pub Vec<TreeEntry>);

impl Default for TreeEntries {
    fn default() -> Self {
        Self::new()
    }
}

impl TreeEntries {
    #[must_use]
    pub fn new() -> Self {
        TreeEntries(Vec::new())
    }

    pub fn sort(&mut self) {
        self.0.sort_by(|a, b| a.name.cmp(&b.name));
    }
}

#[must_use]
pub fn sha256_hex(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

#[async_trait]
pub trait ObjectStore: Send + Sync {
    async fn put_blob(&self, content: &[u8]) -> Result<BlobId>;
    async fn get_blob(&self, id: &BlobId) -> Result<Vec<u8>>;
    async fn has_blob(&self, id: &BlobId) -> Result<bool>;
    async fn put_tree(&self, entries: &TreeEntries) -> Result<TreeId>;
    async fn get_tree(&self, id: &TreeId) -> Result<TreeEntries>;
    async fn has_tree(&self, id: &TreeId) -> Result<bool>;
}

pub async fn create_remote_store(
    config: &crate::config::StorageConfig,
) -> Result<Box<dyn ObjectStore>> {
    match config.backend_type.as_str() {
        "ipfs" => Ok(Box::new(IpfsObjectStore::new(
            &config.endpoint,
            config.auth_token.clone(),
        ))),
        "s3" | "minio" => {
            let bucket = config
                .bucket
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("storage '{}' is missing 'bucket'", config.name))?;
            let access_key = config.access_key.as_deref().ok_or_else(|| {
                anyhow::anyhow!("storage '{}' is missing 'access_key'", config.name)
            })?;
            let secret_key = config.secret_key.as_deref().ok_or_else(|| {
                anyhow::anyhow!("storage '{}' is missing 'secret_key'", config.name)
            })?;
            let region = config.region.as_deref().unwrap_or("us-east-1");
            let store = MinioObjectStore::from_config(
                &config.endpoint,
                bucket,
                access_key,
                secret_key,
                region,
            )
            .await?;
            Ok(Box::new(store))
        }
        #[cfg(feature = "ftp")]
        "ftp" | "ftps" => {
            let store = FtpObjectStore::from_config(config)?;
            Ok(Box::new(store))
        }
        other => Err(anyhow::anyhow!(
            "unknown storage backend type '{}': expected 'ipfs', 's3', or 'ftp'",
            other
        )),
    }
}
