//! Self-hosted pull-request store (P6#B2) — redb-backed PR records served by
//! noa-server so PRs can be created and merged on the user's own platform.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

use redb::{ReadableDatabase, ReadableTable, TableDefinition};

use crate::error::Result;
use crate::forge::PrMetadata;

const PRS: TableDefinition<u64, &[u8]> = TableDefinition::new("prs");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrRecord {
    pub number: u64,
    /// Repository namespace (default `"default"`).
    pub repo: String,
    pub title: String,
    pub body: String,
    /// `open` | `closed` | `merged`.
    pub state: String,
    /// Base workspace name (merge target).
    pub base: String,
    /// Head workspace name (merge source).
    pub head: String,
    /// Snapshot id of the base head at creation time (three-way merge base).
    pub base_snapshot: String,
    pub author: String,
    /// Unix seconds.
    pub created_at: i64,
    /// Snapshot id produced by the server-side merge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merge_snapshot: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<PrMetadata>,
}

#[derive(Clone)]
pub struct PrStore {
    db: Arc<redb::Database>,
}

impl PrStore {
    pub fn new(db: Arc<redb::Database>) -> Result<Self> {
        let store = PrStore { db };
        store.ensure_table()?;
        Ok(store)
    }

    fn ensure_table(&self) -> Result<()> {
        let txn = self.db.begin_write()?;
        txn.open_table(PRS)?;
        txn.commit()?;
        Ok(())
    }

    /// Creates a PR record with the next sequential number, atomically.
    pub async fn create(&self, mut record: PrRecord) -> Result<PrRecord> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_write()?;
            {
                let mut table = txn.open_table(PRS)?;
                let next = table.last()?.map(|(k, _)| k.value() + 1).unwrap_or(1);
                record.number = next;
                // to_vec_named: structs with skip_serializing_if must use the
                // map representation, otherwise element-skipping breaks the
                // positional (array) layout on read-back.
                let data = rmp_serde::to_vec_named(&record)?;
                table.insert(next, data.as_slice())?;
            }
            txn.commit()?;
            Ok(record)
        })
        .await?
    }

    pub async fn get(&self, number: u64) -> Result<Option<PrRecord>> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_read()?;
            let table = txn.open_table(PRS)?;
            match table.get(number)? {
                Some(guard) => {
                    let record: PrRecord = rmp_serde::from_slice(guard.value())?;
                    Ok(Some(record))
                }
                None => Ok(None),
            }
        })
        .await?
    }

    pub async fn put(&self, record: &PrRecord) -> Result<()> {
        let db = self.db.clone();
        let number = record.number;
        let data = rmp_serde::to_vec_named(record)?;
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_write()?;
            {
                let mut table = txn.open_table(PRS)?;
                table.insert(number, data.as_slice())?;
            }
            txn.commit()?;
            Ok(())
        })
        .await?
    }

    pub async fn list(
        &self,
        repo: Option<&str>,
        base: Option<&str>,
        state: Option<&str>,
    ) -> Result<Vec<PrRecord>> {
        let db = self.db.clone();
        let repo = repo.map(str::to_string);
        let base = base.map(str::to_string);
        let state = state.map(str::to_string);
        tokio::task::spawn_blocking(move || {
            let txn = db.begin_read()?;
            let table = txn.open_table(PRS)?;
            let mut records = Vec::new();
            for entry in table.iter()? {
                let (_, value) = entry?;
                let record: PrRecord = rmp_serde::from_slice(value.value())?;
                if let Some(r) = &repo {
                    if record.repo != *r {
                        continue;
                    }
                }
                if let Some(b) = &base {
                    if record.base != *b {
                        continue;
                    }
                }
                if let Some(s) = &state {
                    if record.state != *s {
                        continue;
                    }
                }
                records.push(record);
            }
            records.sort_by_key(|r| std::cmp::Reverse(r.number));
            Ok(records)
        })
        .await?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forge::PrMetadata;

    fn record(number: u64) -> PrRecord {
        PrRecord {
            number,
            repo: "default".to_string(),
            title: "t".to_string(),
            body: String::new(),
            state: "open".to_string(),
            base: "master".to_string(),
            head: "feat/x".to_string(),
            base_snapshot: "noa_base".to_string(),
            author: "lab".to_string(),
            created_at: 100,
            merge_snapshot: None,
            metadata: None,
        }
    }

    async fn store() -> (tempfile::TempDir, PrStore) {
        let tmp = tempfile::TempDir::new().unwrap();
        let db = Arc::new(
            redb::Database::builder()
                .create(tmp.path().join("prs.redb"))
                .unwrap(),
        );
        let store = PrStore::new(db).unwrap();
        (tmp, store)
    }

    #[tokio::test]
    async fn test_create_assigns_sequential_numbers() {
        let (_tmp, store) = store().await;
        let a = store.create(record(0)).await.unwrap();
        let b = store.create(record(0)).await.unwrap();
        let c = store.create(record(0)).await.unwrap();
        assert_eq!(a.number, 1);
        assert_eq!(b.number, 2);
        assert_eq!(c.number, 3);
    }

    #[tokio::test]
    async fn test_get_and_put() {
        let (_tmp, store) = store().await;
        let created = store.create(record(0)).await.unwrap();
        assert!(store.get(created.number).await.unwrap().is_some());
        assert!(store.get(999).await.unwrap().is_none());

        let mut merged = created.clone();
        merged.state = "merged".to_string();
        merged.merge_snapshot = Some("noa_merged".to_string());
        store.put(&merged).await.unwrap();
        let loaded = store.get(created.number).await.unwrap().unwrap();
        assert_eq!(loaded.state, "merged");
        assert_eq!(loaded.merge_snapshot.as_deref(), Some("noa_merged"));
    }

    #[tokio::test]
    async fn test_list_filters() {
        let (_tmp, store) = store().await;
        let mut r = record(0);
        r.state = "open".to_string();
        r.base = "master".to_string();
        store.create(r).await.unwrap();
        let mut r = record(0);
        r.state = "merged".to_string();
        r.base = "release".to_string();
        store.create(r).await.unwrap();

        assert_eq!(store.list(None, None, None).await.unwrap().len(), 2);
        assert_eq!(store.list(None, None, Some("open")).await.unwrap().len(), 1);
        assert_eq!(
            store.list(None, Some("release"), None).await.unwrap().len(),
            1
        );
        assert_eq!(
            store.list(Some("other"), None, None).await.unwrap().len(),
            0
        );
    }

    #[tokio::test]
    async fn test_metadata_roundtrip() {
        let (_tmp, store) = store().await;
        let mut r = record(0);
        r.metadata = Some(PrMetadata {
            model: Some("deepseek/deepseek-chat".to_string()),
            input_tokens: Some(10),
            output_tokens: Some(20),
            cost_usd: Some(0.001),
        });
        let created = store.create(r).await.unwrap();
        let loaded = store.get(created.number).await.unwrap().unwrap();
        assert_eq!(
            loaded.metadata.unwrap().model.as_deref(),
            Some("deepseek/deepseek-chat")
        );
    }
}
