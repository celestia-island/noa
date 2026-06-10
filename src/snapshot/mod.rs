mod diff;
mod engine;
mod redb_impl;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub use diff::{diff_snapshots, diff_snapshots_recursive, DiffKind, FileDiff};
pub use engine::SnapshotEngine;
pub use redb_impl::RedbSnapshotStore;

use crate::error::Result;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SnapshotId(pub String);

impl SnapshotId {
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0 == EMPTY_SNAPSHOT
    }
}

pub const EMPTY_SNAPSHOT: &str = "noa_empty";

#[must_use]
pub fn empty_snapshot_id() -> SnapshotId {
    SnapshotId(EMPTY_SNAPSHOT.to_string())
}

impl std::fmt::Display for SnapshotId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[must_use]
pub fn content_addressed_snapshot_id(
    tree_hash: &str,
    parent_ids: &[SnapshotId],
    workspace: &str,
    author: &str,
    message: &str,
) -> SnapshotId {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(tree_hash.as_bytes());
    for p in parent_ids {
        hasher.update(p.0.as_bytes());
    }
    hasher.update(workspace.as_bytes());
    hasher.update(author.as_bytes());
    hasher.update(message.as_bytes());
    let hash = hasher.finalize();
    let hex_str = hex::encode(hash);
    SnapshotId(format!("noa_{hex_str}"))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: SnapshotId,
    pub tree_hash: String,
    pub parents: Vec<SnapshotId>,
    pub workspace: String,
    pub author: String,
    /// Timestamp in microseconds since Unix epoch
    pub timestamp: u64,
    pub message: String,
}

#[async_trait]
pub trait SnapshotStore: Send + Sync {
    async fn get(&self, id: &SnapshotId) -> Result<Snapshot>;
    async fn store(&self, snapshot: &Snapshot) -> Result<()>;
    async fn children_of(&self, parent: &SnapshotId) -> Result<Vec<SnapshotId>>;
    async fn list_all(&self) -> Result<Vec<Snapshot>>;
}
