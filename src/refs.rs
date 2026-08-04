use async_trait::async_trait;
use std::sync::Arc;

use redb::{ReadableDatabase, ReadableTable};

use crate::{error::Result, snapshot::SnapshotId};

#[async_trait]
pub trait RefStore: Send + Sync {
    async fn get(&self, name: &str) -> Result<Option<SnapshotId>>;
    async fn cas(&self, name: &str, old: Option<&SnapshotId>, new: &SnapshotId) -> Result<bool>;
    async fn list(&self) -> Result<Vec<(String, SnapshotId)>>;
    async fn delete(&self, name: &str) -> Result<bool>;
}

pub struct RedbRefStore {
    db: Arc<redb::Database>,
}

const REFS: redb::TableDefinition<&str, &[u8]> = redb::TableDefinition::new("refs");

impl RedbRefStore {
    pub fn new(db: Arc<redb::Database>) -> Result<Self> {
        let store = RedbRefStore { db };
        store.ensure_table()?;
        Ok(store)
    }

    fn ensure_table(&self) -> Result<()> {
        let txn = self.db.begin_write()?;
        txn.open_table(REFS)?;
        txn.commit()?;
        Ok(())
    }

    #[cfg(test)]
    fn clone_inner(&self) -> Self {
        RedbRefStore {
            db: self.db.clone(),
        }
    }
}

