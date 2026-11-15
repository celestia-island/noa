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

/// The kind of a [`TreeEntry`], carrying the git file-mode/type that plain
/// [`EntryKind::Blob`] cannot represent.
///
/// Git modes preserved across the git bridge (`src/git/import.rs`,
/// `src/git/export.rs`):
/// * `Executable` — mode `100755`.
/// * `Symlink` — mode `120000`; the entry `id` is a blob id whose bytes are
///   the link-target path (exactly as git stores it).
/// * `Gitlink` — mode `160000` (submodule commit reference); the entry `id`
///   is the referenced git commit oid in hex, NOT a noa object id, because a
///   plain `git clone` never fetches submodule objects.
///
/// Compatibility: `Blob` and `Tree` keep their existing serde encodings
/// (variant order unchanged), so trees containing only plain files and
/// directories deserialize exactly as before and their content hashes are
/// stable. Trees containing the new variants are forward-incompatible: an
/// older binary cannot interpret them. That is inherent — the old model
/// could not represent these modes at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EntryKind {
    Blob,
    Tree,
    Executable,
    Symlink,
    Gitlink,
}

impl EntryKind {
    /// Git tree-entry mode bits for this kind (`040000` for trees).
    #[must_use]
    pub const fn git_mode(self) -> u32 {
        match self {
            EntryKind::Blob => 0o100644,
            EntryKind::Tree => 0o040000,
            EntryKind::Executable => 0o100755,
            EntryKind::Symlink => 0o120000,
            EntryKind::Gitlink => 0o160000,
        }
    }

    /// Map a git tree-entry mode to a kind. Any `0o100xxx` blob mode that is
    /// not executable falls back to [`EntryKind::Blob`]; unknown modes also
    /// fall back to `Blob` so import never fails on them.
    #[must_use]
    pub const fn from_git_mode(mode: u32) -> Self {
        match mode {
            0o040000 => EntryKind::Tree,
            0o100755 => EntryKind::Executable,
            0o120000 => EntryKind::Symlink,
            0o160000 => EntryKind::Gitlink,
            _ => EntryKind::Blob,
        }
    }

    /// True for leaf entries that carry blob bytes in the noa object store
    /// (`Blob`, `Executable`, `Symlink`). `Gitlink` ids are git oids, not
    /// noa blob ids; `Tree` ids are tree ids.
    #[must_use]
    pub const fn has_blob_body(self) -> bool {
        match self {
            EntryKind::Blob | EntryKind::Executable | EntryKind::Symlink => true,
            EntryKind::Tree | EntryKind::Gitlink => false,
        }
    }

    /// True for opaque file-like leaves (everything but `Tree`): content is
    /// compared and replaced by id without recursing.
    #[must_use]
    pub const fn is_file_like(self) -> bool {
        !matches!(self, EntryKind::Tree)
    }
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

// clippy(1.99) double_must_use: every macro-generated BoxFuture is already
// must_use; the attribute below cannot be removed from the expansion (H15).
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait ObjectStore: Send + Sync {
    async fn put_blob(&self, content: &[u8]) -> Result<BlobId>;

    /// Owned counterpart of [`ObjectStore::put_blob`] for callers that already
    /// hold a `Vec<u8>` (file reads, base64 decodes, blob fetches). The default
    /// body forwards to `put_blob`, which is exactly one full-object copy on the
    /// borrowed path; backends whose transport needs to own the buffer (S3
    /// request bodies) override this to take the caller's allocation as-is.
    /// ⚠️ An implementor whose `put_blob` delegates to this method MUST
    /// override this method too — the trait default calls `put_blob`, so a
    /// backend that reverses the delegation without overriding recurses
    /// infinitely at runtime (async_trait boxes the future; no static check).
    async fn put_blob_owned(&self, content: Vec<u8>) -> Result<BlobId> {
        self.put_blob(&content).await
    }

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
            let endpoint = config
                .endpoint
                .as_deref()
                .or(ipfs_env.as_deref())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Store that implements only the borrowed `put_blob`. Its `put_blob_owned`
    /// is therefore the trait's default body, which is what these tests cover.
    #[derive(Default)]
    struct BorrowedOnlyStore {
        blobs: Mutex<HashMap<String, Vec<u8>>>,
    }

    #[async_trait]
    impl ObjectStore for BorrowedOnlyStore {
        async fn put_blob(&self, content: &[u8]) -> Result<BlobId> {
            let id = BlobId(sha256_hex(content));
            self.blobs
                .lock()
                .unwrap()
                .insert(id.0.clone(), content.to_vec());
            Ok(id)
        }

        async fn get_blob(&self, id: &BlobId) -> Result<Vec<u8>> {
            self.blobs
                .lock()
                .unwrap()
                .get(&id.0)
                .cloned()
                .ok_or_else(|| crate::error::NoaError::ObjectNotFound { id: id.0.clone() }.into())
        }

        async fn has_blob(&self, id: &BlobId) -> Result<bool> {
            Ok(self.blobs.lock().unwrap().contains_key(&id.0))
        }

        async fn put_tree(&self, _entries: &TreeEntries) -> Result<TreeId> {
            anyhow::bail!("trees are not part of this test store")
        }

        async fn get_tree(&self, _id: &TreeId) -> Result<TreeEntries> {
            anyhow::bail!("trees are not part of this test store")
        }

        async fn has_tree(&self, _id: &TreeId) -> Result<bool> {
            anyhow::bail!("trees are not part of this test store")
        }
    }

    #[tokio::test]
    async fn default_put_blob_owned_delegates_to_put_blob() {
        let store = BorrowedOnlyStore::default();
        let content = b"the owned and borrowed paths must agree".to_vec();

        let borrowed = store.put_blob(&content).await.unwrap();
        let owned = store.put_blob_owned(content.clone()).await.unwrap();

        assert_eq!(
            owned, borrowed,
            "both entry points must produce the same id"
        );
        assert_eq!(owned.0, sha256_hex(&content));
        assert_eq!(store.get_blob(&owned).await.unwrap(), content);
    }
}
