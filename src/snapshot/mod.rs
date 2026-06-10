mod diff;
mod engine;
mod redb_impl;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub use diff::{diff_snapshots, diff_snapshots_recursive, DiffKind, FileDiff};
pub use engine::SnapshotEngine;
pub use redb_impl::RedbSnapshotStore;

use crate::error::Result;

/// Content-addressed snapshot identifier (e.g. `"noa_<sha256_hex>"` or `"noa_empty"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SnapshotId(pub String);

impl SnapshotId {
    /// Returns the inner string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns `true` if this is the sentinel empty-snapshot ID (`"noa_empty"`).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0 == EMPTY_SNAPSHOT
    }
}

/// Sentinel value representing an empty (no-files) snapshot.
pub const EMPTY_SNAPSHOT: &str = "noa_empty";

/// Returns the sentinel empty-snapshot ID.
#[must_use]
pub fn empty_snapshot_id() -> SnapshotId {
    SnapshotId(EMPTY_SNAPSHOT.to_string())
}

impl std::fmt::Display for SnapshotId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Computes a deterministic, content-addressed snapshot ID from the given fields.
///
/// The hash input includes the tree hash, parent IDs, workspace name, author, and message.
/// Two snapshots with identical inputs will produce the same ID.
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

/// Computes a content-addressed snapshot ID that also includes the timestamp,
/// so that two snapshots created at different times produce different IDs
/// even when all other inputs are identical.
#[must_use]
pub fn content_addressed_snapshot_id_with_ts(
    tree_hash: &str,
    parent_ids: &[SnapshotId],
    workspace: &str,
    author: &str,
    message: &str,
    timestamp: u64,
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
    hasher.update(timestamp.to_le_bytes());
    let hash = hasher.finalize();
    let hex_str = hex::encode(hash);
    SnapshotId(format!("noa_{hex_str}"))
}

/// An immutable point-in-time record of a workspace tree, with metadata and parent links.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    /// Content-addressed snapshot identifier.
    pub id: SnapshotId,
    /// SHA-256 hash of the tree this snapshot points to.
    pub tree_hash: String,
    /// Parent snapshot IDs (one for linear history, two for merge snapshots).
    pub parents: Vec<SnapshotId>,
    /// Name of the workspace this snapshot belongs to.
    pub workspace: String,
    /// Author identifier (typically an agent ID).
    pub author: String,
    /// Timestamp in microseconds since Unix epoch.
    pub timestamp: u64,
    /// Human-readable commit message.
    pub message: String,
}

/// Persistent storage trait for snapshots.
#[async_trait]
pub trait SnapshotStore: Send + Sync {
    /// Retrieves a snapshot by ID.
    async fn get(&self, id: &SnapshotId) -> Result<Snapshot>;
    /// Persists a snapshot.
    async fn store(&self, snapshot: &Snapshot) -> Result<()>;
    /// Returns the IDs of snapshots that list `parent` as one of their parents.
    async fn children_of(&self, parent: &SnapshotId) -> Result<Vec<SnapshotId>>;
    /// Lists all snapshots in the store.
    async fn list_all(&self) -> Result<Vec<Snapshot>>;
}
