pub mod minio_impl;
mod redb_impl;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub use minio_impl::MinioObjectStore;
pub use redb_impl::RedbObjectStore;

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlobId(pub String);

impl BlobId {
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
    pub fn new() -> Self {
        TreeEntries(Vec::new())
    }

    pub fn sort(&mut self) {
        self.0.sort_by(|a, b| a.name.cmp(&b.name));
    }
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
