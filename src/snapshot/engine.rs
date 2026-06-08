use std::{
    collections::BTreeMap,
    path::{Component, PathBuf},
};

use crate::{
    error::Result,
    ignore::IgnoreMatcher,
    log::{AgentLog, LogEntry, OpType},
    object::{EntryKind, ObjectStore, TreeEntries, TreeEntry},
    snapshot::{content_addressed_snapshot_id, Snapshot, SnapshotId, SnapshotStore},
};

pub struct SnapshotEngine<L: AgentLog, S: SnapshotStore, O: ObjectStore> {
    pub log: L,
    pub snapshot_store: S,
    pub object_store: O,
    ignore_matcher: Option<IgnoreMatcher>,
    repo_root: Option<PathBuf>,
    compact_on_snapshot: bool,
}

impl<L: AgentLog, S: SnapshotStore, O: ObjectStore> SnapshotEngine<L, S, O> {
    pub fn new(log: L, snapshot_store: S, object_store: O) -> Self {
        SnapshotEngine {
            log,
            snapshot_store,
            object_store,
            ignore_matcher: None,
            repo_root: None,
            compact_on_snapshot: false,
        }
    }

    pub fn with_ignore(mut self, matcher: IgnoreMatcher) -> Self {
        self.ignore_matcher = Some(matcher);
        self
    }

    pub fn with_repo_root(mut self, root: PathBuf) -> Self {
        self.repo_root = Some(root);
        self
    }

    pub fn with_compact_on_snapshot(mut self) -> Self {
        self.compact_on_snapshot = true;
        self
    }

    pub async fn compute(
        &self,
        workspace: &str,
        parent_ids: Vec<SnapshotId>,
        since_seq: u64,
        author: &str,
        message: &str,
    ) -> Result<Snapshot> {
        let tree = if let Some(parent_id) = parent_ids.first() {
            let parent = self.snapshot_store.get(parent_id).await?;
            let parent_tree = self
                .object_store
                .get_tree(&crate::object::TreeId(parent.tree_hash))
                .await?;
            let entries = if since_seq > 0 {
                self.log.read_since(since_seq).await?
            } else {
                self.log.read_all().await?
            };
            self.build_tree_from_entries_with_base(&parent_tree.0, &entries)
                .await?
        } else {
            let entries = self.log.read_all().await?;
            self.build_tree_from_entries(&entries).await?
        };

        let mut sorted = tree;
        sorted.sort();

        let tree_id = self.object_store.put_tree(&sorted).await?;

        let timestamp = chrono::Utc::now().timestamp_micros() as u64;

        let id = content_addressed_snapshot_id(&tree_id.0, &parent_ids, workspace);

        let snapshot = Snapshot {
            id,
            tree_hash: tree_id.0,
            parents: parent_ids,
            workspace: workspace.to_string(),
            author: author.to_string(),
            timestamp,
            message: message.to_string(),
        };

        self.snapshot_store.store(&snapshot).await?;

        if self.compact_on_snapshot {
            if let Ok(all) = self.log.read_all().await {
                if let Some(max_seq) = all.iter().map(|e| e.seq).max() {
                    let _ = self.log.compact_to(max_seq).await;
                }
            }
        }

        Ok(snapshot)
    }

    async fn build_tree_from_entries(&self, entries: &[LogEntry]) -> Result<TreeEntries> {
        self.build_tree_from_entries_with_base(&[], entries).await
    }

    fn is_path_within_root(path: &std::path::Path) -> bool {
        for component in path.components() {
            match component {
                Component::ParentDir | Component::RootDir | Component::Prefix(_) => return false,
                _ => {}
            }
        }
        true
    }

