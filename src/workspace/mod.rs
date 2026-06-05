use redb::ReadableTable;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use crate::error::{NoaError, Result};
use crate::snapshot::SnapshotId;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Workspace {
    pub name: String,
    pub head: SnapshotId,
    pub base: SnapshotId,
    pub agent_id: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

const WORKSPACES: redb::TableDefinition<&str, &[u8]> = redb::TableDefinition::new("workspaces");

macro_rules! redb_err {
    ($result:expr) => {
        $result.map_err(|e| NoaError::Redb(e.to_string()))
    };
}

#[derive(Clone)]
pub struct WorkspaceManager {
    db: Arc<redb::Database>,
}

impl WorkspaceManager {
    pub fn new(db: Arc<redb::Database>) -> Result<Self> {
        let mgr = WorkspaceManager { db };
        mgr.ensure_table()?;
        Ok(mgr)
    }

    fn ensure_table(&self) -> Result<()> {
        let txn = redb_err!(self.db.begin_write())?;
        {
            let _ = redb_err!(txn.open_table(WORKSPACES));
        }
        redb_err!(txn.commit())
    }

    pub async fn create(&self, workspace: &Workspace) -> Result<()> {
        if self.get(&workspace.name).await?.is_some() {
            return Err(NoaError::WorkspaceAlreadyExists(workspace.name.clone()));
        }
        self.put(workspace).await
    }

    pub async fn get(&self, name: &str) -> Result<Option<Workspace>> {
        let txn = redb_err!(self.db.begin_read())?;
        let table = redb_err!(txn.open_table(WORKSPACES))?;
        match redb_err!(table.get(name))? {
            Some(guard) => {
                let ws: Workspace = rmp_serde::from_slice(guard.value())
                    .map_err(|e| NoaError::Serialization(e.to_string()))?;
                Ok(Some(ws))
            }
            None => Ok(None),
        }
    }

    pub async fn put(&self, workspace: &Workspace) -> Result<()> {
        let data = rmp_serde::to_vec(workspace)
            .map_err(|e| NoaError::Serialization(e.to_string()))?;
        let txn = redb_err!(self.db.begin_write())?;
        {
            let mut table = redb_err!(txn.open_table(WORKSPACES))?;
            redb_err!(table.insert(workspace.name.as_str(), data.as_slice()))?;
        }
        redb_err!(txn.commit())
    }

    pub async fn delete(&self, name: &str) -> Result<bool> {
        let txn = redb_err!(self.db.begin_write())?;
        {
            let mut table = redb_err!(txn.open_table(WORKSPACES))?;
            let _existed = redb_err!(table.get(name))?.is_some();
            redb_err!(table.remove(name))?;
        }
        redb_err!(txn.commit())?;
        Ok(true)
    }

    pub async fn list(&self) -> Result<Vec<Workspace>> {
        let txn = redb_err!(self.db.begin_read())?;
        let table = redb_err!(txn.open_table(WORKSPACES))?;
        let mut result = Vec::new();
        for entry in redb_err!(table.iter())? {
            let (_, value) = redb_err!(entry)?;
            let ws: Workspace = rmp_serde::from_slice(value.value())
                .map_err(|e| NoaError::Serialization(e.to_string()))?;
            result.push(ws);
        }
        Ok(result)
    }

    pub async fn update_head(&self, name: &str, new_head: &SnapshotId) -> Result<()> {
        let mut ws = self
            .get(name)
            .await?
            .ok_or_else(|| NoaError::WorkspaceNotFound(name.to_string()))?;
        ws.head = new_head.clone();
        ws.updated_at = chrono::Utc::now().timestamp_micros() as u64;
        self.put(&ws).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_manager() -> (TempDir, WorkspaceManager) {
        let tmp = TempDir::new().unwrap();
        let db = Arc::new(
            redb::Database::builder()
                .create(tmp.path().join("test.redb"))
                .unwrap(),
        );
        let mgr = WorkspaceManager::new(db).unwrap();
        (tmp, mgr)
    }

    fn make_workspace(name: &str) -> Workspace {
        Workspace {
            name: name.to_string(),
            head: SnapshotId("noa_base".to_string()),
            base: SnapshotId("noa_base".to_string()),
            agent_id: None,
            created_at: 1000,
            updated_at: 1000,
        }
    }

    #[tokio::test]
    async fn test_create_and_get() {
        let (_tmp, mgr) = make_manager();
        let ws = make_workspace("default");
        mgr.create(&ws).await.unwrap();
        let got = mgr.get("default").await.unwrap().unwrap();
        assert_eq!(got, ws);
    }

    #[tokio::test]
    async fn test_create_duplicate_fails() {
        let (_tmp, mgr) = make_manager();
        mgr.create(&make_workspace("ws1")).await.unwrap();
        let result = mgr.create(&make_workspace("ws1")).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list() {
        let (_tmp, mgr) = make_manager();
        mgr.create(&make_workspace("a")).await.unwrap();
        mgr.create(&make_workspace("b")).await.unwrap();
        let list = mgr.list().await.unwrap();
        assert_eq!(list.len(), 2);
    }

    #[tokio::test]
    async fn test_delete() {
        let (_tmp, mgr) = make_manager();
        mgr.create(&make_workspace("ws1")).await.unwrap();
        mgr.delete("ws1").await.unwrap();
        assert!(mgr.get("ws1").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_update_head() {
        let (_tmp, mgr) = make_manager();
        mgr.create(&make_workspace("ws1")).await.unwrap();
        let new_head = SnapshotId("noa_new".to_string());
        mgr.update_head("ws1", &new_head).await.unwrap();
        let ws = mgr.get("ws1").await.unwrap().unwrap();
        assert_eq!(ws.head, new_head);
    }

    #[tokio::test]
    async fn test_get_nonexistent() {
        let (_tmp, mgr) = make_manager();
        assert!(mgr.get("missing").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_update_head_nonexistent() {
        let (_tmp, mgr) = make_manager();
        let result = mgr.update_head("missing", &SnapshotId("noa_x".to_string())).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_put_overwrites() {
        let (_tmp, mgr) = make_manager();
        let ws = make_workspace("ws1");
        mgr.create(&ws).await.unwrap();
        let mut updated = ws.clone();
        updated.head = SnapshotId("noa_new_head".to_string());
        mgr.put(&updated).await.unwrap();
        let got = mgr.get("ws1").await.unwrap().unwrap();
        assert_eq!(got.head, SnapshotId("noa_new_head".to_string()));
    }

    #[tokio::test]
    async fn test_workspace_with_agent_id() {
        let (_tmp, mgr) = make_manager();
        let ws = Workspace {
            name: "agent-ws".to_string(),
            head: SnapshotId("noa_base".to_string()),
            base: SnapshotId("noa_base".to_string()),
            agent_id: Some("agent-007".to_string()),
            created_at: 1000,
            updated_at: 1000,
        };
        mgr.create(&ws).await.unwrap();
        let got = mgr.get("agent-ws").await.unwrap().unwrap();
        assert_eq!(got.agent_id, Some("agent-007".to_string()));
    }

    #[tokio::test]
    async fn test_update_head_updates_timestamp() {
        let (_tmp, mgr) = make_manager();
        let ws = make_workspace("ws1");
        mgr.create(&ws).await.unwrap();
        let before = mgr.get("ws1").await.unwrap().unwrap().updated_at;
        std::thread::sleep(std::time::Duration::from_micros(100));
        mgr.update_head("ws1", &SnapshotId("noa_new".to_string())).await.unwrap();
        let after = mgr.get("ws1").await.unwrap().unwrap().updated_at;
        assert!(after >= before);
    }

    #[tokio::test]
    async fn test_list_empty_after_delete_all() {
        let (_tmp, mgr) = make_manager();
        mgr.create(&make_workspace("a")).await.unwrap();
        mgr.create(&make_workspace("b")).await.unwrap();
        mgr.delete("a").await.unwrap();
        mgr.delete("b").await.unwrap();
        let list = mgr.list().await.unwrap();
        assert!(list.is_empty());
    }

    #[tokio::test]
    async fn test_workspace_serialization_roundtrip() {
        let (_tmp, mgr) = make_manager();
        let ws = Workspace {
            name: "test".to_string(),
            head: SnapshotId("noa_head".to_string()),
            base: SnapshotId("noa_base".to_string()),
            agent_id: Some("agent-001".to_string()),
            created_at: 12345,
            updated_at: 67890,
        };
        mgr.create(&ws).await.unwrap();
        let got = mgr.get("test").await.unwrap().unwrap();
        assert_eq!(got, ws);
    }
}
