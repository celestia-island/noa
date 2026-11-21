use serde::{Deserialize, Serialize};
use std::path::{Component, Path, PathBuf};

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

/// Sidecar path holding the applied-identity watermark: `<log>.sync-watermark`.
fn sync_watermark_path(log_path: &Path) -> PathBuf {
    let mut s = log_path.as_os_str().to_owned();
    s.push(".sync-watermark");
    PathBuf::from(s)
}

/// Per-sender applied-identity watermark for pull idempotency (#105).
///
/// Maps each sender to the highest remote seq this receiver has applied. An
/// incoming event `(sender, seq)` is already-applied iff
/// `seq <= watermark[sender]`. This replaces the per-batch `read_all()`
/// rescan: identities are consumed incrementally at the commit point, so the
/// cost of a batch no longer grows with the total size of the agent log.
///
/// Contract: a sender pushes its log tail in seq order (the push/ACK
/// resume protocol), so a sender's remote seqs arrive monotonically. The
/// watermark never consults local log contents after seeding, which is what
/// makes it robust against the log being rewritten: compaction and external
/// truncation/rotation of the log file need no watermark reset (the keys are
/// sender identities, not local seqs). The documented reset channel is to
/// delete the sidecar: the next batch reseeds from whatever identities the
/// log still holds, and events whose evidence is gone re-apply once — the
/// safe direction, since the fs ops tolerate replay idempotently.
struct SyncWatermark {
    high_water: std::collections::BTreeMap<String, u64>,
}

impl SyncWatermark {
    /// Loads the persisted watermark. Returns `None` when the sidecar does
    /// not exist yet (first run after the watermark appeared), so the caller
    /// can seed it from a single full-log scan and keep the pre-watermark
    /// skip behavior for identities recorded before the upgrade. Corrupt
    /// lines are skipped with a warning (that sender's events re-apply once
    /// and self-heal) rather than failing the whole batch.
    fn load(path: &Path) -> std::io::Result<Option<SyncWatermark>> {
        let mut high_water: std::collections::BTreeMap<String, u64> =
            std::collections::BTreeMap::new();
        let mut content = String::new();
        {
            use std::io::Read;
            let mut file = match std::fs::File::open(path) {
                Ok(file) => file,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
                Err(e) => return Err(e),
            };
            file.read_to_string(&mut content)?;
        }
        for line in content.lines() {
            let Some((sender, seq)) = line.split_once('\t') else {
                if !line.trim().is_empty() {
                    tracing::warn!("skipping malformed sync watermark line: {line:?}");
                }
                continue;
            };
            match seq.trim().parse::<u64>() {
                Ok(seq) => {
                    high_water
                        .entry(sender.to_string())
                        .and_modify(|known| *known = (*known).max(seq))
                        .or_insert(seq);
                }
                Err(_) => tracing::warn!("skipping malformed sync watermark seq: {line:?}"),
            }
        }
        Ok(Some(SyncWatermark { high_water }))
    }

    /// Folds remote identities recorded in the log into the watermark.
    /// Only complete `(sender, seq)` pairs count, matching the gate below:
    /// an entry with a seq but no sender must not shadow the same seq from
    /// a different peer.
    fn seed_from_entries(&mut self, entries: &[LogEntry]) {
        for entry in entries {
            if let (Some(sender), Some(seq)) = (&entry.remote_sender, entry.remote_seq) {
                self.high_water
                    .entry(sender.clone())
                    .and_modify(|known| *known = (*known).max(seq))
                    .or_insert(seq);
            }
        }
    }

    /// Whether this identity is covered by the watermark (already applied).
    /// The gate only fires for an identified sender: without one the
    /// watermark has no row for the peer, and every peer's first event
    /// shares remote seq 1 — gating on that would silently drop the second
    /// peer's event. An unidentified event is therefore re-applied, which is
    /// what the pre-watermark code did.
    fn covers(&self, sender: &Option<String>, seq: u64) -> bool {
        sender
            .as_ref()
            .and_then(|s| self.high_water.get(s))
            .is_some_and(|&known| seq <= known)
    }

