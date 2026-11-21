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
// See src/object/mod.rs: clippy(1.99) double_must_use on async_trait (H15).
#[allow(clippy::double_must_use)]
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

/// Finds the merge-base of two snapshot heads: the common ancestor nearest
/// to `ours` in the snapshot DAG reachable via [`Snapshot::parents`], with
/// ties broken toward the ancestor nearest to `theirs`.
///
/// The single source of truth for merge ancestry is the DAG, never the mutable
/// `Workspace.base` field (which goes stale once merges from other branches
/// land). Callers must use the returned snapshot's tree as the three-way
/// merge base and record the returned ID as the base actually used.
///
/// Note the direction of the selection: a common ancestor one level below
/// `theirs` can sit many commits above `ours` when a merge commit intervenes,
/// so candidate quality is measured by distance to `ours` first (#102) — the
/// first `theirs`-side level that intersects `ours`'s ancestry is not
/// necessarily the best answer.
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

    // BFS out from `theirs` level by level over the whole reachable DAG.
    // #102: the first level intersecting `ours`'s ancestors holds the
    // common ancestor *nearest to `theirs`*, which is not necessarily the
    // one nearest to `ours` — a deeper level can hold a strictly better
    // candidate when a merge commit sits below the first intersection.
    // So keep scanning past the first hit and keep the globally best
    // candidate: minimize the distance to `ours`, breaking ties toward the
    // ancestor nearest to `theirs`.
    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(theirs.0.clone());
    let mut frontier: Vec<(SnapshotId, u64)> = vec![(theirs.clone(), 0)];
    let mut best: Option<(SnapshotId, u64, u64)> = None; // (id, ours-depth, theirs-depth)
    while !frontier.is_empty() {
        let mut next: Vec<(SnapshotId, u64)> = Vec::new();
        for (id, theirs_depth) in &frontier {
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
                    Some(&ours_depth) => {
                        let better = match &best {
                            None => true,
                            Some((_, best_ours, best_theirs)) => {
                                ours_depth < *best_ours
                                    || (ours_depth == *best_ours && theirs_depth + 1 < *best_theirs)
                            }
                        };
                        if better {
                            best = Some((parent.clone(), ours_depth, theirs_depth + 1));
                        }
                    }
                    None => next.push((parent.clone(), theirs_depth + 1)),
                }
            }
        }
        frontier = next;
    }
    if let Some((id, _, _)) = best {
        return Ok(Some(id));
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

        assert_eq!(
            find_merge_base(&store, &a1, &b1).await.unwrap(),
            Some(x.clone())
        );
        assert_eq!(
            find_merge_base(&store, &m1, &a1).await.unwrap(),
            Some(a1.clone())
        );
        assert_eq!(
            find_merge_base(&store, &m1, &b1).await.unwrap(),
            Some(b1.clone())
        );
        assert_eq!(
            find_merge_base(&store, &m1, &m1).await.unwrap(),
            Some(m1.clone())
        );
        // Order of the two heads must not matter.
        assert_eq!(
            find_merge_base(&store, &b1, &a1).await.unwrap(),
            Some(x.clone())
        );
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

/// Regression tests for issue #102: `find_merge_base` used to return at the
/// FIRST `theirs`-side level intersecting `ours`'s ancestors, which can pick
/// a common ancestor far away from `ours` when a merge commit sits below the
/// first intersection. The fix keeps scanning and selects the candidate with
/// the minimum distance to `ours`, breaking ties toward `theirs`.
#[cfg(test)]
mod merge_base_nearest_regression {
    use super::*;

    fn make_store() -> (tempfile::TempDir, RedbSnapshotStore) {
        let tmp = tempfile::TempDir::new().unwrap();
        let db = std::sync::Arc::new(
            redb::Database::builder()
                .create(tmp.path().join("merge-base-regression.redb"))
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

    /// #102 diamond-with-merge probe (the topology from the issue):
    ///
    /// ```mermaid
    /// flowchart TD
    ///     R["R"] --> D["D"]
    ///     R --> L1["L1"]
    ///     L1 --> L2["L2"]
    ///     D --> C["C = merge[L2, D]"]
    ///     L2 --> C
    ///     C --> G["G"]
    ///     G --> E["E"]
    ///     E --> T["T = merge[E, R]"]
    ///     R --> T
    /// ```
    ///
    /// `ours = C`, `theirs = T`. The first `theirs`-side level hit is `R`
    /// (T's parent), but `C` itself is a common ancestor strictly nearer to
    /// `ours` — the answer must be `C`, not `R`.
    #[tokio::test]
    async fn merge_base_prefers_ancestor_nearest_to_ours_over_first_theirs_hit() {
        let (_tmp, store) = make_store();
        put(&store, "noa_r", &[]).await;
        put(&store, "noa_d", &["noa_r"]).await;
        put(&store, "noa_l1", &["noa_r"]).await;
        put(&store, "noa_l2", &["noa_l1"]).await;
        let c = put(&store, "noa_c", &["noa_l2", "noa_d"]).await;
        put(&store, "noa_g", &["noa_c"]).await;
        put(&store, "noa_e", &["noa_g"]).await;
        let t = put(&store, "noa_t", &["noa_e", "noa_r"]).await;

        assert_eq!(
            find_merge_base(&store, &c, &t).await.unwrap(),
            Some(c.clone()),
            "#102: merge base must be the common ancestor nearest to ours (C), \
             not the first theirs-side level hit (R)"
        );
        // Symmetry pin: with the heads swapped the ancestor shortcut must
        // still answer C.
        assert_eq!(
            find_merge_base(&store, &t, &c).await.unwrap(),
            Some(c.clone())
        );
    }

    /// #102 tie-break: two common ancestors equidistant from `ours` — the
    /// one nearer to `theirs` must win.
    ///
    /// ```text
    /// R ── A ── (ours W also merges B)
    ///      \__ B ── C ── (theirs T = merge[A, C])
    /// W = merge[A, B]
    /// ```
    ///
    /// A and B are both one step from `ours`; A is one step from `theirs`
    /// but B is two (via C). Best = (ours-distance 1, theirs-distance 1) → A.
    #[tokio::test]
    async fn merge_base_breaks_ours_distance_ties_toward_nearest_to_theirs() {
        let (_tmp, store) = make_store();
        put(&store, "noa_r2", &[]).await;
        let a = put(&store, "noa_a2", &["noa_r2"]).await;
        put(&store, "noa_b2", &["noa_r2"]).await;
        let w = put(&store, "noa_w2", &["noa_a2", "noa_b2"]).await;
        put(&store, "noa_c2", &["noa_b2"]).await;
        let t = put(&store, "noa_t2", &["noa_a2", "noa_c2"]).await;

        assert_eq!(
            find_merge_base(&store, &w, &t).await.unwrap(),
            Some(a.clone()),
            "#102: equal ours-distance must break the tie toward the ancestor \
             nearest to theirs (A), not the farther one (B)"
        );
    }
}
