use async_trait::async_trait;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

use super::{format, AgentLog, LogEntry};
use crate::error::{NoaError, Result};

pub struct FileAgentLog {
    path: PathBuf,
    next_seq: std::sync::atomic::AtomicU64,
}

impl FileAgentLog {
    pub fn create(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        cleanup_stale_temp(path);
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(path)
            .map_err(NoaError::Io)?;
        let metadata = file.metadata().map_err(NoaError::Io)?;
        let mut perms = metadata.permissions();
        perms.set_mode(0o600);
        std::fs::set_permissions(path, perms).ok();
        let max_seq = compute_max_seq_from_file(&mut file)?;
        Ok(FileAgentLog {
            path: path.to_path_buf(),
            next_seq: std::sync::atomic::AtomicU64::new(max_seq + 1),
        })
    }

    pub fn open(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(NoaError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("log file not found: {}", path.display()),
            )));
        }
        cleanup_stale_temp(path);
        let mut file = OpenOptions::new()
            .append(true)
            .read(true)
            .open(path)
            .map_err(NoaError::Io)?;
        let max_seq = compute_max_seq_from_file(&mut file)?;
        Ok(FileAgentLog {
            path: path.to_path_buf(),
            next_seq: std::sync::atomic::AtomicU64::new(max_seq + 1),
        })
    }
}

fn compute_max_seq_from_file(file: &mut File) -> Result<u64> {
    let file_len = file.seek(SeekFrom::End(0))?;

    if file_len < 65536 {
        file.seek(SeekFrom::Start(0))?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        for line in content.lines().rev() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(entry) = format::deserialize_entry(line) {
                return Ok(entry.seq);
            }
        }
        return Ok(0);
    }

    let read_size = 8192u64.min(file_len);
    file.seek(SeekFrom::End(-(read_size as i64)))?;
    let mut tail = vec![0u8; read_size as usize];
    file.read_exact(&mut tail)?;
    let tail_str = String::from_utf8_lossy(&tail);
    for line in tail_str.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(entry) = format::deserialize_entry(trimmed) {
            return Ok(entry.seq);
        }
    }
    file.seek(SeekFrom::Start(0))?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    for line in content.lines().rev() {
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = format::deserialize_entry(line) {
            return Ok(entry.seq);
        }
    }
    Ok(0)
}

#[async_trait]
impl AgentLog for FileAgentLog {
    async fn append(&self, entry: &LogEntry) -> Result<u64> {
        let seq = self
            .next_seq
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut assigned_entry = entry.clone();
        assigned_entry.seq = seq;
        let line = format::serialize_entry(&assigned_entry)?;
        let mut record = line.into_bytes();
        record.push(b'\n');
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            use std::io::Write;
            let mut file = OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .map_err(NoaError::Io)?;
            file.write_all(&record).map_err(NoaError::Io)?;
            file.sync_data().map_err(NoaError::Io)?;
            Ok::<_, NoaError>(seq)
        })
        .await
        .map_err(|e| NoaError::Internal(e.to_string()))?
    }

    async fn read_since(&self, seq: u64) -> Result<Vec<LogEntry>> {
        let entries = self.read_all().await?;
        Ok(entries.into_iter().filter(|e| e.seq > seq).collect())
    }

    async fn read_all(&self) -> Result<Vec<LogEntry>> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            let mut file = OpenOptions::new()
                .read(true)
                .open(&path)
                .map_err(NoaError::Io)?;
            let mut content = String::new();
            file.read_to_string(&mut content).map_err(NoaError::Io)?;
            format::deserialize_entries(&content)
        })
        .await
        .map_err(|e| NoaError::Internal(e.to_string()))?
    }

    async fn next_seq(&self) -> Result<u64> {
        Ok(self.next_seq.load(std::sync::atomic::Ordering::SeqCst))
    }

    async fn compact_to(&self, up_to_seq: u64) -> Result<()> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            cleanup_stale_temp(&path);
            let mut file = OpenOptions::new()
                .read(true)
                .open(&path)
                .map_err(NoaError::Io)?;
            let mut content = String::new();
            file.read_to_string(&mut content).map_err(NoaError::Io)?;
            drop(file);
            let entries = format::deserialize_entries(&content)?;
            let remaining: Vec<LogEntry> =
                entries.into_iter().filter(|e| e.seq > up_to_seq).collect();

            let temp_path = compact_temp_path(&path);

            {
                let mut tmp_file = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&temp_path)
                    .map_err(NoaError::Io)?;

                for entry in &remaining {
                    let line = format::serialize_entry(entry)?;
                    writeln!(tmp_file, "{line}").map_err(NoaError::Io)?;
                }
                tmp_file.sync_all().map_err(NoaError::Io)?;
            }

            if let Err(e) = std::fs::rename(&temp_path, &path) {
                let _ = std::fs::remove_file(&temp_path);
                return Err(NoaError::Io(e));
            }

            Ok(())
        })
        .await
        .map_err(|e| NoaError::Internal(e.to_string()))?
    }
}