    /// Advances the watermark for an event whose effect just committed.
    /// Out-of-registry duplicates (a lower seq arriving after a higher one
    /// already committed) are no-ops.
    fn advance(&mut self, sender: &Option<String>, seq: u64) {
        if let Some(sender) = sender {
            self.high_water
                .entry(sender.clone())
                .and_modify(|known| *known = (*known).max(seq))
                .or_insert(seq);
        }
    }

    /// Crash-safe sidecar write: temp file + fsync + atomic rename, mirroring
    /// the `<log>.seq` sidecar protocol.
    fn persist(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write;
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let mut tmp_os = path.as_os_str().to_owned();
        tmp_os.push(".tmp");
        let tmp = PathBuf::from(tmp_os);
        {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&tmp)?;
            for (sender, seq) in &self.high_water {
                writeln!(f, "{sender}\t{seq}")?;
            }
            f.sync_all()?;
        }
        std::fs::rename(&tmp, path)?;
        Ok(())
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
    /// Sender identity. Together `(sender, seq)` is the idempotency key the
    /// receiver uses to skip already-applied events before mutating anything.
    /// Optional so batches written by older senders still deserialize.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender: Option<String>,
    /// verify.run/1 payload for `op == "verify"` events. Absent on pre-ledger
    /// senders (serde default) and on all non-verify ops. Carried as the
    /// already-serialized JSON value so the wire stays exactly what the
    /// writer produced — the receiver re-parses it into the strong type and
    /// rejects (warn+skip, counted neither applied nor errored) a verify
    /// event that arrives without a parseable payload.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify: Option<serde_json::Value>,
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
            verify: entry
                .verify
                .as_ref()
                .map(serde_json::to_value)
                .transpose()
                .ok()
                .flatten(),
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
            verify: entry
                .verify
                .as_ref()
                .map(serde_json::to_value)
                .transpose()
                .ok()
                .flatten(),
        }
    }
}

/// Failure from [`EventSyncEngine::apply_pull_events`] that preserves the
/// committed-prefix count.
///
/// Events that took effect before the failure are durable (fs + log) and are
/// reported via `applied`, so the sender can resume instead of replaying them.
/// An event whose effect was skipped — a blob that has not arrived yet, say — is
/// neither counted nor recorded as applied, so a resumed batch re-attempts it.
/// The ACK layer maps this to `applied=<count>, ok=false` plus the error text.
#[derive(Debug)]
pub struct ApplyPullError {
    /// Events whose effect took hold before the failure. Skipped effects are
    /// excluded on purpose, so resuming still re-attempts them.
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
        // Single source of truth for "already applied": the applied-identity
        // watermark (#105). It is seeded once from a full-log scan when the
        // sidecar first appears (preserving pre-watermark skip behavior for
        // identities recorded before the upgrade), then consumed
        // incrementally: each commit below advances the per-sender high-water
        // mark instead of rescanning the log, so a batch costs O(batch), not
        // O(total log). Events whose entries compaction removed stay known —
        // the watermark survives log rewrites by construction.
        let log_path = repo.agent_log_path(&self.workspace_name);
        let watermark_path = sync_watermark_path(&log_path);
        let mut watermark = match SyncWatermark::load(&watermark_path) {
            Ok(Some(watermark)) => watermark,
            Ok(None) => {
                let existing = log
                    .read_all()
                    .await
                    .map_err(|source| ApplyPullError { applied: 0, source })?;
                let mut watermark = SyncWatermark {
                    high_water: std::collections::BTreeMap::new(),
                };
                watermark.seed_from_entries(&existing);
                watermark
                    .persist(&watermark_path)
                    .map_err(|source| ApplyPullError {
                        applied: 0,
                        source: anyhow::Error::new(source).context(format!(
                            "failed to persist sync watermark sidecar {}",
                            watermark_path.display()
                        )),
                    })?;
                watermark
            }
            Err(source) => {
                return Err(ApplyPullError {
                    applied: 0,
                    source: anyhow::Error::new(source).context(format!(
                        "failed to read sync watermark sidecar {}",
                        watermark_path.display()
                    )),
                });
            }
        };
        let mut applied: u64 = 0;
        // Helper: every fallible step maps its error into ApplyPullError
        // carrying the durable committed-prefix count at that point.
        let failed = |applied: u64, source: anyhow::Error| ApplyPullError { applied, source };

