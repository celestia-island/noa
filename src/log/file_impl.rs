use async_trait::async_trait;
use std::{
    fs::{File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
    sync::Mutex,
};

use super::{format, AgentLog, LogEntry};
use crate::error::{NoaError, Result};

pub struct FileAgentLog {
    file: Mutex<File>,
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
        Ok(FileAgentLog {
            file: Mutex::new(file),
        })
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
        Ok(FileAgentLog {
            file: Mutex::new(file),
        })
    }
}

#[async_trait]
impl AgentLog for FileAgentLog {
    async fn append(&self, entry: &LogEntry) -> Result<u64> {
        let line = format::serialize_entry(entry)?;
        let mut file = self
            .file
            .lock()
            .map_err(|e| NoaError::Io(std::io::Error::other(e.to_string())))?;
        writeln!(file, "{}", line)?;
        file.sync_all()?;
        Ok(entry.seq)
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
}
