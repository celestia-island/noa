use std::collections::BTreeMap;

use crate::error::Result;
use crate::log::{AgentLog, LogEntry, OpType};
use crate::object::{EntryKind, ObjectStore, TreeEntries, TreeEntry};
use crate::snapshot::{generate_snapshot_id, Snapshot, SnapshotId, SnapshotStore};

pub struct SnapshotEngine<L: AgentLog, S: SnapshotStore, O: ObjectStore> {
    pub log: L,
    pub snapshot_store: S,
    pub object_store: O,
}

impl<L: AgentLog, S: SnapshotStore, O: ObjectStore> SnapshotEngine<L, S, O> {
    pub fn new(log: L, snapshot_store: S, object_store: O) -> Self {
        SnapshotEngine {
            log,
            snapshot_store,
            object_store,
        }
    }

    pub async fn compute(
        &self,
        workspace: &str,
        parent_ids: Vec<SnapshotId>,
        author: &str,
        message: &str,
    ) -> Result<Snapshot> {
        let entries = self.log.read_all().await?;
        let tree = self.build_tree_from_entries(&entries).await?;

        let mut sorted = tree;
        sorted.sort();

        let tree_id = self.object_store.put_tree(&sorted).await?;

        let timestamp = chrono::Utc::now().timestamp_micros() as u64;

        let snapshot = Snapshot {
            id: generate_snapshot_id(),
            tree_hash: tree_id.0,
            parents: parent_ids,
            workspace: workspace.to_string(),
            author: author.to_string(),
            timestamp,
            message: message.to_string(),
        };

        self.snapshot_store.store(&snapshot).await?;
        Ok(snapshot)
    }

    async fn build_tree_from_entries(&self, entries: &[LogEntry]) -> Result<TreeEntries> {
        let mut tree_map: BTreeMap<String, TreeEntry> = BTreeMap::new();

        for entry in entries {
            match entry.op {
                OpType::Write => {
                    if let (Some(path), Some(blob_id)) = (&entry.path, &entry.blob_id) {
                        tree_map.insert(
                            path.clone(),
                            TreeEntry {
                                name: path.clone(),
                                kind: EntryKind::Blob,
                                id: blob_id.clone(),
                            },
                        );
                    }
                }
                OpType::Delete => {
                    if let Some(path) = &entry.path {
                        tree_map.remove(path);
                    }
                }
                OpType::Rename => {
                    if let (Some(from), Some(to)) = (&entry.from_path, &entry.path) {
                        if let Some(removed) = tree_map.remove(from) {
                            tree_map.insert(
                                to.clone(),
                                TreeEntry {
                                    name: to.clone(),
                                    kind: removed.kind,
                                    id: removed.id,
                                },
                            );
                        }
                    }
                }
                OpType::Snapshot | OpType::Merge => {}
            }
        }

        Ok(TreeEntries(tree_map.into_values().collect()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::FileAgentLog;
    use crate::object::RedbObjectStore;
    use crate::snapshot::RedbSnapshotStore;
    use std::sync::Arc;
    use tempfile::TempDir;

    async fn make_engine() -> (TempDir, SnapshotEngine<FileAgentLog, RedbSnapshotStore, RedbObjectStore>) {
        let tmp = TempDir::new().unwrap();
        let db = Arc::new(
            redb::Database::builder()
                .create(tmp.path().join("test.redb"))
                .unwrap(),
        );

        let log = FileAgentLog::create(&tmp.path().join("test.log")).unwrap();
        let snapshot_store = RedbSnapshotStore::new(Arc::clone(&db)).unwrap();
        let object_store = RedbObjectStore::new(db).unwrap();

        let engine = SnapshotEngine::new(log, snapshot_store, object_store);
        (tmp, engine)
    }

    fn write_entry(seq: u64, path: &str, blob_id: &str, ts: u64) -> LogEntry {
        LogEntry {
            seq,
            op: OpType::Write,
            path: Some(path.to_string()),
            blob_id: Some(blob_id.to_string()),
            from_path: None,
            snapshot_id: None,
            ts,
            message: None,
        }
    }

    fn delete_entry(seq: u64, path: &str, ts: u64) -> LogEntry {
        LogEntry {
            seq,
            op: OpType::Delete,
            path: Some(path.to_string()),
            blob_id: None,
            from_path: None,
            snapshot_id: None,
            ts,
            message: None,
        }
    }

    #[tokio::test]
    async fn test_compute_snapshot_basic() {
        let (_tmp, engine) = make_engine().await;
        engine.log.append(&write_entry(1, "main.rs", "h1", 100)).await.unwrap();
        engine.log.append(&write_entry(2, "lib.rs", "h2", 200)).await.unwrap();

        let snap = engine.compute("default", vec![], "test", "initial").await.unwrap();
        assert!(snap.id.0.starts_with("noa_"));
        assert_eq!(snap.workspace, "default");
        assert_eq!(snap.message, "initial");

        let stored = engine.snapshot_store.get(&snap.id).await.unwrap();
        assert_eq!(stored.tree_hash, snap.tree_hash);
    }

    #[tokio::test]
    async fn test_compute_with_delete() {
        let (_tmp, engine) = make_engine().await;
        engine.log.append(&write_entry(1, "a.rs", "h1", 100)).await.unwrap();
        engine.log.append(&write_entry(2, "b.rs", "h2", 200)).await.unwrap();
        engine.log.append(&delete_entry(3, "a.rs", 300)).await.unwrap();

        let snap = engine.compute("ws1", vec![], "agent", "delete test").await.unwrap();

        let tree = engine.object_store.get_tree(&crate::object::TreeId(snap.tree_hash)).await.unwrap();
        assert_eq!(tree.0.len(), 1);
        assert_eq!(tree.0[0].name, "b.rs");
    }

    #[tokio::test]
    async fn test_compute_with_parent() {
        let (_tmp, engine) = make_engine().await;
        engine.log.append(&write_entry(1, "x.rs", "h1", 100)).await.unwrap();

        let parent = engine.compute("ws1", vec![], "test", "parent").await.unwrap();

        engine.log.append(&write_entry(2, "y.rs", "h2", 200)).await.unwrap();
        let child = engine.compute("ws1", vec![parent.id.clone()], "test", "child").await.unwrap();

        assert_eq!(child.parents.len(), 1);
        assert_eq!(child.parents[0], parent.id);
    }
}