        for event in events {
            // Idempotency gate: consult the watermark BEFORE mutating the fs
            // or the log. Already-applied events are a no-op and are NOT
            // recounted, so identical-batch retries change nothing. Only an
            // identified sender can hit the gate (see [`SyncWatermark::covers`]
            // for why an unidentified event must always fall through).
            if watermark.covers(&event.sender, event.seq) {
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
                "verify" => crate::log::OpType::Verify,
                _ => {
                    tracing::warn!("unknown event op '{}', skipping", event.op);
                    continue;
                }
            };

            // A verify event without a parseable payload is malformed: it
            // would append a Verify op with verify == None, which no reader
            // can distinguish from corruption. Warn, skip, and let the sender
            // retry — never half-land a ledger record.
            let synced_verify_payload = if op == crate::log::OpType::Verify {
                match event
                    .verify
                    .as_ref()
                    .map(|value| serde_json::from_value::<crate::log::VerifyRecord>(value.clone()))
                {
                    Some(Ok(record)) => Some(record),
                    Some(Err(err)) => {
                        tracing::warn!(
                            "verify event {} carries an unparseable payload ({}), skipping",
                            event.seq,
                            err
                        );
                        continue;
                    }
                    None => {
                        tracing::warn!(
                            "verify event {} arrived without a payload (pre-ledger sender?), skipping",
                            event.seq
                        );
                        continue;
                    }
                }
            } else {
                None
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
                // Stamped only when this event actually took effect. A skipped
                // effect (a blob that has not arrived yet, a rename whose source
                // was rejected) is a retryable state, not an applied one:
                // stamping it would make the gate skip the event forever while
                // the ACK still reported success. Before this change the next
                // attempt recovered; with an unconditional stamp it never could.
                remote_seq: counts.then_some(event.seq),
                remote_sender: if counts { event.sender.clone() } else { None },
                verify: synced_verify_payload,
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
            if counts {
                if event.sender.is_some() {
                    // Advance the watermark only after the log append
                    // committed: advancing before it could suppress a retry
                    // whose log entry never landed (a silent drop). The
                    // reverse failure — a watermark persist lost after the
                    // entry committed — just leaves the watermark behind the
                    // truth, so a replay re-applies once and self-heals.
                    // #105: this in-batch advance is what retires the
                    // per-batch read_all() rescan above.
                    watermark.advance(&event.sender, event.seq);
                    if let Err(source) = watermark.persist(&watermark_path) {
                        tracing::error!(
                            "failed to persist sync watermark sidecar {}: {}",
                            watermark_path.display(),
                            source
                        );
                    }
                }
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
                verify: None,
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
            verify: None,
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
            verify: None,
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
                verify: None,
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
                verify: None,
            },
        ];
        let err = engine.apply_pull_events(&batch).await.unwrap_err();
        assert_eq!(err.applied, 1);

        // The prefix is durable: fs effect + one log entry.
        assert!(!tmp.path().join("victim.txt").exists());
        assert!(tmp.path().join("blockdir").is_dir());
        {
            let repo = Repository::open(tmp.path()).unwrap();
            let entries = repo.agent_log("default").unwrap().read_all().await.unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].path, Some("victim.txt".to_string()));
            assert_eq!(entries[0].remote_seq, Some(1));
        }

        // The ACK layer reports the prefix accurately instead of disowning it.
        let ack = crate::sync::NoaEventSyncAck::from_apply_result("ws".to_string(), Err(err));
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
                verify: None,
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
                verify: None,
            },
        ];
        assert_eq!(engine.apply_pull_events(&batch).await.unwrap(), 2);
        // Identical retry: no-op, log length unchanged.
        assert_eq!(engine.apply_pull_events(&batch).await.unwrap(), 0);

        {
            let repo = Repository::open(tmp.path()).unwrap();
            let entries = repo.agent_log("default").unwrap().read_all().await.unwrap();
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].remote_seq, Some(1));
            assert_eq!(entries[1].remote_seq, Some(2));
            assert_eq!(entries[0].remote_sender, Some("remote-1".to_string()));
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
            verify: None,
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
            verify: None,
        }];
        assert_eq!(engine.apply_pull_events(&batch_a).await.unwrap(), 1);
        assert_eq!(engine.apply_pull_events(&batch_b).await.unwrap(), 1);

        let repo = Repository::open(tmp.path()).unwrap();
        let entries = repo.agent_log("default").unwrap().read_all().await.unwrap();
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

    fn event(
        seq: u64,
        op: &str,
        path: &str,
        blob_id: Option<&str>,
        sender: Option<&str>,
    ) -> SyncEvent {
        SyncEvent {
            seq,
            op: op.to_string(),
            path: Some(path.to_string()),
            blob_id: blob_id.map(std::string::ToString::to_string),
            from_path: None,
            ts: 100,
            message: None,
            sender: sender.map(std::string::ToString::to_string),
            verify: None,
        }
    }

    fn verify_event(
        seq: u64,
        payload: Option<serde_json::Value>,
        sender: Option<&str>,
    ) -> SyncEvent {
        SyncEvent {
            seq,
            op: "verify".to_string(),
            path: None,
            blob_id: None,
            from_path: None,
            ts: 100,
            message: None,
            sender: sender.map(std::string::ToString::to_string),
            verify: payload,
        }
    }

    fn sample_payload(head: &str) -> serde_json::Value {
        serde_json::json!({
            "schema": "verify.run/1",
            "kind": "partial",
            "repo": "o/r",
            "head": head,
            "tree_clean": true,
            "env": {"toolchain": "t", "platform": "p"},
            "commands": [],
            "not_run": [{"what": "clippy"}],
            "outputs": [],
            "agent": "sync-test",
            "ts": 1
        })
    }

    /// PUSH side round-trip (review R3-M8 on #131): an appended verify entry
    /// must leave this peer with its payload intact — if the `From<&LogEntry>`
    /// projection ever drops the payload, receivers warn+skip FOREVER and the
    /// whole suite stays green (that exact regression survived as SURVIVED).
    #[tokio::test]
    async fn push_side_carries_verify_payload() {
        let tmp = setup_repo();
        {
            let repo = Repository::open(tmp.path()).unwrap();
            let log = repo.agent_log("default").unwrap();
            log.append(&LogEntry {
                seq: 0,
                op: crate::log::OpType::Verify,
                path: None,
                blob_id: None,
                from_path: None,
                resolved_conflict_ours_id: None,
                resolved_conflict_theirs_id: None,
                snapshot_id: None,
                remote_seq: None,
                remote_sender: None,
                verify: Some(crate::log::VerifyRecord {
                    schema: "verify.run/1".to_string(),
                    kind: crate::log::VerifyKind::Sampled,
                    repo: "o/r".to_string(),
                    head: "push-side-head-01".to_string(),
                    tree_clean: true,
                    env: crate::log::VerifyEnv {
                        toolchain: "t".to_string(),
                        platform: "p".to_string(),
                        lock_hash: None,
                        offline: true,
                        hardware: None,
                    },
                    commands: vec![],
                    not_run: vec![crate::log::VerifyNotRun {
                        what: "clippy".to_string(),
                        reason: None,
                    }],
                    outputs: vec![],
                    agent: "push-test".to_string(),
                    prev: Some("deadbeef".to_string()),
                    ts: 7,
                }),
                ts: 7,
                message: None,
            })
            .await
            .unwrap();
        }
        let engine = EventSyncEngine::new(tmp.path(), "default");
        let events = engine.collect_push_events(0).await.unwrap();
        let verify_events: Vec<_> = events.iter().filter(|e| e.op == "verify").collect();
        assert_eq!(verify_events.len(), 1, "verify entry must be collected");
        let ev = verify_events[0];
        let payload = ev
            .verify
            .as_ref()
            .expect("payload must survive the projection");
        assert_eq!(payload["head"], "push-side-head-01");
        assert_eq!(payload["kind"], "sampled");
        assert_eq!(payload["prev"], "deadbeef", "chain link must travel");
        assert_eq!(payload["not_run"][0]["what"], "clippy");
    }

    /// Pre-ledger senders emit events WITHOUT the `verify` key (serde
    /// default); those must keep deserializing — an old peer in a mixed
    /// fleet must not break a new receiver's pull batches.
    #[test]
    fn sync_event_without_verify_key_still_parses() {
        let legacy = r#"{"seq":1,"op":"write","path":"a.txt","ts":100}"#;
        let ev: SyncEvent = serde_json::from_str(legacy).unwrap();
        assert_eq!(ev.op, "write");
        assert!(ev.verify.is_none());
    }

    /// A verify event with a parseable payload lands in the agent log with
    /// the payload byte-intact (the ledger's whole point: records sync).
    #[tokio::test]
    async fn verify_event_with_payload_lands() {
        let tmp = setup_repo();
        let engine = EventSyncEngine::new(tmp.path(), "default");
        let applied = engine
            .apply_pull_events(&[verify_event(
                1,
                Some(sample_payload("abc123")),
                Some("peer-a"),
            )])
            .await
            .unwrap();
        assert_eq!(applied, 1);
        let repo = Repository::open(tmp.path()).unwrap();
        let log = repo.agent_log("default").unwrap();
        let entries = log.read_all().await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].op, crate::log::OpType::Verify);
        let rec = entries[0].verify.as_ref().expect("payload must travel");
        assert_eq!(rec.head, "abc123");
        assert_eq!(entries[0].remote_seq, Some(1), "counts as applied");
    }

    /// Verify events without a payload (pre-ledger sender or malformed) are
    /// skipped loudly and retried — never half-landed as Verify-with-None.
    #[tokio::test]
    async fn verify_event_without_payload_skips_and_retries() {
        let tmp = setup_repo();
        let engine = EventSyncEngine::new(tmp.path(), "default");
        let applied = engine
            .apply_pull_events(&[
                verify_event(1, None, Some("peer-a")),
                verify_event(
                    2,
                    Some(serde_json::json!({"garbage": true})),
                    Some("peer-a"),
                ),
            ])
            .await
            .unwrap();
        assert_eq!(applied, 0, "neither malformed event may count");
        drop(engine); // redb is single-writer per process; release before reopening
        let repo = Repository::open(tmp.path()).unwrap();
        let log = repo.agent_log("default").unwrap();
        assert!(
            log.read_all().await.unwrap().is_empty(),
            "nothing half-lands"
        );
        drop(repo);
        drop(log);
        // Same seq arriving again (sender retry) must still apply cleanly.
        let engine = EventSyncEngine::new(tmp.path(), "default");
        let reapplied = engine
            .apply_pull_events(&[verify_event(
                1,
                Some(sample_payload("retry")),
                Some("peer-a"),
            )])
            .await
            .unwrap();
        assert_eq!(reapplied, 1, "skipped events stay retryable, not stuck");
    }

    /// Two peers whose first events both carry sequence 1 must both land.
    ///
    /// An unidentified event cannot be deduped: the key degenerates to
    /// `(None, 1)`, which every peer's first event shares, so the second peer's
    /// event would be skipped while the ACK still reported success. Review found
    /// this on 2026-10-06 by driving the batch through `collect_push_events`,
    /// which never fills `sender`.
    #[tokio::test]
    async fn test_unidentified_events_are_never_deduped() {
        let tmp = setup_repo();
        {
            let repo = Repository::open(tmp.path()).unwrap();
            let log = repo.agent_log("default").unwrap();
            log.append(&LogEntry {
                seq: 1,
                op: OpType::Write,
                path: Some("first.txt".to_string()),
                blob_id: Some("h1".to_string()),
                from_path: None,
                resolved_conflict_ours_id: None,
                resolved_conflict_theirs_id: None,
                snapshot_id: None,
                remote_seq: None,
                remote_sender: None,
                verify: None,
                ts: 100,
                message: None,
            })
            .await
            .unwrap();
        }

        let engine = EventSyncEngine::new(tmp.path(), "default");
        let applied = engine
            .apply_pull_events(&[
                event(1, "delete", "peer-a.txt", None, None),
                event(1, "delete", "peer-b.txt", None, None),
            ])
            .await
            .unwrap();
        assert_eq!(
            applied, 2,
            "both peers' seq-1 events must apply; an unidentified event must \
             never be treated as already seen"
        );
    }

    /// An effect that was skipped because its blob had not arrived yet must not
    /// be stamped as applied, or the retry can never take effect.
    ///
    /// This is the data-loss half of the same review: the commit point used to
    /// record the remote identity unconditionally, so a write whose blob was
    /// still in flight was marked as applied while the ACK reported success —
    /// the file never appeared. Before that change the next attempt recovered.
    #[tokio::test]
    async fn test_event_whose_blob_is_missing_is_retried_not_sealed() {
        let tmp = setup_repo();
        let engine = EventSyncEngine::new(tmp.path(), "default");

        // First attempt: the event names a blob this store has never seen — the
        // normal state while the content transfer is still in flight.
        let first = event(
            7,
            "write",
            "awaited.txt",
            Some("noa_not_yet"),
            Some("peer-a"),
        );
        let applied = engine.apply_pull_events(&[first]).await.unwrap();
        assert_eq!(applied, 0, "an event with no blob has not been applied");

        // The blob arrives with the next transfer step. The retry keeps the same
        // identity — `(peer-a, 7)` — which is the whole point: had the skipped
        // attempt recorded that identity, the gate would skip this retry too.
        let blob_id = {
            let repo = Repository::open(tmp.path()).unwrap();
            repo.object_store()
                .unwrap()
                .put_blob(b"arrived\n")
                .await
                .unwrap()
                .0
        };
        let retry = event(
            7,
            "write",
            "awaited.txt",
            Some(blob_id.as_str()),
            Some("peer-a"),
        );
        let applied = engine.apply_pull_events(&[retry]).await.unwrap();
        assert_eq!(
            applied, 1,
            "the retry must apply once the blob exists; the first attempt must \
             not have sealed the event as already applied"
        );
        assert!(
            tmp.path().join("awaited.txt").exists(),
            "the file must exist after the retry"
        );
    }
}

