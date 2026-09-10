mod diff;
mod engine;
mod redb_impl;

use std::collections::{HashMap, HashSet, VecDeque};

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub use diff::{diff_snapshots, diff_snapshots_recursive, DiffKind, FileDiff};
pub use engine::SnapshotEngine;
pub use redb_impl::RedbSnapshotStore;

use crate::error::{is_snapshot_not_found, Result};

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

/// Finds the merge-base of two snapshot heads: the nearest common ancestor in
/// the snapshot DAG reachable via [`Snapshot::parents`].
///
/// The single source of truth for merge ancestry is the DAG, never the mutable
/// `Workspace.base` field (which goes stale once merges from other branches
/// land). Callers must use the returned snapshot's tree as the three-way
/// merge base and record the returned ID as the base actually used.
///
/// Returns `Ok(None)` when the heads share no ancestor — including when either
/// head is the empty-snapshot sentinel or unknown to the store — in which case
/// callers should merge against the empty tree. Snapshots missing from the
/// store are treated as roots rather than errors, so a partially-available DAG
/// still yields the best ancestor visible from both sides; genuine storage
/// failures are propagated.
pub async fn find_merge_base<S: SnapshotStore>(
    store: &S,
    ours: &SnapshotId,
    theirs: &SnapshotId,
) -> Result<Option<SnapshotId>> {
    if ours.is_empty() || theirs.is_empty() {
        return Ok(None);
    }
    if ours == theirs {
        return Ok(Some(ours.clone()));
    }

    // BFS out from `ours`, recording the distance of every reachable ancestor.
    let mut ours_depth: HashMap<String, u64> = HashMap::new();
    let mut queue: VecDeque<(SnapshotId, u64)> = VecDeque::new();
    ours_depth.insert(ours.0.clone(), 0);
    queue.push_back((ours.clone(), 0));
    while let Some((id, depth)) = queue.pop_front() {
        let snapshot = match store.get(&id).await {
            Ok(snapshot) => snapshot,
            Err(e) if is_snapshot_not_found(&e) => continue,
            Err(e) => return Err(e),
        };
        for parent in &snapshot.parents {
            if parent.is_empty() || ours_depth.contains_key(&parent.0) {
                continue;
            }
            ours_depth.insert(parent.0.clone(), depth + 1);
            queue.push_back((parent.clone(), depth + 1));
        }
    }

    // `theirs` itself is an ancestor of `ours`: it is the merge-base.
    if ours_depth.contains_key(&theirs.0) {
        return Ok(Some(theirs.clone()));
    }

    // BFS out from `theirs` level by level; the first level intersecting
    // `ours`'s ancestors holds the nearest common ancestor(s) to `theirs`.
    // Ties on one level break toward the ancestor nearest to `ours`.
    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(theirs.0.clone());
    let mut frontier = vec![theirs.clone()];
    while !frontier.is_empty() {
        let mut next = Vec::new();
        let mut best: Option<(SnapshotId, u64)> = None;
        for id in &frontier {
            let snapshot = match store.get(id).await {
                Ok(snapshot) => snapshot,
                Err(e) if is_snapshot_not_found(&e) => continue,
                Err(e) => return Err(e),
            };
            for parent in &snapshot.parents {
                if parent.is_empty() || !visited.insert(parent.0.clone()) {
                    continue;
                }
                match ours_depth.get(&parent.0) {
                    Some(&depth) => {
                        let replace = match &best {
                            None => true,
                            Some((_, best_depth)) => depth < *best_depth,
                        };
                        if replace {
                            best = Some((parent.clone(), depth));
                        }
                    }
                    None => next.push(parent.clone()),
                }
            }
        }
        if let Some((id, _)) = best {
            return Ok(Some(id));
        }
        frontier = next;
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_store() -> (tempfile::TempDir, RedbSnapshotStore) {
        let tmp = tempfile::TempDir::new().unwrap();
        let db = std::sync::Arc::new(
            redb::Database::builder()
                .create(tmp.path().join("merge-base-test.redb"))
                .unwrap(),
        );
        let store = RedbSnapshotStore::new(db).unwrap();
        (tmp, store)
    }

    async fn put(store: &RedbSnapshotStore, id: &str, parents: &[&str]) -> SnapshotId {
        let snapshot = Snapshot {
            id: SnapshotId(id.to_string()),
            tree_hash: format!("tree-{id}"),
            parents: parents
                .iter()
                .map(|p| SnapshotId((*p).to_string()))
                .collect(),
            workspace: "w".to_string(),
            author: "test".to_string(),
            timestamp: 0,
            message: "test".to_string(),
        };
        store.store(&snapshot).await.unwrap();
        snapshot.id
    }

    #[tokio::test]
    async fn test_find_merge_base_diamond() {
        let (_tmp, store) = make_store();
        let x = put(&store, "noa_x", &[]).await;
        let a1 = put(&store, "noa_a1", &["noa_x"]).await;
        let b1 = put(&store, "noa_b1", &["noa_x"]).await;
        let m1 = put(&store, "noa_m1", &["noa_a1", "noa_b1"]).await;

        assert_eq!(find_merge_base(&store, &a1, &b1).await.unwrap(), Some(x.clone()));
        assert_eq!(find_merge_base(&store, &m1, &a1).await.unwrap(), Some(a1.clone()));
        assert_eq!(find_merge_base(&store, &m1, &b1).await.unwrap(), Some(b1.clone()));
        assert_eq!(find_merge_base(&store, &m1, &m1).await.unwrap(), Some(m1.clone()));
        // Order of the two heads must not matter.
        assert_eq!(find_merge_base(&store, &b1, &a1).await.unwrap(), Some(x.clone()));
    }

    #[tokio::test]
    async fn test_find_merge_base_none_without_common_ancestor() {
        let (_tmp, store) = make_store();
        let r1 = put(&store, "noa_r1", &[]).await;
        let r2 = put(&store, "noa_r2", &[]).await;
        assert_eq!(find_merge_base(&store, &r1, &r2).await.unwrap(), None);
        assert_eq!(
            find_merge_base(&store, &r1, &empty_snapshot_id())
                .await
                .unwrap(),
            None
        );
        // Unknown heads are treated as roots, not errors.
        let missing = SnapshotId("noa_missing".to_string());
        assert_eq!(find_merge_base(&store, &r1, &missing).await.unwrap(), None);
    }
}
