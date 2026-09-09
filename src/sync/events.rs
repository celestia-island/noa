use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};
use std::collections::HashSet;

use crate::{
    error::{is_object_not_found, Result},
    log::{AgentLog, LogEntry},
    object::ObjectStore,
    repo::Repository,
};

fn sanitize_path(base: &Path, user_path: &str) -> Option<PathBuf> {
    let joined = base.join(user_path);
    if let Ok(canonical) = joined.canonicalize() {
        let base_canonical = base.canonicalize().unwrap_or_else(|_| base.to_path_buf());
        if canonical.starts_with(&base_canonical) {
            Some(canonical)
        } else {
            None
        }
    } else {
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
        if safe.starts_with(base) {
            Some(safe)
        } else {
            None
        }
    }
}

fn verify_path_safe(base: &Path, resolved: &Path) -> bool {
    if let Ok(resolved_canonical) = resolved.canonicalize() {
        if let Ok(base_canonical) = base.canonicalize() {
            return resolved_canonical.starts_with(&base_canonical);
        }
    }
    resolved.starts_with(base)
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
    /// Sender identity. Together `(sender, seq)` is the idempotency key the
    /// receiver uses to skip already-applied events before mutating anything.
    /// Optional so batches written by older senders still deserialize.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender: Option<String>,
}

impl From<LogEntry> for SyncEvent {
    fn from(entry: LogEntry) -> Self {
        SyncEvent {
            seq: entry.seq,
            op: entry.op.as_op_str().to_string(),
            path: entry.path,
            blob_id: entry.blob_id,
            from_path: entry.from_path,
            ts: entry.ts,
            message: entry.message,
            sender: None,
        }
    }
}

impl From<&LogEntry> for SyncEvent {
    fn from(entry: &LogEntry) -> Self {
        SyncEvent {
            seq: entry.seq,
            op: entry.op.as_op_str().to_string(),
            path: entry.path.clone(),
            blob_id: entry.blob_id.clone(),
            from_path: entry.from_path.clone(),
            ts: entry.ts,
            message: entry.message.clone(),
            sender: None,
        }
    }
}

/// Failure from [`EventSyncEngine::apply_pull_events`] that preserves the
/// committed-prefix count.
///
/// Events applied before the failure are durable (fs + log) and are reported
/// via `applied`, so the sender resumes after the committed prefix instead of
/// replaying it. The ACK layer maps this to `applied=<prefix>, ok=false` plus
/// the error text.
#[derive(Debug)]
pub struct ApplyPullError {
    /// Number of events durably committed before the failure.
    pub applied: u64,
    /// The underlying failure.
    pub source: anyhow::Error,
}

impl std::fmt::Display for ApplyPullError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "apply_pull_events failed after {} applied: {}",
            self.applied, self.source
        )
    }
}

impl std::error::Error for ApplyPullError {}

pub struct EventSyncEngine {
    workspace_root: PathBuf,
    workspace_name: String,
}

impl EventSyncEngine {
    #[must_use]
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