/// Regression tests for issue #105: `apply_pull_events` used to read the
/// ENTIRE agent log on every batch to rebuild the applied-identity set.
/// The fix consumes identities incrementally via a persisted per-sender
/// watermark sidecar (`<log>.sync-watermark`), so a batch costs O(batch)
/// instead of O(total log) and applied identities survive log rewrites.
#[cfg(test)]
mod watermark_incremental_regression {
    use super::*;
    use crate::log::OpType;
    use tempfile::TempDir;

    fn setup_repo() -> TempDir {
        let tmp = TempDir::new().unwrap();
        let repo = Repository::init(tmp.path()).unwrap();
        drop(repo);
        tmp
    }

    fn snap_event(sender: &str, seq: u64) -> SyncEvent {
        SyncEvent {
            seq,
            op: "snapshot".to_string(),
            path: None,
            blob_id: None,
            from_path: None,
            ts: seq * 100,
            message: None,
            sender: Some(sender.to_string()),
            verify: None,
        }
    }

    fn batch(sender: &str, seqs: &[u64]) -> Vec<SyncEvent> {
        seqs.iter().map(|seq| snap_event(sender, *seq)).collect()
    }

    async fn compact_all_log(tmp: &TempDir) {
        let repo = Repository::open(tmp.path()).unwrap();
        let log = repo.agent_log("default").unwrap();
        let next = log.next_seq().await.unwrap();
        log.compact_to(next.saturating_sub(1)).await.unwrap();
        assert!(
            log.read_all().await.unwrap().is_empty(),
            "fixture: compaction must empty the log"
        );
    }

