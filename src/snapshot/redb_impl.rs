use std::sync::Arc;

use redb::{Database, ReadableTable};

use super::{Snapshot, SnapshotId, SnapshotStore};
use crate::{,
};

const SNAPSHOTS: redb::TableDefinition<&str, &[u8]> = redb::TableDefinition::new("snapshots");
const PARENT_INDEX: redb::TableDefinition<&str, &str> =
    redb::TableDefinition::new("snapshot_parent_index");

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
        let txn =!(self.db.begin_write())?;
        {
            let _ =!(txn.open_table(SNAPSHOTS));
            let _ =!(txn.open_table(PARENT_INDEX));
        }!(txn.commit())
    }

    fn index_key(parent_id: &str, child_id: &str) -> String {
        format!("{parent_id}:{child_id}")
    }
}

#[async_trait::async_trait]
impl SnapshotStore for RedbSnapshotStore {
    async fn get(&self, id: &SnapshotId) -> Result<Snapshot> {
        let db = Arc::clone(&self.db);
        let id = id.clone();
        tokio::task::spawn_blocking(move || {
            let txn =!(db.begin_read())?;
            let table =!(txn.open_table(SNAPSHOTS))?;

            match!(table.get(id.as_str()))? {
                Some(guard) => rmp_serde::from_slice(guard.value())
                    ?,
                None => anyhow::bail!("snapshot not found: {}", id.to_string()),
            }
        })
        .await
        ?
    }

    async fn store(&self, snapshot: &Snapshot) -> Result<()> {
        let data =
            rmp_serde::to_vec(snapshot)?;

        let db = Arc::clone(&self.db);
        let id = snapshot.id.clone();
        let parents = snapshot.parents.clone();
        tokio::task::spawn_blocking(move || {
            let txn =!(db.begin_write())?;
            {
                let mut table =!(txn.open_table(SNAPSHOTS))?;!(table.insert(id.as_str(), data.as_slice()))?;

                let mut parent_idx =!(txn.open_table(PARENT_INDEX))?;
                for parent in &parents {
                    let key = Self::index_key(parent.as_str(), id.as_str());!(parent_idx.insert(key.as_str(), id.as_str()))?;
                }
            }!(txn.commit())
        })
        .await
        ?
    }

    async fn children_of(&self, parent: &SnapshotId) -> Result<Vec<SnapshotId>> {
        let db = Arc::clone(&self.db);
        let parent = parent.clone();
        tokio::task::spawn_blocking(move || {
            let txn =!(db.begin_read())?;
            let table =!(txn.open_table(PARENT_INDEX))?;

            let prefix = format!("{}:", parent.as_str());
            let mut children = Vec::new();
            for entry in!(table.range(prefix.as_str()..))? {
                let (key, value) = entry?;
                let key_str = key.value();
                if !key_str.starts_with(&prefix) {
                    break;
                }
                children.push(SnapshotId(value.value().to_string()));
            }
            Ok(children)
        })
        .await
        ?
    }

    async fn list_all(&self) -> Result<Vec<Snapshot>> {
        let db = Arc::clone(&self.db);
        tokio::task::spawn_blocking(move || {
            let txn =!(db.begin_read())?;
            let table =!(txn.open_table(SNAPSHOTS))?;

            let mut result = Vec::new();
            for entry in!(table.iter())? {
                let (_, value) = entry?;
                let snapshot: Snapshot = rmp_serde::from_slice(value.value())
                    ?;
                result.push(snapshot);
            }
            Ok(result)
        })
        .await
        ?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::snapshot::content_addressed_snapshot_id;
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
    async fn test_children_of_uses_index() {
        let (_tmp, store) = make_store();
        let parent = make_snapshot("parent1", vec![]);
        store.store(&parent).await.unwrap();

        let unrelated = make_snapshot("unrelated", vec![]);
        store.store(&unrelated).await.unwrap();

        let child = make_snapshot("child1", vec![&parent.id]);
        store.store(&child).await.unwrap();

        let children = store.children_of(&parent.id).await.unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0], child.id);
    }

    #[tokio::test]
    async fn test_children_of_empty() {
        let (_tmp, store) = make_store();
        let parent = make_snapshot("parent1", vec![]);
        store.store(&parent).await.unwrap();

        let children = store.children_of(&parent.id).await.unwrap();
        assert!(children.is_empty());
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
    async fn test_content_addressed_snapshot_id_format() {
        let id = content_addressed_snapshot_id("treehash", &[], "workspace");
        assert!(id.0.starts_with("noa_"));
        assert_eq!(id.0.len(), 36);
    }

    #[tokio::test]
    async fn test_children_of_multi_parent() {
        let (_tmp, store) = make_store();
        let p1 = make_snapshot("parent1", vec![]);
        let p2 = make_snapshot("parent2", vec![]);
        store.store(&p1).await.unwrap();
        store.store(&p2).await.unwrap();

        let merge_child = make_snapshot("merge", vec![&p1.id, &p2.id]);
        store.store(&merge_child).await.unwrap();

        let children_p1 = store.children_of(&p1.id).await.unwrap();
        let children_p2 = store.children_of(&p2.id).await.unwrap();
        assert_eq!(children_p1.len(), 1);
        assert_eq!(children_p2.len(), 1);
        assert_eq!(children_p1[0], merge_child.id);
        assert_eq!(children_p2[0], merge_child.id);
    }
}