    async fn build_tree_from_entries_with_base(
        &self,
        base: &[TreeEntry],
        entries: &[LogEntry],
    ) -> Result<TreeEntries> {
        let mut tree_map: BTreeMap<String, TreeEntry> =
            base.iter().map(|e| (e.name.clone(), e.clone())).collect();

        for entry in entries {
            match entry.op {
                OpType::Write => {
                    if let (Some(path), Some(log_blob_id)) = (&entry.path, &entry.blob_id) {
                        if let Some(ref matcher) = self.ignore_matcher {
                            if matcher.should_skip(path, false) {
                                continue;
                            }
                        }
                        if let Some(ref repo_root) = self.repo_root {
                            if !Self::is_path_within_root(PathBuf::from(path).as_path()) {
                                tracing::warn!("skipping path traversal in log entry: {}", path);
                                continue;
                            }
                        }
                        let blob_id = if let Some(ref repo_root) = self.repo_root {
                            let file_path = repo_root.join(path);
                            match std::fs::read(&file_path) {
                                Ok(content) => self.object_store.put_blob(&content).await?.0,
                                Err(_) => log_blob_id.clone(),
                            }
                        } else {
                            log_blob_id.clone()
                        };
                        tree_map.insert(
                            path.clone(),
                            TreeEntry {
                                name: path.clone(),
                                kind: EntryKind::Blob,
                                id: blob_id,
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
                        if let Some(ref matcher) = self.ignore_matcher {
                            if matcher.should_skip(to, false) {
                                tree_map.remove(from);
                                continue;
                            }
                        }
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
                OpType::Resolve => {
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

    async fn make_engine() -> (
        TempDir,
        SnapshotEngine<FileAgentLog, RedbSnapshotStore, RedbObjectStore>,
    ) {
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
            resolved_conflict_ours_id: None,
            resolved_conflict_theirs_id: None,
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
            resolved_conflict_ours_id: None,
            resolved_conflict_theirs_id: None,
            snapshot_id: None,
            ts,
            message: None,
        }
    }

    #[tokio::test]
    async fn test_compute_snapshot_basic() {
        let (_tmp, engine) = make_engine().await;
        engine
            .log
            .append(&write_entry(1, "main.rs", "h1", 100))
            .await
            .unwrap();
        engine
            .log
            .append(&write_entry(2, "lib.rs", "h2", 200))
            .await
            .unwrap();

        let snap = engine
            .compute("default", vec![], 0, "test", "initial")
            .await
            .unwrap();
        assert!(snap.id.0.starts_with("noa_"));
        assert_eq!(snap.workspace, "default");
        assert_eq!(snap.message, "initial");

        let stored = engine.snapshot_store.get(&snap.id).await.unwrap();
        assert_eq!(stored.tree_hash, snap.tree_hash);
    }

    #[tokio::test]
    async fn test_compute_with_delete() {
        let (_tmp, engine) = make_engine().await;
        engine
            .log
            .append(&write_entry(1, "a.rs", "h1", 100))
            .await
            .unwrap();
        engine
            .log
            .append(&write_entry(2, "b.rs", "h2", 200))
            .await
            .unwrap();
        engine
            .log
            .append(&delete_entry(3, "a.rs", 300))
            .await
            .unwrap();

        let snap = engine
            .compute("ws1", vec![], 0, "agent", "delete test")
            .await
            .unwrap();

        let tree = engine
            .object_store
            .get_tree(&crate::object::TreeId(snap.tree_hash))
            .await
            .unwrap();
        assert_eq!(tree.0.len(), 1);
        assert_eq!(tree.0[0].name, "b.rs");
    }

    #[tokio::test]
    async fn test_compute_with_parent() {
        let (_tmp, engine) = make_engine().await;
        engine
            .log
            .append(&write_entry(1, "x.rs", "h1", 100))
            .await
            .unwrap();

        let parent = engine
            .compute("ws1", vec![], 0, "test", "parent")
            .await
            .unwrap();

        engine
            .log
            .append(&write_entry(2, "y.rs", "h2", 200))
            .await
            .unwrap();
        let child = engine
            .compute("ws1", vec![parent.id.clone()], 0, "test", "child")
            .await
            .unwrap();

        assert_eq!(child.parents.len(), 1);
        assert_eq!(child.parents[0], parent.id);
    }

    #[tokio::test]
    async fn test_ignore_filters_noa_paths() {
        let tmp = TempDir::new().unwrap();
        let db = Arc::new(
            redb::Database::builder()
                .create(tmp.path().join("test.redb"))
                .unwrap(),
        );

        let log = FileAgentLog::create(&tmp.path().join("test.log")).unwrap();
        let snapshot_store = RedbSnapshotStore::new(Arc::clone(&db)).unwrap();
        let object_store = RedbObjectStore::new(db).unwrap();

        let matcher = IgnoreMatcher::from_repo_root(tmp.path());
        let engine = SnapshotEngine::new(log, snapshot_store, object_store).with_ignore(matcher);

        engine
            .log
            .append(&write_entry(1, "main.rs", "h1", 100))
            .await
            .unwrap();
        engine
            .log
            .append(&write_entry(2, ".noa/config", "h2", 200))
            .await
            .unwrap();
        engine
            .log
            .append(&write_entry(3, ".noa/noa.redb", "h3", 300))
            .await
            .unwrap();

        let snap = engine
            .compute("default", vec![], 0, "test", "ignore noa")
            .await
            .unwrap();
        let tree = engine
            .object_store
            .get_tree(&crate::object::TreeId(snap.tree_hash))
            .await
            .unwrap();
        assert_eq!(tree.0.len(), 1);
        assert_eq!(tree.0[0].name, "main.rs");
    }

    #[tokio::test]
    async fn test_ignore_filters_gitignore_patterns() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(".gitignore"), "*.log\ntarget/\n").unwrap();

        let db = Arc::new(
            redb::Database::builder()
                .create(tmp.path().join("test.redb"))
                .unwrap(),
        );

        let log = FileAgentLog::create(&tmp.path().join("test.log")).unwrap();
        let snapshot_store = RedbSnapshotStore::new(Arc::clone(&db)).unwrap();
        let object_store = RedbObjectStore::new(db).unwrap();

        let matcher = IgnoreMatcher::from_repo_root(tmp.path());
        let engine = SnapshotEngine::new(log, snapshot_store, object_store).with_ignore(matcher);

        engine
            .log
            .append(&write_entry(1, "main.rs", "h1", 100))
            .await
            .unwrap();
        engine
            .log
            .append(&write_entry(2, "debug.log", "h2", 200))
            .await
            .unwrap();
        engine
            .log
            .append(&write_entry(3, "target/dep.rs", "h3", 300))
            .await
            .unwrap();

        let snap = engine
            .compute("default", vec![], 0, "test", "ignore gitignore")
            .await
            .unwrap();
        let tree = engine
            .object_store
            .get_tree(&crate::object::TreeId(snap.tree_hash))
            .await
            .unwrap();
        assert_eq!(tree.0.len(), 1);
        assert_eq!(tree.0[0].name, "main.rs");
    }

    #[tokio::test]
    async fn test_ignore_allows_whitelisted() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join(".gitignore"), "*.log\n!important.log\n").unwrap();

        let db = Arc::new(
            redb::Database::builder()
                .create(tmp.path().join("test.redb"))
                .unwrap(),
        );

        let log = FileAgentLog::create(&tmp.path().join("test.log")).unwrap();
        let snapshot_store = RedbSnapshotStore::new(Arc::clone(&db)).unwrap();
        let object_store = RedbObjectStore::new(db).unwrap();

        let matcher = IgnoreMatcher::from_repo_root(tmp.path());
        let engine = SnapshotEngine::new(log, snapshot_store, object_store).with_ignore(matcher);

        engine
            .log
            .append(&write_entry(1, "important.log", "h1", 100))
            .await
            .unwrap();
        engine
            .log
            .append(&write_entry(2, "debug.log", "h2", 200))
            .await
            .unwrap();

        let snap = engine
            .compute("default", vec![], 0, "test", "whitelist")
            .await
            .unwrap();
        let tree = engine
            .object_store
            .get_tree(&crate::object::TreeId(snap.tree_hash))
            .await
            .unwrap();
        assert_eq!(tree.0.len(), 1);
        assert_eq!(tree.0[0].name, "important.log");
    }
}