    pub async fn apply_pull_events(
        &self,
        events: &[SyncEvent],
    ) -> std::result::Result<u64, ApplyPullError> {
        let repo = Repository::open(&self.workspace_root)
            .map_err(|source| ApplyPullError { applied: 0, source })?;
        let log = repo
            .agent_log(&self.workspace_name)
            .map_err(|source| ApplyPullError { applied: 0, source })?;
        // Single source of truth for "already applied": the remote identities
        // recorded in the log at commit time. Snapshot once up front and extend
        // in-memory as this batch commits, so duplicates inside one batch are
        // caught too. (History lost to compaction is no longer known; a retry
        // of compacted-away events re-applies, which the fs ops below tolerate
        // idempotently.)
        let existing = log
            .read_all()
            .await
            .map_err(|source| ApplyPullError { applied: 0, source })?;
        let mut seen: HashSet<(Option<String>, u64)> = existing
            .iter()
            .filter_map(|e| e.remote_seq.map(|seq| (e.remote_sender.clone(), seq)))
            .collect();
        let mut applied: u64 = 0;
        // Helper: every fallible step maps its error into ApplyPullError
        // carrying the durable committed-prefix count at that point.
        let failed = |applied: u64, source: anyhow::Error| ApplyPullError { applied, source };

        for event in events {
            // Idempotency gate: consult the recorded remote identity BEFORE
            // mutating the fs or the log. Already-seen events are a no-op and
            // are NOT recounted, so identical-batch retries change nothing.
            let key = (event.sender.clone(), event.seq);
            if seen.contains(&key) {
                tracing::debug!(
                    "skipping already-applied event (sender {:?}, remote seq {})",
                    event.sender,
                    event.seq
                );
                continue;
            }

            let op = match event.op.as_str() {
                "write" => crate::log::OpType::Write,
                "delete" => crate::log::OpType::Delete,
                "rename" => crate::log::OpType::Rename,
                "snapshot" => crate::log::OpType::Snapshot,
                "merge" => crate::log::OpType::Merge,
                "resolve" => crate::log::OpType::Resolve,
                _ => {
                    tracing::warn!("unknown event op '{}', skipping", event.op);
                    continue;
                }
            };

            // Whether this event counts toward `applied`. Only events that are
            // durable (fs effect attempted + log appended) by the end of the
            // iteration count; the count moves after the commit point below,
            // never before it.
            let mut counts = false;

            if let Some(path) = &event.path {
                let Some(file_path) = sanitize_path(&self.workspace_root, path) else {
                    tracing::warn!("rejecting path traversal attempt: {}", path);
                    continue;
                };

                let workspace_root = self.workspace_root.clone();
                let from_path_raw = event.from_path.clone();

                match event.op.as_str() {
                    "write" => {
                        if let Some(blob_id) = &event.blob_id {
                            let obj_store = repo
                                .object_store()
                                .map_err(|source| failed(applied, source))?;
                            let blob_id = crate::object::BlobId(blob_id.clone());
                            match obj_store.get_blob(&blob_id).await {
                                Ok(data) => {
                                    let fp = file_path.clone();
                                    let wr = workspace_root.clone();
                                    let write_res = tokio::task::spawn_blocking(
                                        move || -> anyhow::Result<()> {
                                            if !verify_path_safe(&wr, &fp) {
                                                anyhow::bail!(
                                                    "path safety check failed after async gap: {}",
                                                    fp.display()
                                                );
                                            }
                                            if let Some(parent) = fp.parent() {
                                                std::fs::create_dir_all(parent)?;
                                            }
                                            std::fs::write(&fp, &data)?;
                                            Ok(())
                                        },
                                    )
                                    .await;
                                    match write_res {
                                        Ok(Ok(())) => counts = true,
                                        Ok(Err(source)) => {
                                            return Err(failed(applied, source));
                                        }
                                        Err(join_err) => {
                                            return Err(failed(
                                                applied,
                                                anyhow::Error::new(join_err),
                                            ));
                                        }
                                    }
                                }
                                Err(e) if is_object_not_found(&e) => {
                                    tracing::warn!("blob {} not found, skipping write", blob_id.0);
                                }
                                Err(source) => return Err(failed(applied, source)),
                            }
                        }
                    }
                    "delete" => {
                        let fp = file_path.clone();
                        let wr = workspace_root.clone();
                        let delete_res = tokio::task::spawn_blocking(move || {
                            if !verify_path_safe(&wr, &fp) {
                                anyhow::bail!(
                                    "path safety check failed for delete: {}",
                                    fp.display()
                                );
                            }
                            if fp.exists() {
                                std::fs::remove_file(&fp)?;
                            }
                            Ok::<(), anyhow::Error>(())
                        })
                        .await;
                        match delete_res {
                            Ok(Ok(())) => counts = true,
                            Ok(Err(source)) => return Err(failed(applied, source)),
                            Err(join_err) => {
                                return Err(failed(applied, anyhow::Error::new(join_err)));
                            }
                        }
                    }
                    "rename" => {
                        if let Some(from) = &from_path_raw {
                            let from_sanitized = sanitize_path(&workspace_root, from);
                            if let Some(from_path) = from_sanitized {
                                let fp = file_path.clone();
                                let wr = workspace_root.clone();
                                let rename_res = tokio::task::spawn_blocking(move || {
                                    if !verify_path_safe(&wr, &fp) {
                                        anyhow::bail!(
                                            "path safety check failed for rename dest: {}",
                                            fp.display()
                                        );
                                    }
                                    if !verify_path_safe(&wr, &from_path) {
                                        anyhow::bail!(
                                            "path safety check failed for rename src: {}",
                                            from_path.display()
                                        );
                                    }
                                    if from_path.exists() {
                                        if let Err(e) = std::fs::rename(&from_path, &fp) {
                                            if e.kind() == std::io::ErrorKind::CrossesDevices {
                                                std::fs::copy(&from_path, &fp)?;
                                                std::fs::remove_file(&from_path)?;
                                            } else {
                                                anyhow::bail!(e);
                                            }
                                        }
                                    }
                                    Ok::<(), anyhow::Error>(())
                                })
                                .await;
                                match rename_res {
                                    Ok(Ok(())) => counts = true,
                                    Ok(Err(source)) => return Err(failed(applied, source)),
                                    Err(join_err) => {
                                        return Err(failed(applied, anyhow::Error::new(join_err)));
                                    }
                                }
                            } else {
                                tracing::warn!(
                                    "rejecting path traversal in rename source: {}",
                                    from
                                );
                            }
                        }
                    }
                    _ => {
                        counts = true;
                    }
                }
            } else {
                counts = true;
            }

            // Commit point: record the remote identity in the log (the single
            // source of truth for replay detection), then count. A failed
            // append leaves `applied` excluding this event, so the returned
            // error reports exactly the committed prefix and the sender can
            // resume after it.
            let log_entry = LogEntry {
                seq: 0,
                op,
                path: event.path.clone(),
                blob_id: event.blob_id.clone(),
                from_path: event.from_path.clone(),
                resolved_conflict_ours_id: None,
                resolved_conflict_theirs_id: None,
                snapshot_id: None,
                remote_seq: Some(event.seq),
                remote_sender: event.sender.clone(),
                ts: event.ts,
                message: event.message.clone(),
            };
            log.append(&log_entry).await.map_err(|e| {
                tracing::error!(
                    "failed to append event to agent log (seq {}): {}",
                    event.seq,
                    e
                );
                failed(applied, e)
            })?;
            seen.insert(key);
            if counts {
                applied += 1;
            }
        }

        Ok(applied)
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
                remote_seq: None,
                remote_sender: None,
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
            sender: None,
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
            sender: None,
        }];
        let applied = engine.apply_pull_events(&events).await.unwrap();
        assert_eq!(applied, 1);
        assert!(!tmp.path().join("a.rs").exists());
        assert!(tmp.path().join("b.rs").exists());
    }

    /// Issue #73, phase A: a batch whose first event commits and whose second
    /// event fails must report the committed prefix (`applied=1`, `ok=false`
    /// with error detail) — never `applied=0` disowning committed work.
    #[tokio::test]
    async fn test_partial_batch_reports_committed_prefix() {
        let tmp = setup_repo();
        std::fs::write(tmp.path().join("victim.txt"), "v1").unwrap();
        std::fs::create_dir(tmp.path().join("blockdir")).unwrap();
        let blob_id = {
            let repo = Repository::open(tmp.path()).unwrap();
            repo.object_store()
                .unwrap()
                .put_blob(b"hello-sync")
                .await
                .unwrap()
        };

        let engine = EventSyncEngine::new(tmp.path(), "default");
        let batch = vec![
            SyncEvent {
                seq: 1,
                op: "delete".to_string(),
                path: Some("victim.txt".to_string()),
                blob_id: None,
                from_path: None,
                ts: 100,
                message: None,
                sender: None,
            },
            // Writing file bytes onto an existing directory fails on every
            // platform (Windows: OS error 5; Unix: EISDIR).
            SyncEvent {
                seq: 2,
                op: "write".to_string(),
                path: Some("blockdir".to_string()),
                blob_id: Some(blob_id.0.clone()),
                from_path: None,
                ts: 101,
                message: None,
                sender: None,
            },
        ];
        let err = engine.apply_pull_events(&batch).await.unwrap_err();
        assert_eq!(err.applied, 1);

        // The prefix is durable: fs effect + one log entry.
        assert!(!tmp.path().join("victim.txt").exists());
        assert!(tmp.path().join("blockdir").is_dir());
        {
            let repo = Repository::open(tmp.path()).unwrap();
            let entries = repo
                .agent_log("default")
                .unwrap()
                .read_all()
                .await
                .unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].path, Some("victim.txt".to_string()));
            assert_eq!(entries[0].remote_seq, Some(1));
        }

        // The ACK layer reports the prefix accurately instead of disowning it.
        let ack =
            crate::sync::NoaEventSyncAck::from_apply_result("ws".to_string(), Err(err));
        assert_eq!(ack.applied, 1);
        assert!(!ack.ok);
        assert!(ack.error.is_some());
    }

    /// Issue #73, phase B: re-applying an identical batch must be a no-op —
    /// no duplicate log entries and nothing recounted.
    #[tokio::test]
    async fn test_identical_retry_is_noop() {
        let tmp = setup_repo();
        std::fs::write(tmp.path().join("r1.txt"), "A").unwrap();
        std::fs::write(tmp.path().join("r2.txt"), "B").unwrap();

        let engine = EventSyncEngine::new(tmp.path(), "default");
        let batch = vec![
            SyncEvent {
                seq: 1,
                op: "delete".to_string(),
                path: Some("r1.txt".to_string()),
                blob_id: None,
                from_path: None,
                ts: 100,
                message: None,
                sender: Some("remote-1".to_string()),
            },
            SyncEvent {
                seq: 2,
                op: "delete".to_string(),
                path: Some("r2.txt".to_string()),
                blob_id: None,
                from_path: None,
                ts: 101,
                message: None,
                sender: Some("remote-1".to_string()),
            },
        ];
        assert_eq!(engine.apply_pull_events(&batch).await.unwrap(), 2);
        // Identical retry: no-op, log length unchanged.
        assert_eq!(engine.apply_pull_events(&batch).await.unwrap(), 0);

        {
            let repo = Repository::open(tmp.path()).unwrap();
            let entries = repo
                .agent_log("default")
                .unwrap()
                .read_all()
                .await
                .unwrap();
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].remote_seq, Some(1));
            assert_eq!(entries[1].remote_seq, Some(2));
            assert_eq!(
                entries[0].remote_sender,
                Some("remote-1".to_string())
            );
        }
        assert!(!tmp.path().join("r1.txt").exists());
        assert!(!tmp.path().join("r2.txt").exists());
    }

    /// The idempotency key is `(sender, seq)`: the same remote seqs from a
    /// different sender are distinct events and still apply.
    #[tokio::test]
    async fn test_same_seq_different_sender_applies() {
        let tmp = setup_repo();
        std::fs::write(tmp.path().join("s1.txt"), "A").unwrap();

        let engine = EventSyncEngine::new(tmp.path(), "default");
        let batch_a = vec![SyncEvent {
            seq: 1,
            op: "delete".to_string(),
            path: Some("s1.txt".to_string()),
            blob_id: None,
            from_path: None,
            ts: 100,
            message: None,
            sender: Some("a".to_string()),
        }];
        let batch_b = vec![SyncEvent {
            seq: 1,
            op: "delete".to_string(),
            path: Some("s1.txt".to_string()),
            blob_id: None,
            from_path: None,
            ts: 100,
            message: None,
            sender: Some("b".to_string()),
        }];
        assert_eq!(engine.apply_pull_events(&batch_a).await.unwrap(), 1);
        assert_eq!(engine.apply_pull_events(&batch_b).await.unwrap(), 1);

        let repo = Repository::open(tmp.path()).unwrap();
        let entries = repo
            .agent_log("default")
            .unwrap()
            .read_all()
            .await
            .unwrap();
        assert_eq!(entries.len(), 2);
    }

    /// Wire compatibility: payloads written by older peers (no `sender` on
    /// events, no `error` on ACKs) still deserialize via serde defaults.
    #[test]
    fn test_wire_compat_old_payloads_parse() {
        let ack: crate::sync::NoaEventSyncAck =
            serde_json::from_str(r#"{"workspace_id":"w","applied":1,"ok":false}"#).unwrap();
        assert_eq!(ack.applied, 1);
        assert!(!ack.ok);
        assert_eq!(ack.error, None);

        let ev: SyncEvent = serde_json::from_str(
            r#"{"seq":1,"op":"delete","path":"x","blob_id":null,"from_path":null,"ts":1,"message":null}"#,
        )
        .unwrap();
        assert_eq!(ev.seq, 1);
        assert_eq!(ev.sender, None);
    }
}
