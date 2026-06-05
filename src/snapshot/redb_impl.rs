use std::sync::Arc;

use redb::{Database, ReadableTable};

use super::{Snapshot, SnapshotId, SnapshotStore};
use crate::error::{NoaError, Result};

const SNAPSHOTS: redb::TableDefinition<&str, &[u8]> = redb::TableDefinition::new("snapshots");

macro_rules! redb_err {
    ($result:expr) => {
        $result.map_err(|e| NoaError::Redb(e.to_string()))
    };
}

#[derive(Clone)]
pub struct RedbSnapshotStore {
    db: Arc<Database>,
}

impl RedbSnapshotStore {
    pub fn new(db: Arc<Database>) -> Result<Self> {
        let store = RedbSnapshotStore { db };
        store.ensure_table()?;
        Ok(store)
    }

    fn ensure_table(&self) -> Result<()> {
        let txn = redb_err!(self.db.begin_write())?;
        {
            let _ = redb_err!(txn.open_table(SNAPSHOTS));
        }
        redb_err!(txn.commit())
    }
}

#[async_trait::async_trait]
impl SnapshotStore for RedbSnapshotStore {
    async fn get(&self, id: &SnapshotId) -> Result<Snapshot> {
        let txn = redb_err!(self.db.begin_read())?;
        let table = redb_err!(txn.open_table(SNAPSHOTS))?;

        match redb_err!(table.get(id.as_str()))? {
            Some(guard) => rmp_serde::from_slice(guard.value())
                .map_err(|e| NoaError::Serialization(e.to_string())),
            None => Err(NoaError::SnapshotNotFound(id.to_string())),
        }
    }

    async fn store(&self, snapshot: &Snapshot) -> Result<()> {
        let data = rmp_serde::to_vec(snapshot)
            .map_err(|e| NoaError::Serialization(e.to_string()))?;

        let txn = redb_err!(self.db.begin_write())?;
        {
            let mut table = redb_err!(txn.open_table(SNAPSHOTS))?;
            redb_err!(table.insert(snapshot.id.as_str(), data.as_slice()))?;
        }
        redb_err!(txn.commit())
    }

    async fn children_of(&self, parent: &SnapshotId) -> Result<Vec<SnapshotId>> {
        let all = self.list_all().await?;
        Ok(all
            .into_iter()
            .filter(|s| s.parents.contains(parent))
            .map(|s| s.id)
            .collect())
    }

    async fn list_all(&self) -> Result<Vec<Snapshot>> {
        let txn = redb_err!(self.db.begin_read())?;
        let table = redb_err!(txn.open_table(SNAPSHOTS))?;

        let mut result = Vec::new();
        for entry in redb_err!(table.iter())? {
            let (_, value) = redb_err!(entry)?;
            let snapshot: Snapshot = rmp_serde::from_slice(value.value())
                .map_err(|e| NoaError::Serialization(e.to_string()))?;
            result.push(snapshot);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::generate_snapshot_id;
    use tempfile::TempDir;

    fn make_store() -> (TempDir, RedbSnapshotStore) {
        let tmp = TempDir::new().unwrap();
        let db = Database::builder()
            .create(tmp.path().join("test.redb"))
            .unwrap();
        let store = RedbSnapshotStore::new(Arc::new(db)).unwrap();
        (tmp, store)
    }

    fn make_snapshot(id_suffix: &str, parents: Vec<&SnapshotId>) -> Snapshot {
        Snapshot {
            id: SnapshotId(format!("noa_{}", id_suffix)),
            tree_hash: format!("hash_{}", id_suffix),
            parents: parents.iter().map(|p| (*p).clone()).collect(),
            workspace: "default".to_string(),
            author: "test".to_string(),
            timestamp: 1000,
            message: format!("snapshot {}", id_suffix),
        }
    }

    #[tokio::test]
    async fn test_store_and_get() {
        let (_tmp, store) = make_store();
        let snap = make_snapshot("aaa111", vec![]);
        store.store(&snap).await.unwrap();
        let retrieved = store.get(&snap.id).await.unwrap();
        assert_eq!(retrieved, snap);
    }

    #[tokio::test]
    async fn test_not_found() {
        let (_tmp, store) = make_store();
        let result = store.get(&SnapshotId("noa_missing".to_string())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_children_of() {
        let (_tmp, store) = make_store();
        let parent = make_snapshot("parent1", vec![]);
        store.store(&parent).await.unwrap();

        let child1 = make_snapshot("child1", vec![&parent.id]);
        let child2 = make_snapshot("child2", vec![&parent.id]);
        store.store(&child1).await.unwrap();
        store.store(&child2).await.unwrap();

        let children = store.children_of(&parent.id).await.unwrap();
        assert_eq!(children.len(), 2);
    }

    #[tokio::test]
    async fn test_list_all() {
        let (_tmp, store) = make_store();
        store.store(&make_snapshot("s1", vec![])).await.unwrap();
        store.store(&make_snapshot("s2", vec![])).await.unwrap();
        let all = store.list_all().await.unwrap();
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_generate_snapshot_id_format() {
        let id = generate_snapshot_id();
        assert!(id.0.starts_with("noa_"));
        assert_eq!(id.0.len(), 16);
    }
}
