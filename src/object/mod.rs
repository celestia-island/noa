#[cfg(feature = "ftp")]
pub mod ftp_impl;
pub mod ipfs_impl;
pub mod minio_impl;
mod redb_impl;
pub mod sftp_impl;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

#[cfg(feature = "ftp")]
pub use ftp_impl::FtpObjectStore;
pub use ipfs_impl::IpfsObjectStore;
pub use minio_impl::MinioObjectStore;
pub use redb_impl::RedbObjectStore;
pub use sftp_impl::SftpObjectStore;
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
    match config.backend_type {
        crate::config::StorageProtocol::Ipfs => {
            let ipfs_env = std::env::var("IPFS_API").ok();
            let endpoint = config.endpoint.as_deref()
                .or_else(|| ipfs_env.as_deref())
                .unwrap_or("http://127.0.0.1:5001");
            Ok(Box::new(IpfsObjectStore::new(
                endpoint,
                config.auth_token.clone(),
            )))
        }
        crate::config::StorageProtocol::S3 | crate::config::StorageProtocol::Minio => {
            let store = MinioObjectStore::from_transport_config(config).await?;
            Ok(Box::new(store))
        }
        #[cfg(feature = "ftp")]
        crate::config::StorageProtocol::Ftp | crate::config::StorageProtocol::Ftps => {
            let store = FtpObjectStore::from_config(config)?;
            Ok(Box::new(store))
        }
        crate::config::StorageProtocol::Sftp => {
            let store = SftpObjectStore::from_config(config)?;
            Ok(Box::new(store))
        }
    }
}
