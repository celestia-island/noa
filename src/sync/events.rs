use std::path::{Component, Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{
    error::{NoaError, Result},
    log::{AgentLog, LogEntry},
    object::ObjectStore,
    repo::Repository,
};

fn sanitize_path(base: &Path, user_path: &str) -> Option<PathBuf> {
    let joined = base.join(user_path);
    match joined.canonicalize() {
        Ok(canonical) => {
            let base_canonical = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
            if canonical.starts_with(&base_canonical) {
                Some(canonical)
            } else {
                None
            }
        }
        Err(_) => {
            let mut safe = base.to_path_buf();
            for component in Path::new(user_path).components() {
                match component {
                    Component::Normal(c) => {
                        safe.push(c);
                    }
                    Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                        return None;
                    }
                    Component::CurDir => {}
                }
            }
            Some(safe)
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncEvent {
    pub seq: u64,
    pub op: String,
    pub path: Option<String>,
    pub blob_id: Option<String>,
    pub from_path: Option<String>,
    pub ts: u64,
    pub message: Option<String>,
}

impl From<LogEntry> for SyncEvent {
    fn from(entry: LogEntry) -> Self {
        SyncEvent {
            seq: entry.seq,
            op: format!("{:?}", entry.op).to_lowercase(),
            path: entry.path,
            blob_id: entry.blob_id,
            from_path: entry.from_path,
            ts: entry.ts,
            message: entry.message,
        }
    }
}

impl From<&LogEntry> for SyncEvent {
    fn from(entry: &LogEntry) -> Self {
        SyncEvent {
            seq: entry.seq,
            op: format!("{:?}", entry.op).to_lowercase(),
            path: entry.path.clone(),
            blob_id: entry.blob_id.clone(),
            from_path: entry.from_path.clone(),
            ts: entry.ts,
            message: entry.message.clone(),
        }
    }
}

pub struct EventSyncEngine {
    workspace_root: PathBuf,
    workspace_name: String,
}

impl EventSyncEngine {
    pub fn new(workspace_root: &Path, workspace_name: &str) -> Self {
        EventSyncEngine {
            workspace_root: workspace_root.to_path_buf(),
            workspace_name: workspace_name.to_string(),
        }
    }

    pub async fn collect_push_events(&self, since_seq: u64) -> Result<Vec<SyncEvent>> {
        let repo = Repository::open(&self.workspace_root)?;
        let log = repo.agent_log(&self.workspace_name)?;
        let entries = log.read_since(since_seq).await?;
        Ok(entries.iter().map(SyncEvent::from).collect())
    }

    pub async fn apply_pull_events(&self, events: &[SyncEvent]) -> Result<u64> {
        let repo = Repository::open(&self.workspace_root)?;
        let log = repo.agent_log(&self.workspace_name)?;
        let mut applied: u64 = 0;

        for event in events {
            if let Some(path) = &event.path {
                let file_path = match sanitize_path(&self.workspace_root, path) {
                    Some(p) => p,
                    None => {
                        tracing::warn!("rejecting path traversal attempt: {}", path);
                        continue;
                    }
                };
                if let Some(parent) = file_path.parent() {
                    std::fs::create_dir_all(parent)?;
                }

                match event.op.as_str() {
                    "write" => {
                        if let Some(blob_id) = &event.blob_id {
                            let obj_store = repo.object_store()?;
                            let blob_id = crate::object::BlobId(blob_id.clone());
                            match obj_store.get_blob(&blob_id).await {
                                Ok(data) => {
                                    std::fs::write(&file_path, &data)?;
                                    applied += 1;
                                }
                                Err(NoaError::ObjectNotFound(_)) => {
                                    tracing::warn!("blob {} not found, skipping write", blob_id.0);
                                }
                                Err(e) => return Err(e),
                            }
                        }
                    }
                    "delete" => {
                        if file_path.exists() {
                            std::fs::remove_file(&file_path)?;
                        }
                        applied += 1;
                    }
                    "rename" => {
                        if let Some(from) = &event.from_path {
                            let from_path = match sanitize_path(&self.workspace_root, from) {
                                Some(p) => p,
                                None => {
                                    tracing::warn!(
                                        "rejecting path traversal in rename source: {}",
                                        from
                                    );
                                    continue;
                                }
                            };
                            if from_path.exists() {
                                std::fs::rename(&from_path, &file_path)?;
                                applied += 1;
                            }
                        }
                    }
                    _ => {
                        applied += 1;
                    }
                }

                let log_entry = LogEntry {
                    seq: event.seq,
                    op: match event.op.as_str() {
                        "write" => crate::log::OpType::Write,
                        "delete" => crate::log::OpType::Delete,
                        "rename" => crate::log::OpType::Rename,
                        "snapshot" => crate::log::OpType::Snapshot,
                        "merge" => crate::log::OpType::Merge,
                        "resolve" => crate::log::OpType::Resolve,
                        _ => crate::log::OpType::Write,
                    },
                    path: event.path.clone(),
                    blob_id: event.blob_id.clone(),
                    from_path: event.from_path.clone(),
                    resolved_conflict_ours_id: None,
                    resolved_conflict_theirs_id: None,
                    snapshot_id: None,
                    ts: event.ts,
                    message: event.message.clone(),
                };
                if let Err(e) = log.append(&log_entry).await {
                    tracing::warn!("failed to append event to agent log (seq {}): {}", event.seq, e);
                }
            }
        }

        Ok(applied)
    }

    pub async fn queue_local_events(&self, since_seq: u64) -> Result<Vec<SyncEvent>> {
        self.collect_push_events(since_seq).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::OpType;
    use tempfile::TempDir;

    fn setup_repo() -> TempDir {
        let tmp = TempDir::new().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();
        drop(repo);
        tmp
    }

    #[test]
    fn test_sanitize_path_rejects_traversal() {
        let tmp = TempDir::new().unwrap();
        assert!(sanitize_path(tmp.path(), "../../../etc/passwd").is_none());
        assert!(sanitize_path(tmp.path(), "/etc/passwd").is_none());
        assert!(sanitize_path(tmp.path(), "sub/../../../etc/passwd").is_none());
    }

    #[test]
    fn test_sanitize_path_allows_normal() {
        let tmp = TempDir::new().unwrap();
        assert!(sanitize_path(tmp.path(), "src/main.rs").is_some());
        assert!(sanitize_path(tmp.path(), "a/b/c.txt").is_some());
    }

    #[tokio::test]
    async fn test_collect_empty() {
        let tmp = setup_repo();
        let engine = EventSyncEngine::new(tmp.path(), "default");
        let events = engine.collect_push_events(0).await.unwrap();
        assert!(events.is_empty());
    }

    #[tokio::test]
    async fn test_collect_after_append() {
        let tmp = setup_repo();
        {
            let repo = Repository::open(tmp.path()).unwrap();
            let log = repo.agent_log("default").unwrap();
            log.append(&LogEntry {
                seq: 1,
                op: OpType::Write,
                path: Some("test.rs".to_string()),
                blob_id: Some("h1".to_string()),
                from_path: None,
                resolved_conflict_ours_id: None,
                resolved_conflict_theirs_id: None,
                snapshot_id: None,
                ts: 100,
                message: None,
            })
            .await
            .unwrap();
        }

        let engine = EventSyncEngine::new(tmp.path(), "default");
        let events = engine.collect_push_events(0).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].path, Some("test.rs".to_string()));
    }

    #[tokio::test]
    async fn test_apply_delete_event() {
        let tmp = setup_repo();
        std::fs::write(tmp.path().join("old.rs"), "content").unwrap();

        let engine = EventSyncEngine::new(tmp.path(), "default");
        let events = vec![SyncEvent {
            seq: 1,
            op: "delete".to_string(),
            path: Some("old.rs".to_string()),
            blob_id: None,
            from_path: None,
            ts: 100,
            message: None,
        }];
        let applied = engine.apply_pull_events(&events).await.unwrap();
        assert_eq!(applied, 1);
        assert!(!tmp.path().join("old.rs").exists());
    }

    #[tokio::test]
    async fn test_apply_rename_event() {
        let tmp = setup_repo();
        std::fs::write(tmp.path().join("a.rs"), "content").unwrap();

        let engine = EventSyncEngine::new(tmp.path(), "default");
        let events = vec![SyncEvent {
            seq: 1,
            op: "rename".to_string(),
            path: Some("b.rs".to_string()),
            blob_id: None,
            from_path: Some("a.rs".to_string()),
            ts: 100,
            message: None,
        }];
        let applied = engine.apply_pull_events(&events).await.unwrap();
        assert_eq!(applied, 1);
        assert!(!tmp.path().join("a.rs").exists());
        assert!(tmp.path().join("b.rs").exists());
    }
}
