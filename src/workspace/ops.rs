use crate::{error::Result, snapshot::SnapshotId, workspace::{Workspace, WorkspaceManager}};

pub async fn create(
    mgr: &WorkspaceManager,
    name: &str,
    base_snapshot: &SnapshotId,
    agent_id: Option<String>,
) -> Result<Workspace> {
    let now = chrono::Utc::now().timestamp_micros() as u64;
    let ws = Workspace {
        name: name.to_string(),
        head: base_snapshot.clone(),
        base: base_snapshot.clone(),
        agent_id,
        last_seq: 0,
        created_at: now,
        updated_at: now,
    };
    mgr.create(&ws).await?;
    Ok(ws)
}

pub async fn switch(
    mgr: &WorkspaceManager,
    name: &str,
) -> Result<()> {
    mgr.get(name)
        .await?
        .ok_or_else(|| crate::error::NoaError::WorkspaceNotFound(name.to_string()))?;
    Ok(())
}

pub async fn list(mgr: &WorkspaceManager) -> Result<Vec<Workspace>> {
    mgr.list().await
}

pub async fn delete(mgr: &WorkspaceManager, name: &str) -> Result<bool> {
    mgr.delete(name).await
}

pub async fn update_head(
    mgr: &WorkspaceManager,
    name: &str,
    new_head: &SnapshotId,
) -> Result<()> {
    mgr.update_head(name, new_head).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::WorkspaceManager;
    use std::sync::Arc;
    use tempfile::TempDir;

    fn make_mgr() -> (TempDir, WorkspaceManager) {
        let tmp = TempDir::new().unwrap();
        let db = Arc::new(
            redb::Database::builder()
                .create(tmp.path().join("test.redb"))
                .unwrap(),
        );
        (tmp, WorkspaceManager::new(db).unwrap())
    }

    #[tokio::test]
    async fn test_ops_create() {
        let (_tmp, mgr) = make_mgr();
        let ws = create(&mgr, "ws1", &SnapshotId("noa_base".to_string()), None)
            .await
            .unwrap();
        assert_eq!(ws.name, "ws1");
    }

    #[tokio::test]
    async fn test_ops_switch() {
        let (_tmp, mgr) = make_mgr();
        create(&mgr, "ws1", &SnapshotId("noa_base".to_string()), None)
            .await
            .unwrap();
        assert!(switch(&mgr, "ws1").await.is_ok());
        assert!(switch(&mgr, "missing").await.is_err());
    }

    #[tokio::test]
    async fn test_ops_list() {
        let (_tmp, mgr) = make_mgr();
        create(&mgr, "a", &SnapshotId("noa_base".to_string()), None)
            .await
            .unwrap();
        let list = list(&mgr).await.unwrap();
        assert_eq!(list.len(), 1);
    }
}