fn compact_temp_path(original: &Path) -> PathBuf {
    let file_name = original
        .file_name()
        .map_or_else(|| "log".to_string(), |n| n.to_string_lossy().into_owned());
    original.with_file_name(format!(".{file_name}.compact.tmp"))
}

fn cleanup_stale_temp(path: &Path) {
    let temp_path = compact_temp_path(path);
    if temp_path.exists() {
        if let Err(e) = std::fs::remove_file(&temp_path) {
            tracing::warn!("failed to remove stale compact temp file {}: {e}", temp_path.display());
        } else {
            tracing::debug!("cleaned up stale compact temp file {}", temp_path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::OpType;
    use tempfile::TempDir;

    fn make_entry(seq: u64, op: OpType, path: &str, ts: u64) -> LogEntry {
        LogEntry {
            seq,
            op,
            path: Some(path.to_string()),
            blob_id: None,
            from_path: None,
            resolved_conflict_ours_id: None,
            resolved_conflict_theirs_id: None,
            snapshot_id: None,
            ts,
            message: None,
        }
    }

    #[tokio::test]
    async fn test_append_and_read() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("test.log");
        let log = FileAgentLog::create(&log_path).unwrap();

        let e1 = make_entry(1, OpType::Write, "a.rs", 100);
        let e2 = make_entry(2, OpType::Delete, "b.rs", 200);

        log.append(&e1).await.unwrap();
        log.append(&e2).await.unwrap();

        let entries = log.read_all().await.unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], e1);
        assert_eq!(entries[1], e2);
    }

    #[tokio::test]
    async fn test_read_since() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("test.log");
        let log = FileAgentLog::create(&log_path).unwrap();

        for i in 1..=5 {
            log.append(&make_entry(
                i,
                OpType::Write,
                &format!("f{}.rs", i),
                i * 100,
            ))
            .await
            .unwrap();
        }

        let entries = log.read_since(2).await.unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].seq, 3);
        assert_eq!(entries[2].seq, 5);
    }

    #[tokio::test]
    async fn test_concurrent_appends() {
        use std::sync::Arc;

        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("concurrent.log");
        let log = Arc::new(FileAgentLog::create(&log_path).unwrap());

        let mut handles = Vec::new();
        for thread_id in 0..10 {
            let log = Arc::clone(&log);
            handles.push(tokio::spawn(async move {
                for i in 0..10 {
                    let seq = thread_id * 10 + i + 1;
                    let entry = make_entry(
                        seq,
                        OpType::Write,
                        &format!("t{}-{}.rs", thread_id, i),
                        seq * 100,
                    );
                    log.append(&entry).await.unwrap();
                }
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        let entries = log.read_all().await.unwrap();
        assert_eq!(entries.len(), 100);
    }

    #[tokio::test]
    async fn test_open_existing() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("existing.log");
        let log = FileAgentLog::create(&log_path).unwrap();
        log.append(&make_entry(1, OpType::Write, "x.rs", 100))
            .await
            .unwrap();
        drop(log);

        let log2 = FileAgentLog::open(&log_path).unwrap();
        let entries = log2.read_all().await.unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[tokio::test]
    async fn test_open_missing_fails() {
        let tmp = TempDir::new().unwrap();
        let result = FileAgentLog::open(&tmp.path().join("missing.log"));
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_create_on_existing_file_preserves_seq() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("seq-test.log");
        let log = FileAgentLog::create(&log_path).unwrap();
        log.append(&make_entry(1, OpType::Write, "a.rs", 100))
            .await
            .unwrap();
        log.append(&make_entry(2, OpType::Write, "b.rs", 200))
            .await
            .unwrap();
        log.append(&make_entry(3, OpType::Write, "c.rs", 300))
            .await
            .unwrap();
        drop(log);

        let log2 = FileAgentLog::create(&log_path).unwrap();
        let next = log2.next_seq().await.unwrap();
        assert!(
            next > 3,
            "create() on existing file must compute next_seq from content, got {}",
            next
        );

        let seq = log2
            .append(&make_entry(4, OpType::Write, "d.rs", 400))
            .await
            .unwrap();
        assert!(seq > 3, "appended seq must be > 3, got {}", seq);
    }

    #[tokio::test]
    async fn test_compact_removes_old_entries() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("compact.log");
        let log = FileAgentLog::create(&log_path).unwrap();
        for i in 1..=5 {
            log.append(&make_entry(
                i,
                OpType::Write,
                &format!("f{}.rs", i),
                i * 100,
            ))
            .await
            .unwrap();
        }

        log.compact_to(3).await.unwrap();

        let entries = log.read_all().await.unwrap();
        assert!(entries.iter().all(|e| e.seq > 3));
        assert_eq!(entries.len(), 2);
    }

    #[tokio::test]
    async fn test_compact_preserves_order() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("compact-order.log");
        let log = FileAgentLog::create(&log_path).unwrap();
        for i in 1..=10 {
            log.append(&make_entry(i, OpType::Write, &format!("{}.rs", i), i * 100))
                .await
                .unwrap();
        }

        log.compact_to(7).await.unwrap();

        let entries = log.read_all().await.unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].seq, 8);
        assert_eq!(entries[1].seq, 9);
        assert_eq!(entries[2].seq, 10);
    }

    #[tokio::test]
    async fn test_compact_then_append() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("compact-append.log");
        let log = FileAgentLog::create(&log_path).unwrap();
        for i in 1..=4 {
            log.append(&make_entry(i, OpType::Write, &format!("{}.rs", i), i * 100))
                .await
                .unwrap();
        }

        log.compact_to(2).await.unwrap();

        let seq = log
            .append(&make_entry(5, OpType::Write, "new.rs", 500))
            .await
            .unwrap();
        assert!(seq > 4, "seq after compact+append must be > 4, got {}", seq);

        let entries = log.read_all().await.unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[tokio::test]
    async fn test_compact_to_zero_removes_all() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("compact-zero.log");
        let log = FileAgentLog::create(&log_path).unwrap();
        for i in 1..=3 {
            log.append(&make_entry(i, OpType::Write, &format!("{}.rs", i), i * 100))
                .await
                .unwrap();
        }

        log.compact_to(3).await.unwrap();

        let entries = log.read_all().await.unwrap();
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn test_append_after_compact_all() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("compact-all-append.log");
        let log = FileAgentLog::create(&log_path).unwrap();
        for i in 1..=3 {
            log.append(&make_entry(i, OpType::Write, &format!("{}.rs", i), i * 100))
                .await
                .unwrap();
        }
        log.compact_to(3).await.unwrap();

        let seq = log
            .append(&make_entry(0, OpType::Write, "after.rs", 400))
            .await
            .unwrap();
        assert!(
            seq > 3,
            "new seq after compact-all must be > 3, got {}",
            seq
        );

        let entries = log.read_all().await.unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[tokio::test]
    async fn test_read_empty_log() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("empty.log");
        let log = FileAgentLog::create(&log_path).unwrap();
        let entries = log.read_all().await.unwrap();
        assert!(entries.is_empty());
    }

    #[tokio::test]
    async fn test_next_seq_empty_log() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("empty-seq.log");
        let log = FileAgentLog::create(&log_path).unwrap();
        let next = log.next_seq().await.unwrap();
        assert!(
            next < 2,
            "empty log next_seq should be 0 or 1, got {}",
            next
        );
    }

    #[tokio::test]
    async fn test_entry_with_all_fields() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("full-entry.log");
        let log = FileAgentLog::create(&log_path).unwrap();

        let entry = LogEntry {
            seq: 1,
            op: OpType::Merge,
            path: Some("src/main.rs".to_string()),
            blob_id: Some("abc123".to_string()),
            from_path: Some("src/old.rs".to_string()),
            resolved_conflict_ours_id: Some("ours1".to_string()),
            resolved_conflict_theirs_id: Some("theirs1".to_string()),
            snapshot_id: Some("noa_snap1".to_string()),
            ts: 12345,
            message: Some("merge conflict resolved".to_string()),
        };
        log.append(&entry).await.unwrap();

        let entries = log.read_all().await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].op, OpType::Merge);
        assert_eq!(entries[0].snapshot_id, Some("noa_snap1".to_string()));
        assert_eq!(
            entries[0].message,
            Some("merge conflict resolved".to_string())
        );
    }

    #[tokio::test]
    async fn test_compact_noop_when_no_entries_to_remove() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("compact-noop.log");
        let log = FileAgentLog::create(&log_path).unwrap();
        for i in 1..=3 {
            log.append(&make_entry(i, OpType::Write, &format!("{}.rs", i), i * 100))
                .await
                .unwrap();
        }

        log.compact_to(0).await.unwrap();

        let entries = log.read_all().await.unwrap();
        assert_eq!(entries.len(), 3);
    }

    #[tokio::test]
    async fn test_compact_interrupted_temp_file_left_behind() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("interrupted.log");
        let log = FileAgentLog::create(&log_path).unwrap();
        for i in 1..=5 {
            log.append(&make_entry(i, OpType::Write, &format!("{}.rs", i), i * 100))
                .await
                .unwrap();
        }
        drop(log);

        let temp_path = compact_temp_path(&log_path);
        std::fs::write(&temp_path, b"trash data").unwrap();

        let log2 = FileAgentLog::open(&log_path).unwrap();
        log2.compact_to(3).await.unwrap();

        let entries = log2.read_all().await.unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|e| e.seq > 3));

        assert!(!temp_path.exists(), "stale temp file should be cleaned up");
    }

    #[tokio::test]
    async fn test_compute_max_seq_large_file_tail() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("large.log");
        let log = FileAgentLog::create(&log_path).unwrap();

        let mut last_seq = 0u64;
        for i in 1..=2000 {
            let entry = LogEntry {
                seq: i,
                op: OpType::Write,
                path: Some(format!("src/very/long/path/to/file/number/{}/module.rs", i)),
                blob_id: Some("abcd1234efgh5678ijkl9012mnop3456qrst7890".to_string()),
                from_path: None,
                resolved_conflict_ours_id: None,
                resolved_conflict_theirs_id: None,
                snapshot_id: Some(format!("noa_snapshot_{}", i)),
                ts: i * 1000,
                message: Some(format!(
                    "commit message number {} that is fairly long to increase file size",
                    i
                )),
            };
            last_seq = log.append(&entry).await.unwrap();
        }

        drop(log);

        let reopened = FileAgentLog::open(&log_path).unwrap();
        let next = reopened.next_seq().await.unwrap();
        assert!(
            next > last_seq,
            "next_seq {} should be > last_appended_seq {}",
            next,
            last_seq
        );

        let entries = reopened.read_all().await.unwrap();
        assert_eq!(entries.len() as u64, last_seq);
    }
}