    /// #105: identities removed from the log by compaction must stay known.
    /// Pre-fix, the per-batch read_all() rebuilt the set from the log, so a
    /// replayed batch whose identities had been compacted away re-applied
    /// (Ok(N)); with the watermark it is skipped (Ok(0)).
    #[tokio::test]
    async fn watermark_survives_log_compaction() {
        let tmp = setup_repo();
        let engine = EventSyncEngine::new(tmp.path(), "default");

        let peer_a = batch("peer-a", &[1, 2, 3]);
        assert_eq!(engine.apply_pull_events(&peer_a).await.unwrap(), 3);

        // Rewrite the log out from under the applied identities.
        compact_all_log(&tmp).await;

        assert_eq!(
            engine.apply_pull_events(&peer_a).await.unwrap(),
            0,
            "#105: a replayed batch must stay skipped even after its log \
             evidence was compacted away"
        );

        // The gate must not over-skip: fresh events still apply once, and a
        // re-send of them is skipped again.
        let peer_a_next = batch("peer-a", &[4]);
        assert_eq!(engine.apply_pull_events(&peer_a_next).await.unwrap(), 1);
        assert_eq!(engine.apply_pull_events(&peer_a_next).await.unwrap(), 0);
    }

    /// #105 upgrade path: identities recorded in the log BEFORE the
    /// watermark sidecar existed must still suppress replays — the first
    /// call seeds the watermark from a single full-log scan.
    #[tokio::test]
    async fn watermark_seeds_from_preexisting_log_identities() {
        let tmp = setup_repo();
        {
            let repo = Repository::open(tmp.path()).unwrap();
            let log = repo.agent_log("default").unwrap();
            for seq in 7..=9 {
                log.append(&LogEntry {
                    seq: 0,
                    op: OpType::Snapshot,
                    path: None,
                    blob_id: None,
                    from_path: None,
                    resolved_conflict_ours_id: None,
                    resolved_conflict_theirs_id: None,
                    snapshot_id: None,
                    remote_seq: Some(seq),
                    remote_sender: Some("peer-b".to_string()),
                    verify: None,
                    ts: seq * 100,
                    message: None,
                })
                .await
                .unwrap();
            }
        }

        let engine = EventSyncEngine::new(tmp.path(), "default");
        let replayed = batch("peer-b", &[8]);
        assert_eq!(
            engine.apply_pull_events(&replayed).await.unwrap(),
            0,
            "#105: pre-watermark identities must be seeded from the log and \
             still suppress a replay"
        );
        let fresh = batch("peer-b", &[10]);
        assert_eq!(engine.apply_pull_events(&fresh).await.unwrap(), 1);
    }

