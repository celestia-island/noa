use async_trait::async_trait;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Mutex,
};

use super::{format, AgentLog, LogEntry};
use crate::error::{NoaError, Result};

pub struct FileAgentLog {
    file: Mutex<File>,
    path: PathBuf,
    next_seq: std::sync::atomic::AtomicU64,
}

impl FileAgentLog {
    pub fn create(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .read(true)
            .open(path)
            .map_err(NoaError::Io)?;
        let log = FileAgentLog {
            file: Mutex::new(file),
            path: path.to_path_buf(),
            next_seq: std::sync::atomic::AtomicU64::new(1),
        };
        let max_seq = log.compute_max_seq()?;
        log.next_seq
            .store(max_seq + 1, std::sync::atomic::Ordering::SeqCst);
        Ok(log)
    }

    pub fn open(path: &Path) -> Result<Self> {
        if !path.exists() {
            return Err(NoaError::Io(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("log file not found: {}", path.display()),
            )));
        }
        let file = OpenOptions::new()
            .append(true)
            .read(true)
            .open(path)
            .map_err(NoaError::Io)?;
        let log = FileAgentLog {
            file: Mutex::new(file),
            path: path.to_path_buf(),
            next_seq: std::sync::atomic::AtomicU64::new(1),
        };
        let max_seq = log.compute_max_seq()?;
        log.next_seq
            .store(max_seq + 1, std::sync::atomic::Ordering::SeqCst);
        Ok(log)
    }

    fn compute_max_seq(&self) -> Result<u64> {
        let mut file = self
            .file
            .lock()
            .map_err(|e| NoaError::Io(std::io::Error::other(e.to_string())))?;
        file.seek(SeekFrom::End(-2)).ok();
        let mut tail = String::new();
        file.seek(SeekFrom::Start(0))?;
        file.read_to_string(&mut tail)?;
        for line in tail.lines().rev() {
            if line.trim().is_empty() {
                continue;
            }
            if let Ok(entry) = format::deserialize_entry(line) {
                return Ok(entry.seq);
            }
        }
        Ok(0)
    }
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
        let mut file = self
            .file
            .lock()
            .map_err(|e| NoaError::Io(std::io::Error::other(e.to_string())))?;
        writeln!(file, "{}", line)?;
        file.sync_data()?;
        Ok(seq)
    }

    async fn read_since(&self, seq: u64) -> Result<Vec<LogEntry>> {
        let entries = self.read_all().await?;
        Ok(entries.into_iter().filter(|e| e.seq > seq).collect())
    }

    async fn read_all(&self) -> Result<Vec<LogEntry>> {
        let mut file = self
            .file
            .lock()
            .map_err(|e| NoaError::Io(std::io::Error::other(e.to_string())))?;
        file.seek(SeekFrom::Start(0))?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        format::deserialize_entries(&content)
    }

    async fn next_seq(&self) -> Result<u64> {
        Ok(self.next_seq.load(std::sync::atomic::Ordering::SeqCst))
    }

    async fn compact_to(&self, up_to_seq: u64) -> Result<()> {
        let mut file = self
            .file
            .lock()
            .map_err(|e| NoaError::Io(std::io::Error::other(e.to_string())))?;

        file.seek(SeekFrom::Start(0))?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        let entries = format::deserialize_entries(&content)?;
        let remaining: Vec<LogEntry> = entries.into_iter().filter(|e| e.seq > up_to_seq).collect();

        let temp_path = compact_temp_path(&self.path);
        let backup_path = self.path.with_extension("bak");

        {
            let mut tmp_file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&temp_path)
                .map_err(NoaError::Io)?;

            for entry in &remaining {
                let line = format::serialize_entry(entry)?;
                writeln!(tmp_file, "{}", line)?;
            }
            tmp_file.sync_all()?;
        }

        if self.path.exists() {
            if let Err(e) = std::fs::rename(&self.path, &backup_path) {
                let _ = std::fs::remove_file(&temp_path);
                return Err(NoaError::Io(e));
            }
        }

        if let Err(e) = std::fs::rename(&temp_path, &self.path) {
            let _ = std::fs::rename(&backup_path, &self.path);
            return Err(NoaError::Io(e));
        }

        let _ = std::fs::remove_file(&backup_path);

        *file = OpenOptions::new()
            .append(true)
            .read(true)
            .open(&self.path)
            .map_err(NoaError::Io)?;

        Ok(())
    }
}

fn compact_temp_path(original: &Path) -> PathBuf {
    let file_name = original
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "log".to_string());
    original.with_file_name(format!(".{}.compact.tmp", file_name))
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
        assert!(next > 3, "create() on existing file must compute next_seq from content, got {}", next);

        let seq = log2.append(&make_entry(4, OpType::Write, "d.rs", 400)).await.unwrap();
        assert!(seq > 3, "appended seq must be > 3, got {}", seq);
    }

    #[tokio::test]
    async fn test_compact_removes_old_entries() {
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("compact.log");
        let log = FileAgentLog::create(&log_path).unwrap();
        for i in 1..=5 {
            log.append(&make_entry(i, OpType::Write, &format!("f{}.rs", i), i * 100))
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

        let seq = log.append(&make_entry(5, OpType::Write, "new.rs", 500)).await.unwrap();
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
}