#[async_trait]
impl RefStore for RedbRefStore {
    async fn get(&self, name: &str) -> Result<Option<SnapshotId>> {
        let db = self.db.clone();
        let name = name.to_string();
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_read()?;
            let table = txn.open_table(REFS)?;
            match table.get(name.as_str())? {
                Some(guard) => {
                    let id_str = String::from_utf8(guard.value().to_vec())?;
                    Ok(Some(SnapshotId(id_str)))
                }
                None => Ok(None),
            }
        })
        .await?
    }

    async fn cas(&self, name: &str, old: Option<&SnapshotId>, new: &SnapshotId) -> Result<bool> {
        let db = self.db.clone();
        let name = name.to_string();
        let old = old.cloned();
        let new = new.clone();
        let result = tokio::task::spawn_blocking(move || {
            let txn = db.begin_write()?;
            {
                let mut table = txn.open_table(REFS)?;

                let current: Option<SnapshotId> = match table.get(&*name)? {
                    Some(guard) => {
                        let s = String::from_utf8(guard.value().to_vec())?;
                        Some(SnapshotId(s))
                    }
                    None => None,
                };

                let matches = match (&old, &current) {
                    (None, None) => true,
                    (Some(expected), Some(cur)) => expected == cur,
                    _ => false,
                };

                if !matches {
                    return Ok(false);
                }
                table.insert(&*name, new.0.as_bytes())?;
            }
            match txn.commit() {
                Ok(()) => Ok(true),
                Err(e) => Err(e.into()),
            }
        })
        .await?;
        result
    }

    async fn list(&self) -> Result<Vec<(String, SnapshotId)>> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_read()?;
            let table = txn.open_table(REFS)?;
            let mut result = Vec::new();
            for entry in table.iter()? {
                let (key, value) = entry?;
                let id_str = String::from_utf8(value.value().to_vec())?;
                result.push((key.value().to_string(), SnapshotId(id_str)));
            }
            Ok(result)
        })
        .await?
    }

    async fn delete(&self, name: &str) -> Result<bool> {
        let db = self.db.clone();
        let name = name.to_string();
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_write()?;
            let existed = {
                let mut table = txn.open_table(REFS)?;
                let removed = table.remove(name.as_str())?;
                removed.is_some()
            };
            txn.commit()?;
            Ok(existed)
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_store() -> (TempDir, RedbRefStore) {
        let tmp = TempDir::new().unwrap();
        let db = Arc::new(
            redb::Database::builder()
                .create(tmp.path().join("test.redb"))
                .unwrap(),
        );
        let store = RedbRefStore::new(db).unwrap();
        (tmp, store)
    }

    #[tokio::test]
    async fn test_cas_create() {
        let (_tmp, store) = make_store();
        let id = SnapshotId("noa_abc".to_string());
        let ok = store.cas("main", None, &id).await.unwrap();
        assert!(ok);
        let got = store.get("main").await.unwrap();
        assert_eq!(got, Some(id));
    }

    #[tokio::test]
    async fn test_cas_conflict() {
        let (_tmp, store) = make_store();
        let id1 = SnapshotId("noa_abc".to_string());
        let id2 = SnapshotId("noa_def".to_string());
        store.cas("main", None, &id1).await.unwrap();
        let ok = store.cas("main", None, &id2).await.unwrap();
        assert!(!ok);
    }

    #[tokio::test]
    async fn test_cas_update() {
        let (_tmp, store) = make_store();
        let id1 = SnapshotId("noa_abc".to_string());
        let id2 = SnapshotId("noa_def".to_string());
        store.cas("main", None, &id1).await.unwrap();
        let ok = store.cas("main", Some(&id1), &id2).await.unwrap();
        assert!(ok);
        let got = store.get("main").await.unwrap();
        assert_eq!(got, Some(id2));
    }

    #[tokio::test]
    async fn test_list() {
        let (_tmp, store) = make_store();
        store
            .cas("main", None, &SnapshotId("noa_a".to_string()))
            .await
            .unwrap();
        store
            .cas("dev", None, &SnapshotId("noa_b".to_string()))
            .await
            .unwrap();
        let refs = store.list().await.unwrap();
        assert_eq!(refs.len(), 2);
    }

    #[tokio::test]
    async fn test_delete() {
        let (_tmp, store) = make_store();
        store
            .cas("main", None, &SnapshotId("noa_a".to_string()))
            .await
            .unwrap();
        let _ = store.delete("main").await.unwrap();
        let got = store.get("main").await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn test_get_nonexistent() {
        let (_tmp, store) = make_store();
        let got = store.get("missing").await.unwrap();
        assert!(got.is_none());
    }

    #[tokio::test]
    async fn test_multiple_refs() {
        let (_tmp, store) = make_store();
        store
            .cas("main", None, &SnapshotId("noa_a".to_string()))
            .await
            .unwrap();
        store
            .cas("dev", None, &SnapshotId("noa_b".to_string()))
            .await
            .unwrap();
        store
            .cas("feature", None, &SnapshotId("noa_c".to_string()))
            .await
            .unwrap();

        let refs = store.list().await.unwrap();
        assert_eq!(refs.len(), 3);

        assert_eq!(
            store.get("main").await.unwrap(),
            Some(SnapshotId("noa_a".to_string()))
        );
        assert_eq!(
            store.get("dev").await.unwrap(),
            Some(SnapshotId("noa_b".to_string()))
        );
        assert_eq!(
            store.get("feature").await.unwrap(),
            Some(SnapshotId("noa_c".to_string()))
        );
    }

    #[tokio::test]
    async fn test_cas_update_wrong_old_fails() {
        let (_tmp, store) = make_store();
        let id1 = SnapshotId("noa_abc".to_string());
        let id2 = SnapshotId("noa_def".to_string());
        let id3 = SnapshotId("noa_ghi".to_string());
        store.cas("main", None, &id1).await.unwrap();
        let ok = store.cas("main", Some(&id2), &id3).await.unwrap();
        assert!(!ok);
        assert_eq!(store.get("main").await.unwrap(), Some(id1));
    }

    #[tokio::test]
    async fn test_delete_nonexistent_returns_false() {
        let (_tmp, store) = make_store();
        let existed = store.delete("nonexistent").await.unwrap();
        assert!(!existed);
    }

    #[tokio::test]
    async fn test_empty_list() {
        let (_tmp, store) = make_store();
        let refs = store.list().await.unwrap();
        assert!(refs.is_empty());
    }

    #[tokio::test]
    async fn test_overwrite_via_cas() {
        let (_tmp, store) = make_store();
        let v1 = SnapshotId("noa_v1".to_string());
        let v2 = SnapshotId("noa_v2".to_string());
        let v3 = SnapshotId("noa_v3".to_string());

        store.cas("main", None, &v1).await.unwrap();
        store.cas("main", Some(&v1), &v2).await.unwrap();
        store.cas("main", Some(&v2), &v3).await.unwrap();

        assert_eq!(store.get("main").await.unwrap(), Some(v3));
    }

    #[tokio::test]
    async fn test_cas_concurrent_only_one_wins() {
        let (_tmp, store) = make_store();
        let v1 = SnapshotId("noa_v1".to_string());
        store.cas("main", None, &v1).await.unwrap();

        let v_a = SnapshotId("noa_a".to_string());
        let v_b = SnapshotId("noa_b".to_string());

        let store_c1 = store.clone_inner();
        let store_c2 = store.clone_inner();

        let (r1, r2) = tokio::join!(
            async { store_c1.cas("main", Some(&v1), &v_a).await },
            async { store_c2.cas("main", Some(&v1), &v_b).await }
        );
        let ok1 = r1.unwrap();
        let ok2 = r2.unwrap();
        assert!(
            ok1 != ok2,
            "exactly one CAS should succeed, got ok1={} ok2={}",
            ok1,
            ok2
        );
        let final_val = store.get("main").await.unwrap().unwrap();
        assert!(final_val == v_a || final_val == v_b);
    }

    #[tokio::test]
    async fn test_cas_create_concurrent_only_one_wins() {
        let (_tmp, store) = make_store();

        let v_a = SnapshotId("noa_a".to_string());
        let v_b = SnapshotId("noa_b".to_string());

        let store_c1 = store.clone_inner();
        let store_c2 = store.clone_inner();

        let (r1, r2) = tokio::join!(async { store_c1.cas("new-ref", None, &v_a).await }, async {
            store_c2.cas("new-ref", None, &v_b).await
        });
        let ok1 = r1.unwrap();
        let ok2 = r2.unwrap();
        assert!(ok1 || ok2, "at least one must succeed");
        assert!(!(ok1 && ok2), "both should not succeed");
    }

    #[tokio::test]
    async fn test_list_after_delete() {
        let (_tmp, store) = make_store();
        store
            .cas("a", None, &SnapshotId("noa_1".to_string()))
            .await
            .unwrap();
        store
            .cas("b", None, &SnapshotId("noa_2".to_string()))
            .await
            .unwrap();
        store.delete("a").await.unwrap();
        let refs = store.list().await.unwrap();
        assert_eq!(refs.len(), 1);
        assert_eq!(refs[0].0, "b");
    }
}