    /// #105 documented reset channel: deleting the sidecar resets the
    /// watermark. After a reset the next batch reseeds from whatever
    /// identities the log still holds; events whose evidence is gone
    /// re-apply once (the safe direction — fs ops tolerate replay).
    #[tokio::test]
    async fn watermark_reset_re_applies_unprovable_events() {
        let tmp = setup_repo();
        // Capture the sidecar path up front — the repo open/close discipline
        // in this file keeps at most one handle alive at a time (redb 4.5
        // holds a whole-file writer lock, so an open racing a just-closed
        // handle can hit DatabaseAlreadyOpen).
        let sidecar = {
            let repo = Repository::open(tmp.path()).unwrap();
            sync_watermark_path(&repo.agent_log_path("default"))
        };
        let engine = EventSyncEngine::new(tmp.path(), "default");

        let peer_c = batch("peer-c", &[1]);
        assert_eq!(engine.apply_pull_events(&peer_c).await.unwrap(), 1);

        // With the log compacted, the watermark alone guards the replay.
        compact_all_log(&tmp).await;
        assert_eq!(engine.apply_pull_events(&peer_c).await.unwrap(), 0);

        // Reset: remove the sidecar; nothing proves the event applied
        // anymore, so it re-applies.
        std::fs::remove_file(&sidecar).unwrap();
        assert_eq!(
            engine.apply_pull_events(&peer_c).await.unwrap(),
            1,
            "#105: deleting the watermark sidecar is the documented reset; \
             unprovable events re-apply"
        );
    }
}
