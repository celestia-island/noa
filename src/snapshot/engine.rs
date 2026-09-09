use std::{
    collections::{BTreeMap, VecDeque},
    path::{Component, PathBuf},
};

use crate::{
    error::Result,
    ignore::IgnoreMatcher,
    log::{AgentLog, LogEntry, OpType},
    merge::{self, ConflictResolution},
    object::{EntryKind, ObjectStore, TreeEntries, TreeEntry, TreeId},
    snapshot::{content_addressed_snapshot_id_with_ts, Snapshot, SnapshotId, SnapshotStore},
};

pub struct SnapshotEngine<L: AgentLog, S: SnapshotStore, O: ObjectStore> {
    pub log: L,
    pub snapshot_store: S,
    pub object_store: O,
    ignore_matcher: Option<IgnoreMatcher>,
    repo_root: Option<PathBuf>,
    conflict_resolution: ConflictResolution,
}

impl<L: AgentLog, S: SnapshotStore, O: ObjectStore + Clone + 'static> SnapshotEngine<L, S, O> {
    pub fn new(log: L, snapshot_store: S, object_store: O) -> Self {
        SnapshotEngine {
            log,
            snapshot_store,
            object_store,
            ignore_matcher: None,
            repo_root: None,
            conflict_resolution: ConflictResolution::Theirs,
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

    pub fn with_conflict_resolution(mut self, resolution: ConflictResolution) -> Self {
        self.conflict_resolution = resolution;
        self
    }

    /// Build phase (pure): fold log entries over the parent tree(s) and return
    /// the snapshot value WITHOUT persisting it and WITHOUT compacting the log.
    ///
    /// Tree objects written here are content-addressed and idempotent, so a
    /// discarded build leaves no observable residue. The snapshot object itself
    /// is NOT stored: persisting it before the publication CAS wins would strand
    /// an unreachable orphan on every CAS loss (issue #74). The commit path must
    /// store the snapshot only after `cas()` returns `true`.
    pub async fn build(
        &self,
        workspace: &str,
        parent_ids: Vec<SnapshotId>,
        since_seq: u64,
        author: &str,
        message: &str,
    ) -> Result<Snapshot> {
        let tree = if parent_ids.len() > 1 {
            let first_parent = self.snapshot_store.get(&parent_ids[0]).await?;
            let first_tree = self
                .object_store
                .get_tree(&crate::object::TreeId(first_parent.tree_hash))
                .await?;
            let mut merged_tree = first_tree.clone();
            for extra_parent_id in &parent_ids[1..] {
                let extra_parent = self.snapshot_store.get(extra_parent_id).await?;
                let extra_tree = self
                    .object_store
                    .get_tree(&crate::object::TreeId(extra_parent.tree_hash))
                    .await?;
                let merge_result = merge::merge_trees_recursive(
                    first_tree.clone(),
                    merged_tree,
                    extra_tree,
                    self.object_store.clone(),
                    &self.conflict_resolution,
                )
                .await?;
                merged_tree = merge_result.into_tree_entries(&self.conflict_resolution);
            }
            let entries = if since_seq > 0 {
                self.log.read_since(since_seq).await?
            } else {
                self.log.read_all().await?
            };
            self.build_tree_from_entries_with_base(&merged_tree.0, &entries)
                .await?
        } else if let Some(parent_id) = parent_ids.first() {
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

        let timestamp = crate::now_micros();

        let id = content_addressed_snapshot_id_with_ts(
            &tree_id.0,
            &parent_ids,
            workspace,
            author,
            message,
            timestamp,
        );

        let snapshot = Snapshot {
            id,
            tree_hash: tree_id.0,
            parents: parent_ids,
            workspace: workspace.to_string(),
            author: author.to_string(),
            timestamp,
            message: message.to_string(),
        };

        Ok(snapshot)
    }

    /// Build + persist the snapshot object, without compacting the log.
    ///
    /// Convenience for direct callers (tests, single-writer flows) that publish
    /// without a competing CAS. The multi-writer commit path (`run_create`) must
    /// use [`Self::build`] and store only after the publication CAS succeeds,
    /// then compact via [`Self::compact_committed`].
    pub async fn compute(
        &self,
        workspace: &str,
        parent_ids: Vec<SnapshotId>,
        since_seq: u64,
        author: &str,
        message: &str,
    ) -> Result<Snapshot> {
        let snapshot = self
            .build(workspace, parent_ids, since_seq, author, message)
            .await?;
        self.snapshot_store.store(&snapshot).await?;
        Ok(snapshot)
    }

    /// Commit-phase log compaction. Call ONLY on the CAS winner, after the
    /// snapshot object is stored and the workspace head/seq advanced — and
    /// always last: compaction is destructive, and running it before a
    /// successful publication CAS destroys pending entries a retry can no
    /// longer rebuild (issue #74).
    pub async fn compact_committed(&self, up_to_seq: u64) -> Result<()> {
        if up_to_seq == 0 {
            return Ok(());
        }
        self.log.compact_to(up_to_seq).await
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

    async fn flatten_base_entries(
        &self,
        base: &[TreeEntry],
        map: &mut BTreeMap<String, TreeEntry>,
    ) -> Result<()> {
        let mut stack: Vec<(String, String, EntryKind)> = Vec::new();
        for entry in base {
            stack.push((entry.name.clone(), entry.id.clone(), entry.kind));
        }
        while let Some((path, id, kind)) = stack.pop() {
            if kind == EntryKind::Tree {
                let sub_tree = self.object_store.get_tree(&TreeId(id)).await?;
                for child in &sub_tree.0 {
                    let child_path = format!("{}/{}", path, child.name);
                    stack.push((child_path, child.id.clone(), child.kind));
                }
            } else {
                map.insert(
                    path.clone(),
                    TreeEntry {
                        name: path,
                        kind: EntryKind::Blob,
                        id,
                    },
                );
            }
        }
        Ok(())
    }

    /// Build a hierarchical tree from a flat map.
    /// Decomposes into layers by depth (top-down), then rebuilds bottom-up, storing sub-trees.
    /// Uses a FIFO worklist during decomposition to ensure sibling directories at the same
    /// depth are processed in order, and a position-based resolved list during assembly
    /// so that sibling directories with same-named children do not collide.
    async fn build_hierarchical_tree(
        &self,
        flat_map: &BTreeMap<String, TreeEntry>,
    ) -> Result<TreeEntries> {
        type LayerEntry = (Vec<TreeEntry>, Vec<(String, Vec<TreeEntry>)>);
        // Phase 1: decompose into layers (top-down, FIFO order)
        let mut layers: Vec<LayerEntry> = Vec::new();
        let mut work_queue: VecDeque<Vec<(String, TreeEntry)>> = VecDeque::new();
        work_queue.push_back(
            flat_map
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect(),
        );

        while let Some(current) = work_queue.pop_front() {
            let mut roots: Vec<TreeEntry> = Vec::new();
            let mut dirs: BTreeMap<String, Vec<(String, TreeEntry)>> = BTreeMap::new();

            for (path, entry) in current {
                if let Some((dir, rest)) = path.split_once('/') {
                    let mut child = entry.clone();
                    child.name = rest.to_string();
                    dirs.entry(dir.to_string())
                        .or_default()
                        .push((rest.to_string(), child));
                } else {
                    roots.push(entry);
                }
            }

            let mut subdirs: Vec<(String, Vec<TreeEntry>)> = Vec::new();
            for (dir_name, children) in dirs {
                let deep: Vec<_> = children
                    .iter()
                    .filter(|(k, _)| k.contains('/'))
                    .cloned()
                    .collect();
                let leaf: Vec<_> = children
                    .iter()
                    .filter(|(k, _)| !k.contains('/'))
                    .cloned()
                    .collect();

                if !deep.is_empty() {
                    work_queue.push_back(deep.clone());
                }

                let mut entries: Vec<TreeEntry> = leaf.into_iter().map(|(_, e)| e).collect();
                for (deep_path, _) in &deep {
                    if let Some((immediate, _)) = deep_path.split_once('/') {
                        if !entries.iter().any(|e| e.name == immediate) {
                            entries.push(TreeEntry {
                                name: immediate.to_string(),
                                kind: EntryKind::Tree,
                                id: String::new(),
                            });
                        }
                    }
                }
                subdirs.push((dir_name, entries));
            }

            layers.push((roots, subdirs));
        }

        // Phase 2: rebuild bottom-up, drain resolved into a name-indexed map so that
        // sibling directories with same-named children each get the correct sub-tree.
        // Uses a HashMap for O(1) lookup + O(1) pop instead of O(n) position() + O(n) remove().
        let mut resolved: Vec<(String, TreeEntry)> = Vec::new();

        for (roots, subdirs) in layers.into_iter().rev() {
            let mut level_map = BTreeMap::new();

            for entry in roots {
                level_map.insert(entry.name.clone(), entry);
            }

            let mut resolved_by_name: std::collections::HashMap<String, Vec<TreeEntry>> =
                std::collections::HashMap::new();
            for (_, entry) in std::mem::take(&mut resolved) {
                resolved_by_name
                    .entry(entry.name.clone())
                    .or_default()
                    .push(entry);
            }

            for (dir_name, children) in subdirs {
                let mut sub_entries = Vec::new();
                for entry in children {
                    if entry.kind == EntryKind::Tree && entry.id.is_empty() {
                        if let Some(entries) = resolved_by_name.get_mut(&entry.name) {
                            if let Some(resolved_entry) = entries.pop() {
                                sub_entries.push(resolved_entry);
                            } else {
                                tracing::warn!(
                                    "unresolved subdirectory placeholder: {} in {}",
                                    entry.name,
                                    dir_name
                                );
                            }
                        } else {
                            tracing::warn!(
                                "unresolved subdirectory placeholder: {} in {}",
                                entry.name,
                                dir_name
                            );
                        }
                    } else if let Some(entries) = resolved_by_name.get_mut(&entry.name) {
                        if let Some(resolved_entry) = entries.pop() {
                            sub_entries.push(resolved_entry);
                        } else {
                            sub_entries.push(entry);
                        }
                    } else {
                        sub_entries.push(entry);
                    }
                }
                let sub_tree = TreeEntries(sub_entries);
                let tree_id = self.object_store.put_tree(&sub_tree).await?;
                level_map.insert(
                    dir_name.clone(),
                    TreeEntry {
                        name: dir_name,
                        kind: EntryKind::Tree,
                        id: tree_id.0,
                    },
                );
            }

            let remaining: Vec<(String, TreeEntry)> = resolved_by_name
                .into_values()
                .flat_map(|entries| entries.into_iter().map(|e| (e.name.clone(), e)))
                .collect();
            let mut new_resolved: Vec<(String, TreeEntry)> = level_map.into_iter().collect();
            new_resolved.extend(remaining);
            resolved = new_resolved;
        }

        Ok(TreeEntries(resolved.into_iter().map(|(_, v)| v).collect()))
    }

    async fn build_tree_from_entries_with_base(
        &self,
        base: &[TreeEntry],
        entries: &[LogEntry],
    ) -> Result<TreeEntries> {
        let mut tree_map: BTreeMap<String, TreeEntry> = BTreeMap::new();

        self.flatten_base_entries(base, &mut tree_map).await?;

        for entry in entries {
            match entry.op {
                OpType::Write => {
                    if let (Some(path), Some(log_blob_id)) = (&entry.path, &entry.blob_id) {
                        if let Some(ref matcher) = self.ignore_matcher {
                            if matcher.should_skip(path, false) {
                                continue;
                            }
                        }
                        if !Self::is_path_within_root(PathBuf::from(path).as_path()) {
                            tracing::warn!("skipping path traversal in log entry: {}", path);
                            continue;
                        }
                        let blob_id = if let Some(ref repo_root) = self.repo_root {
                            let file_path = repo_root.join(path);
                            let blob_id = match tokio::fs::read(&file_path).await {
                                Ok(content) => self.object_store.put_blob(&content).await?.0,
                                Err(e) => {
                                    tracing::warn!(
                                        "failed to read {}, keeping previous blob: {e}",
                                        path
                                    );
                                    log_blob_id.clone()
                                }
                            };
                            blob_id
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
                        if !Self::is_path_within_root(PathBuf::from(from).as_path())
                            || !Self::is_path_within_root(PathBuf::from(to).as_path())
                        {
                            tracing::warn!("skipping path traversal in rename: {} -> {}", from, to);
                            continue;
                        }
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
                        if !Self::is_path_within_root(PathBuf::from(path).as_path()) {
                            tracing::warn!("skipping path traversal in resolve: {}", path);
                            continue;
                        }
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

        self.build_hierarchical_tree(&tree_map).await
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

    #[tokio::test]
    async fn test_path_traversal_entries_skipped() {
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

        engine
            .log
            .append(&write_entry(1, "src/main.rs", "h1", 100))
            .await
            .unwrap();
        engine
            .log
            .append(&write_entry(2, "../../../etc/passwd", "h2", 200))
            .await
            .unwrap();
        engine
            .log
            .append(&write_entry(3, "/absolute/path.rs", "h3", 300))
            .await
            .unwrap();
        engine
            .log
            .append(&write_entry(4, "normal/file.rs", "h4", 400))
            .await
            .unwrap();

        let snap = engine
            .compute("default", vec![], 0, "test", "traversal test")
            .await
            .unwrap();

        let tree = engine
            .object_store
            .get_tree(&crate::object::TreeId(snap.tree_hash))
            .await
            .unwrap();

        let names: Vec<&str> = tree.0.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"src"));
        assert!(names.contains(&"normal"));
        assert!(!names.iter().any(|n| n.contains("..")));
        assert!(!names.iter().any(|n| n.starts_with('/')));
        assert_eq!(tree.0.len(), 2);
        // Verify sub-trees contain the expected files
        if let Some(src_entry) = tree.0.iter().find(|e| e.name == "src") {
            assert_eq!(src_entry.kind, crate::object::EntryKind::Tree);
            let src_tree = engine
                .object_store
                .get_tree(&TreeId(src_entry.id.clone()))
                .await
                .unwrap();
            assert_eq!(src_tree.0.len(), 1);
            assert_eq!(src_tree.0[0].name, "main.rs");
        }
        if let Some(normal_entry) = tree.0.iter().find(|e| e.name == "normal") {
            assert_eq!(normal_entry.kind, crate::object::EntryKind::Tree);
            let normal_tree = engine
                .object_store
                .get_tree(&TreeId(normal_entry.id.clone()))
                .await
                .unwrap();
            assert_eq!(normal_tree.0.len(), 1);
            assert_eq!(normal_tree.0[0].name, "file.rs");
        }
    }

    #[test]
    fn test_is_path_within_root_valid() {
        assert!(SnapshotEngine::<
            crate::log::FileAgentLog,
            crate::snapshot::RedbSnapshotStore,
            crate::object::RedbObjectStore,
        >::is_path_within_root(
            PathBuf::from("src/main.rs").as_path()
        ));
        assert!(SnapshotEngine::<
            crate::log::FileAgentLog,
            crate::snapshot::RedbSnapshotStore,
            crate::object::RedbObjectStore,
        >::is_path_within_root(
            PathBuf::from("a/b/c").as_path()
        ));
    }

    #[test]
    fn test_is_path_within_root_traversal() {
        assert!(!SnapshotEngine::<
            crate::log::FileAgentLog,
            crate::snapshot::RedbSnapshotStore,
            crate::object::RedbObjectStore,
        >::is_path_within_root(
            PathBuf::from("../etc/passwd").as_path()
        ));
        assert!(!SnapshotEngine::<
            crate::log::FileAgentLog,
            crate::snapshot::RedbSnapshotStore,
            crate::object::RedbObjectStore,
        >::is_path_within_root(
            PathBuf::from("/absolute").as_path()
        ));
        assert!(!SnapshotEngine::<
            crate::log::FileAgentLog,
            crate::snapshot::RedbSnapshotStore,
            crate::object::RedbObjectStore,
        >::is_path_within_root(
            PathBuf::from("foo/../../bar").as_path()
        ));
    }

    #[tokio::test]
    async fn test_compute_empty_log() {
        let (_tmp, engine) = make_engine().await;
        let snap = engine
            .compute("default", vec![], 0, "test", "empty")
            .await
            .unwrap();
        let tree = engine
            .object_store
            .get_tree(&crate::object::TreeId(snap.tree_hash))
            .await
            .unwrap();
        assert!(tree.0.is_empty());
    }

    #[tokio::test]
    async fn test_compute_delete_only_log() {
        let (_tmp, engine) = make_engine().await;
        engine
            .log
            .append(&delete_entry(1, "nonexistent.rs", 100))
            .await
            .unwrap();
        let snap = engine
            .compute("default", vec![], 0, "test", "delete-only")
            .await
            .unwrap();
        let tree = engine
            .object_store
            .get_tree(&crate::object::TreeId(snap.tree_hash))
            .await
            .unwrap();
        assert!(tree.0.is_empty());
    }

    #[tokio::test]
    async fn test_compute_rename_chain() {
        let (_tmp, engine) = make_engine().await;
        engine
            .log
            .append(&write_entry(1, "a.rs", "h1", 100))
            .await
            .unwrap();

        let rename_entry = LogEntry {
            seq: 2,
            op: OpType::Rename,
            path: Some("b.rs".to_string()),
            blob_id: None,
            from_path: Some("a.rs".to_string()),
            resolved_conflict_ours_id: None,
            resolved_conflict_theirs_id: None,
            snapshot_id: None,
            ts: 200,
            message: None,
        };
        engine.log.append(&rename_entry).await.unwrap();

        let rename2 = LogEntry {
            seq: 3,
            op: OpType::Rename,
            path: Some("c.rs".to_string()),
            blob_id: None,
            from_path: Some("b.rs".to_string()),
            resolved_conflict_ours_id: None,
            resolved_conflict_theirs_id: None,
            snapshot_id: None,
            ts: 300,
            message: None,
        };
        engine.log.append(&rename2).await.unwrap();

        let snap = engine
            .compute("ws1", vec![], 0, "test", "rename chain")
            .await
            .unwrap();
        let tree = engine
            .object_store
            .get_tree(&crate::object::TreeId(snap.tree_hash))
            .await
            .unwrap();
        assert_eq!(tree.0.len(), 1);
        assert_eq!(tree.0[0].name, "c.rs");
        assert_eq!(tree.0[0].id, "h1");
    }

    #[tokio::test]
    async fn test_compute_write_overwrite_delete_readd() {
        let (_tmp, engine) = make_engine().await;
        engine
            .log
            .append(&write_entry(1, "f.rs", "h1", 100))
            .await
            .unwrap();
        engine
            .log
            .append(&write_entry(2, "f.rs", "h2", 200))
            .await
            .unwrap();
        engine
            .log
            .append(&delete_entry(3, "f.rs", 300))
            .await
            .unwrap();
        engine
            .log
            .append(&write_entry(4, "f.rs", "h3", 400))
            .await
            .unwrap();

        let snap = engine
            .compute("ws1", vec![], 0, "test", "complex lifecycle")
            .await
            .unwrap();
        let tree = engine
            .object_store
            .get_tree(&crate::object::TreeId(snap.tree_hash))
            .await
            .unwrap();
        assert_eq!(tree.0.len(), 1);
        assert_eq!(tree.0[0].name, "f.rs");
        assert_eq!(tree.0[0].id, "h3");
    }

    #[tokio::test]
    async fn test_compute_with_since_seq() {
        let (_tmp, engine) = make_engine().await;
        engine
            .log
            .append(&write_entry(1, "a.rs", "h1", 100))
            .await
            .unwrap();
        let parent = engine
            .compute("ws1", vec![], 0, "test", "parent")
            .await
            .unwrap();

        engine
            .log
            .append(&write_entry(2, "b.rs", "h2", 200))
            .await
            .unwrap();
        let child = engine
            .compute("ws1", vec![parent.id], 1, "test", "child since seq=1")
            .await
            .unwrap();

        let tree = engine
            .object_store
            .get_tree(&crate::object::TreeId(child.tree_hash))
            .await
            .unwrap();
        let names: Vec<&str> = tree.0.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"a.rs"), "parent entry should be inherited");
        assert!(
            names.contains(&"b.rs"),
            "new entry from since_seq should appear"
        );
    }

    #[tokio::test]
    async fn test_compute_rename_to_ignored_path_removes_source() {
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
            .append(&write_entry(1, "good.rs", "h1", 100))
            .await
            .unwrap();

        let rename_to_noa = LogEntry {
            seq: 2,
            op: OpType::Rename,
            path: Some(".noa/stolen".to_string()),
            blob_id: None,
            from_path: Some("good.rs".to_string()),
            resolved_conflict_ours_id: None,
            resolved_conflict_theirs_id: None,
            snapshot_id: None,
            ts: 200,
            message: None,
        };
        engine.log.append(&rename_to_noa).await.unwrap();

        let snap = engine
            .compute("default", vec![], 0, "test", "rename to ignored")
            .await
            .unwrap();
        let tree = engine
            .object_store
            .get_tree(&crate::object::TreeId(snap.tree_hash))
            .await
            .unwrap();
        let names: Vec<&str> = tree.0.iter().map(|e| e.name.as_str()).collect();
        assert!(
            !names.contains(&"good.rs"),
            "source should be removed by rename"
        );
        assert!(
            !names.iter().any(|n| n.starts_with(".noa")),
            "target should be ignored"
        );
        assert!(tree.0.is_empty());
    }

    #[tokio::test]
    async fn test_deep_nested_directories() {
        let (_tmp, engine) = make_engine().await;
        engine
            .log
            .append(&write_entry(1, "src/a/b/c/d.rs", "h1", 100))
            .await
            .unwrap();
        engine
            .log
            .append(&write_entry(2, "src/a/b/c/e.rs", "h2", 200))
            .await
            .unwrap();
        engine
            .log
            .append(&write_entry(3, "src/a/f.rs", "h3", 300))
            .await
            .unwrap();

        let snap = engine
            .compute("default", vec![], 0, "test", "deep nesting")
            .await
            .unwrap();
        let tree = engine
            .object_store
            .get_tree(&crate::object::TreeId(snap.tree_hash))
            .await
            .unwrap();

        assert_eq!(tree.0.len(), 1);
        assert_eq!(tree.0[0].name, "src");
        assert_eq!(tree.0[0].kind, crate::object::EntryKind::Tree);

        let src_tree = engine
            .object_store
            .get_tree(&TreeId(tree.0[0].id.clone()))
            .await
            .unwrap();
        assert_eq!(src_tree.0.len(), 1);
        assert_eq!(src_tree.0[0].name, "a");
        assert_eq!(src_tree.0[0].kind, crate::object::EntryKind::Tree);

        let a_tree = engine
            .object_store
            .get_tree(&TreeId(src_tree.0[0].id.clone()))
            .await
            .unwrap();
        assert_eq!(a_tree.0.len(), 2);

        let b_entry = a_tree.0.iter().find(|e| e.name == "b").unwrap();
        assert_eq!(b_entry.kind, crate::object::EntryKind::Tree);

        let f_entry = a_tree.0.iter().find(|e| e.name == "f.rs").unwrap();
        assert_eq!(f_entry.kind, crate::object::EntryKind::Blob);
        assert_eq!(f_entry.id, "h3");

        let b_tree = engine
            .object_store
            .get_tree(&TreeId(b_entry.id.clone()))
            .await
            .unwrap();
        assert_eq!(b_tree.0.len(), 1);
        assert_eq!(b_tree.0[0].name, "c");
        assert_eq!(b_tree.0[0].kind, crate::object::EntryKind::Tree);

        let c_tree = engine
            .object_store
            .get_tree(&TreeId(b_tree.0[0].id.clone()))
            .await
            .unwrap();
        assert_eq!(c_tree.0.len(), 2);

        let d_entry = c_tree.0.iter().find(|e| e.name == "d.rs").unwrap();
        assert_eq!(d_entry.id, "h1");
        let e_entry = c_tree.0.iter().find(|e| e.name == "e.rs").unwrap();
        assert_eq!(e_entry.id, "h2");
    }

    #[tokio::test]
    async fn test_sibling_dirs_with_same_named_children() {
        // Regression test: sibling directories with same-named children must
        // each produce independent sub-trees instead of colliding in the
        // hierarchical tree builder.
        let (_tmp, engine) = make_engine().await;
        engine
            .log
            .append(&write_entry(1, "src/a/main.rs", "hash_a", 100))
            .await
            .unwrap();
        engine
            .log
            .append(&write_entry(2, "src/b/main.rs", "hash_b", 200))
            .await
            .unwrap();

        let snap = engine
            .compute("default", vec![], 0, "test", "sibling same-name")
            .await
            .unwrap();
        let tree = engine
            .object_store
            .get_tree(&crate::object::TreeId(snap.tree_hash))
            .await
            .unwrap();
        assert_eq!(tree.0.len(), 1);
        assert_eq!(tree.0[0].name, "src");

        let src_tree = engine
            .object_store
            .get_tree(&TreeId(tree.0[0].id.clone()))
            .await
            .unwrap();
        assert_eq!(src_tree.0.len(), 2);

        let a_entry = src_tree.0.iter().find(|e| e.name == "a").unwrap();
        let b_entry = src_tree.0.iter().find(|e| e.name == "b").unwrap();
        assert_eq!(a_entry.kind, crate::object::EntryKind::Tree);
        assert_eq!(b_entry.kind, crate::object::EntryKind::Tree);

        let a_tree = engine
            .object_store
            .get_tree(&TreeId(a_entry.id.clone()))
            .await
            .unwrap();
        let b_tree = engine
            .object_store
            .get_tree(&TreeId(b_entry.id.clone()))
            .await
            .unwrap();
        assert_eq!(a_tree.0.len(), 1);
        assert_eq!(a_tree.0[0].name, "main.rs");
        assert_eq!(a_tree.0[0].id, "hash_a");
        assert_eq!(b_tree.0.len(), 1);
        assert_eq!(b_tree.0[0].name, "main.rs");
        assert_eq!(b_tree.0[0].id, "hash_b");
    }
}
