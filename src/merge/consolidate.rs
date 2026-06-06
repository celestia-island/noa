use crate::{
    error::Result,
    log::AgentLog,
    object::ObjectStore,
    snapshot::{SnapshotEngine, SnapshotId, SnapshotStore},
};

pub struct Consolidator<'a, L: AgentLog, S: SnapshotStore, O: ObjectStore> {
    engine: &'a SnapshotEngine<L, S, O>,
}

impl<'a, L: AgentLog, S: SnapshotStore, O: ObjectStore> Consolidator<'a, L, S, O> {
    pub fn new(engine: &'a SnapshotEngine<L, S, O>) -> Self {
        Consolidator { engine }
    }

    pub async fn consolidate(
        &self,
        workspace: &str,
        parent_ids: Vec<SnapshotId>,
        author: &str,
        message: &str,
    ) -> Result<crate::snapshot::Snapshot> {
        self.engine
            .compute(workspace, parent_ids, author, message)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::{FileAgentLog, LogEntry, OpType};
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
        (tmp, SnapshotEngine::new(log, snapshot_store, object_store))
    }

    #[tokio::test]
    async fn test_consolidate_builds_snapshot() {
        let (_tmp, engine) = make_engine().await;
        let consolidator = Consolidator::new(&engine);

        let e1 = LogEntry {
            seq: 2,
            op: OpType::Write,
            path: Some("b.rs".to_string()),
            blob_id: Some("h2".to_string()),
            from_path: None,
            resolved_conflict_ours_id: None,
            resolved_conflict_theirs_id: None,
            snapshot_id: None,
            ts: 200,
            message: None,
        };
        let e2 = LogEntry {
            seq: 1,
            op: OpType::Write,
            path: Some("a.rs".to_string()),
            blob_id: Some("h1".to_string()),
            from_path: None,
            resolved_conflict_ours_id: None,
            resolved_conflict_theirs_id: None,
            snapshot_id: None,
            ts: 100,
            message: None,
        };
        engine.log.append(&e1).await.unwrap();
        engine.log.append(&e2).await.unwrap();

        let snap = consolidator
            .consolidate("ws1", vec![], "agent", "consolidated")
            .await
            .unwrap();

        let tree = engine
            .object_store
            .get_tree(&crate::object::TreeId(snap.tree_hash))
            .await
            .unwrap();
        assert_eq!(tree.0.len(), 2);
    }
}
