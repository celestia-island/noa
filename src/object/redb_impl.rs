use std::sync::Arc;

use redb::Database;

use super::{sha256_hex, BlobId, ObjectStore, TreeEntries, TreeId};
use crate::error::{NoaError, Result};

const BLOBS: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new("blobs");
const TREES: redb::TableDefinition<&[u8], &[u8]> = redb::TableDefinition::new("trees");

#[derive(Clone)]
pub struct RedbObjectStore {
    db: Arc<Database>,
}

impl RedbObjectStore {
    pub fn new(db: Arc<Database>) -> Result<Self> {
        let store = RedbObjectStore { db };
        store.ensure_tables()?;
        Ok(store)
    }

    fn ensure_tables(&self) -> Result<()> {
        let txn = self.db.begin_write()?;
        txn.open_table(BLOBS)?;
        txn.open_table(TREES)?;
        txn.commit()?;
        Ok(())
    }
}

#[async_trait::async_trait]
impl ObjectStore for RedbObjectStore {
    async fn put_blob(&self, content: &[u8]) -> Result<BlobId> {
        let db = self.db.clone();
        let content = content.to_vec();
        tokio::task::spawn_blocking(move || {
            let hash = sha256_hex(&content);
            let id = BlobId(hash);
            let txn = db.begin_write()?;
            {
                let mut table = txn.open_table(BLOBS)?;
                table.insert(id.as_bytes(), content.as_slice())?;
            }
            txn.commit()?;
            Ok(id)
        })
        .await?
    }

    async fn get_blob(&self, id: &BlobId) -> Result<Vec<u8>> {
        let db = self.db.clone();
        let id = id.clone();
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_read()?;
            let table = txn.open_table(BLOBS)?;
            match table.get(id.as_bytes())? {
                Some(guard) => Ok(guard.value().to_vec()),
                None => Err(NoaError::ObjectNotFound { id: id.0 }.into()),
            }
        })
        .await?
    }

    async fn has_blob(&self, id: &BlobId) -> Result<bool> {
        let db = self.db.clone();
        let id = id.clone();
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_read()?;
            let table = txn.open_table(BLOBS)?;
            Ok(table.get(id.as_bytes())?.is_some())
        })
        .await?
    }

    async fn put_tree(&self, entries: &TreeEntries) -> Result<TreeId> {
        let db = self.db.clone();
        let data = rmp_serde::to_vec(entries)?;
        tokio::task::spawn_blocking(move || {
            let hash = sha256_hex(&data);
            let id = TreeId(hash);
            let txn = db.begin_write()?;
            {
                let mut table = txn.open_table(TREES)?;
                table.insert(id.as_bytes(), data.as_slice())?;
            }
            txn.commit()?;
            Ok(id)
        })
        .await?
    }

    async fn get_tree(&self, id: &TreeId) -> Result<TreeEntries> {
        let db = self.db.clone();
        let id = id.clone();
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_read()?;
            let table = txn.open_table(TREES)?;
            match table.get(id.as_bytes())? {
                Some(guard) => Ok(rmp_serde::from_slice::<TreeEntries>(guard.value())?),
                None => Err(NoaError::ObjectNotFound { id: id.0 }.into()),
            }
        })
        .await?
    }

    async fn has_tree(&self, id: &TreeId) -> Result<bool> {
        let db = self.db.clone();
        let id = id.clone();
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_read()?;
            let table = txn.open_table(TREES)?;
            Ok(table.get(id.as_bytes())?.is_some())
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_store() -> (TempDir, RedbObjectStore) {
        let tmp = TempDir::new().unwrap();
        let db = Database::builder()
            .create(tmp.path().join("test.redb"))
            .unwrap();
        let store = RedbObjectStore::new(Arc::new(db)).unwrap();
        (tmp, store)
    }

    #[tokio::test]
    async fn test_blob_roundtrip() {
        let (_tmp, store) = make_store();
        let content = b"hello, noa!";
        let id = store.put_blob(content).await.unwrap();
        let retrieved = store.get_blob(&id).await.unwrap();
        assert_eq!(retrieved, content);
    }

    #[tokio::test]
    async fn test_blob_dedup() {
        let (_tmp, store) = make_store();
        let content = b"duplicate content";
        let id1 = store.put_blob(content).await.unwrap();
        let id2 = store.put_blob(content).await.unwrap();
        assert_eq!(id1, id2);
    }

    #[tokio::test]
    async fn test_blob_has() {
        let (_tmp, store) = make_store();
        let id = store.put_blob(b"data").await.unwrap();
        assert!(store.has_blob(&id).await.unwrap());
        assert!(!store
            .has_blob(&BlobId("nonexistent".to_string()))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_blob_not_found() {
        let (_tmp, store) = make_store();
        let result = store.get_blob(&BlobId("missing".to_string())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_tree_roundtrip() {
        let (_tmp, store) = make_store();
        let entries = TreeEntries(vec![
            super::super::TreeEntry {
                name: "main.rs".to_string(),
                kind: super::super::EntryKind::Blob,
                id: "abc123".to_string(),
            },
            super::super::TreeEntry {
                name: "lib".to_string(),
                kind: super::super::EntryKind::Tree,
                id: "def456".to_string(),
            },
        ]);
        let id = store.put_tree(&entries).await.unwrap();
        let retrieved = store.get_tree(&id).await.unwrap();
        assert_eq!(retrieved, entries);
    }

    #[tokio::test]
    async fn test_tree_dedup() {
        let (_tmp, store) = make_store();
        let entries = TreeEntries(vec![super::super::TreeEntry {
            name: "foo.rs".to_string(),
            kind: super::super::EntryKind::Blob,
            id: "hash1".to_string(),
        }]);
        let id1 = store.put_tree(&entries).await.unwrap();
        let id2 = store.put_tree(&entries).await.unwrap();
        assert_eq!(id1, id2);
    }

    #[tokio::test]
    async fn test_tree_has() {
        let (_tmp, store) = make_store();
        let entries = TreeEntries::new();
        let id = store.put_tree(&entries).await.unwrap();
        assert!(store.has_tree(&id).await.unwrap());
        assert!(!store
            .has_tree(&TreeId("nonexistent".to_string()))
            .await
            .unwrap());
    }

    #[tokio::test]
    async fn test_tree_not_found() {
        let (_tmp, store) = make_store();
        let result = store.get_tree(&TreeId("missing".to_string())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_content_addressing_deterministic() {
        let (_tmp, store) = make_store();
        let content = b"deterministic test";
        let id = store.put_blob(content).await.unwrap();
        let expected_hash = sha256_hex(content);
        assert_eq!(id.0, expected_hash);
    }
}
