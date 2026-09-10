use std::sync::Arc;
use async_trait::async_trait;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use fs2::FileExt;
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use super::{format, AgentLog, LogEntry};
use crate::error::Result;

/// Durable, cross-handle sequence allocator for a single JSONL agent log.
///
/// Source of truth for the next sequence number is the `<log>.seq` sidecar
/// file holding the next value to allocate (decimal, e.g. `41\n` means the
/// next `append` returns 41). It is updated — under an exclusive OS file
/// lock on `<log>.lock` — *before* the entry itself is written, so a crash
/// between the two leaves a (harmless) gap rather than a duplicate.
/// The in-memory `next_seq` is only a fast-path cache: every allocation and
/// every `next_seq()` re-reads the sidecar under/after the lock and takes
/// the max, so compaction (which destroys the file evidence) and concurrent
/// handles (same process or another) can never rewind or collide.
pub struct FileAgentLog {
    path: PathBuf,
    seq_path: PathBuf,
    lock_path: PathBuf,
    next_seq: Arc<std::sync::atomic::AtomicU64>,
    compact_lock: tokio::sync::Mutex<()>,
}

impl FileAgentLog {
    pub fn create(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        cleanup_stale_temp(path);
        let seq_path = seq_sidecar_path(path);
        let lock_path = lock_file_path(path);
        cleanup_stale_seq_temp(&seq_path);
        {
            let mut opts = OpenOptions::new();
            opts.create(true).append(true).read(true);
            #[cfg(unix)]
            opts.mode(0o600);
            opts.open(path)?;
        }
        let next = init_high_water(path, &seq_path, &lock_path)?;
        Ok(FileAgentLog {
            path: path.to_path_buf(),
            seq_path,
            lock_path,
            next_seq: Arc::new(std::sync::atomic::AtomicU64::new(next)),
            compact_lock: tokio::sync::Mutex::new(()),
        })
    }

    pub fn open(path: &Path) -> Result<Self> {
        if !path.exists() {
            anyhow::bail!("log file not found: {}", path.display());
        }
        cleanup_stale_temp(path);
        let seq_path = seq_sidecar_path(path);
        let lock_path = lock_file_path(path);
        cleanup_stale_seq_temp(&seq_path);
        let next = init_high_water(path, &seq_path, &lock_path)?;
        Ok(FileAgentLog {
            path: path.to_path_buf(),
            seq_path,
            lock_path,
            next_seq: Arc::new(std::sync::atomic::AtomicU64::new(next)),
            compact_lock: tokio::sync::Mutex::new(()),
        })
    }
}

/// Sidecar holding the durable seq high-water mark: `<log>.seq`.
fn seq_sidecar_path(log_path: &Path) -> PathBuf {
    let mut s = log_path.as_os_str().to_owned();
    s.push(".seq");
    PathBuf::from(s)
}

/// Lock file serializing allocate-append across handles and processes.
fn lock_file_path(log_path: &Path) -> PathBuf {
    let mut s = log_path.as_os_str().to_owned();
    s.push(".lock");
    PathBuf::from(s)
}

fn read_seq_file(seq_path: &Path) -> Option<u64> {
    let content = std::fs::read_to_string(seq_path).ok()?;
    content.trim().parse::<u64>().ok()
}

/// Crash-safe sidecar write: temp file + fsync + atomic rename.
fn write_seq_file(seq_path: &Path, next: u64) -> Result<()> {
    if let Some(parent) = seq_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut tmp_os = seq_path.as_os_str().to_owned();
    tmp_os.push(".tmp");
    let tmp = PathBuf::from(tmp_os);
    {
        let mut f = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&tmp)?;
        write!(f, "{next}\n")?;
        f.sync_all()?;
    }
    std::fs::rename(&tmp, seq_path)?;
    Ok(())
}

fn cleanup_stale_seq_temp(seq_path: &Path) {
    let mut tmp_os = seq_path.as_os_str().to_owned();
    tmp_os.push(".tmp");
    let tmp = PathBuf::from(tmp_os);
    if tmp.exists() {
        if let Err(e) = std::fs::remove_file(&tmp) {
            tracing::warn!(
                "failed to remove stale seq temp file {}: {e}",
                tmp.display()
            );
        }
    }
}

/// Blocking exclusive OS lock (threads *and* processes). Released on drop.
fn acquire_exclusive(lock_path: &Path) -> Result<File> {
    if let Some(parent) = lock_path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let mut opts = OpenOptions::new();
    opts.create(true).read(true).write(true);
    #[cfg(unix)]
    opts.mode(0o600);
    let file = opts.open(lock_path)?;
    file.lock_exclusive()?;
    Ok(file)
}

/// Resolve the durable high-water mark: `max(sidecar, file_max + 1, 1)`,
/// persisting it when the sidecar is missing or behind (first open of a
/// pre-sidecar log, or a sidecar lost to external deletion). Runs under the
/// exclusive lock so concurrent creates converge instead of racing.
fn init_high_water(log_path: &Path, seq_path: &Path, lock_path: &Path) -> Result<u64> {
    let _lock = acquire_exclusive(lock_path)?;
    let file_max = {
        let mut f = OpenOptions::new().read(true).open(log_path)?;
        compute_max_seq_from_file(&mut f)?
    };
    let persisted = read_seq_file(seq_path).unwrap_or(0);
    let next = persisted.max(file_max.saturating_add(1)).max(1);
    if persisted < next {
        write_seq_file(seq_path, next)?;
    }
    Ok(next)
}

fn compute_max_seq_from_file(file: &mut File) -> Result<u64> {
    let file_len = file.seek(SeekFrom::End(0))?;

    if file_len < 65536 {
        file.seek(SeekFrom::Start(0))?;
        let mut content = String::new();
        file.read_to_string(&mut content)?;
        return max_seq_from_lines(content.lines());
    }

    let read_size = 65536u64.min(file_len);
    file.seek(SeekFrom::End(-(read_size as i64)))?;
    let mut tail = vec![0u8; read_size as usize];
    file.read_exact(&mut tail)?;

    let last_newline = tail.iter().rposition(|&b| b == b'\n').unwrap_or(0);
    if last_newline > 0 {
        tail = tail[..last_newline].to_vec();
    }

    let tail_content = String::from_utf8_lossy(&tail);

    if let Some(seq) = try_parse_last_seq(&tail_content) {
        return Ok(seq);
    }

    file.seek(SeekFrom::Start(0))?;
    let mut content = String::new();
    file.read_to_string(&mut content)?;
    max_seq_from_lines(content.lines()).map_err(|e| {
        tracing::error!("failed to parse full log file content: {e}");
        e
    })
}

fn try_parse_last_seq(content: &str) -> Option<u64> {
    for line in content.lines().rev() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(entry) = format::deserialize_entry(trimmed) {
            return Some(entry.seq);
        }
    }
    None
}

fn max_seq_from_lines<'a>(lines: impl DoubleEndedIterator<Item = &'a str>) -> Result<u64> {
    for line in lines.rev() {
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
        let _guard = self.compact_lock.lock().await;
        let path = self.path.clone();
        let seq_path = self.seq_path.clone();
        let lock_path = self.lock_path.clone();
        let cache = Arc::clone(&self.next_seq);
        let template = entry.clone();
        tokio::task::spawn_blocking(move || {
            // Exclusive OS lock: serializes read-allocate-append across all
            // handles in this process AND across processes.
            let _lock = acquire_exclusive(&lock_path)?;
            // Re-read durable state under the lock; self-heals against any
            // entry written outside this protocol (max with file evidence).
            let file_max = if path.exists() {
                let mut f = OpenOptions::new().read(true).open(&path)?;
                compute_max_seq_from_file(&mut f)?
            } else {
                0
            };
            let persisted = read_seq_file(&seq_path).unwrap_or(0);
            let cached = cache.load(std::sync::atomic::Ordering::Acquire);
            let seq = persisted.max(file_max.saturating_add(1)).max(cached).max(1);
            let next = seq.saturating_add(1);
            // Reserve `seq` durably BEFORE the entry is visible: a crash in
            // between leaves a gap (monotonicity holds); the reverse order
            // could duplicate `seq` on reopen.
            write_seq_file(&seq_path, next)?;
            let mut assigned_entry = template;
            assigned_entry.seq = seq;
            let line = format::serialize_entry(&assigned_entry)?;
            let mut record = line.into_bytes();
            record.push(b'\n');
            {
                let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
                file.write_all(&record)?;
                file.sync_all()?;
            }
            cache.store(next, std::sync::atomic::Ordering::Release);
            Ok(seq)
        })
        .await?
    }

    async fn read_since(&self, seq: u64) -> Result<Vec<LogEntry>> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            if !path.exists() {
                return Ok(Vec::new());
            }
            let mut file = OpenOptions::new().read(true).open(&path)?;
            let mut reader = std::io::BufReader::new(&mut file);
            format::deserialize_entries_since(&mut reader, seq)
        })
        .await?
    }

    async fn read_all(&self) -> Result<Vec<LogEntry>> {
        let path = self.path.clone();
        tokio::task::spawn_blocking(move || {
            if !path.exists() {
                return Ok(Vec::new());
            }
            let mut file = OpenOptions::new().read(true).open(&path)?;
            let mut content = String::new();
            file.read_to_string(&mut content)?;
            format::deserialize_entries(&content)
        })
        .await?
    }

    async fn next_seq(&self) -> Result<u64> {
        let cached = self.next_seq.load(std::sync::atomic::Ordering::Acquire);
        // The sidecar is the source of truth: other handles (this process
        // or another) may have advanced it since we cached.
        match read_seq_file(&self.seq_path) {
            Some(persisted) => {
                let next = persisted.max(cached);
                if next > cached {
                    self.next_seq
                        .store(next, std::sync::atomic::Ordering::Release);
                }
                Ok(next)
            }
            None => Ok(cached),
        }
    }

    async fn compact_to(&self, up_to_seq: u64) -> Result<()> {
        let _guard = self.compact_lock.lock().await;
        let path = self.path.clone();
        let seq_path = self.seq_path.clone();
        let lock_path = self.lock_path.clone();
        let cache = Arc::clone(&self.next_seq);
        tokio::task::spawn_blocking(move || {
            // Same exclusive lock as append: compaction must not interleave
            // with an allocate-append on any handle.
            let _lock = acquire_exclusive(&lock_path)?;
            cleanup_stale_temp(&path);
            if !path.exists() {
                return Ok(());
            }
            let file = OpenOptions::new().read(true).open(&path).map_err(|e| {
                anyhow::anyhow!(
                    "failed to open log for compaction ({}): {e}",
                    path.display()
                )
            })?;
            let reader = std::io::BufReader::new(file);

            let temp_path = compact_temp_path(&path);

            let max_remaining = {
                let mut tmp_file = OpenOptions::new()
                    .create(true)
                    .write(true)
                    .truncate(true)
                    .open(&temp_path)?;

                let mut max_remaining = 0u64;
                let mut line = String::new();
                let mut reader = reader;
                while reader.read_line(&mut line)? > 0 {
                    if line.trim().is_empty() {
                        line.clear();
                        continue;
                    }
                    match format::deserialize_entry(&line) {
                        Ok(entry) if entry.seq > up_to_seq => {
                            max_remaining = max_remaining.max(entry.seq);
                            writeln!(tmp_file, "{}", line.trim())?;
                        }
                        Ok(_) => {}
                        Err(e) => {
                            tracing::warn!("skipping corrupted log line during compaction: {e}");
                        }
                    }
                    line.clear();
                }
                tmp_file.sync_all()?;
                max_remaining
            };

            if let Err(e) = std::fs::rename(&temp_path, &path) {
                let _ = std::fs::remove_file(&temp_path);
                return Err(e.into());
            }

            // The durable high-water mark never moves backwards: compaction
            // destroys file evidence, so re-persist the max. The sidecar
            // already holds every allocated seq, hence `persisted` dominates
            // `max_remaining + 1` in the normal case; the max covers a
            // missing/stale sidecar.
            let cached = cache.load(std::sync::atomic::Ordering::Acquire);
            let persisted = read_seq_file(&seq_path).unwrap_or(0);
            let next = cached
                .max(persisted)
                .max(max_remaining.saturating_add(1))
                .max(1);
            if persisted < next {
                write_seq_file(&seq_path, next)?;
            }
            cache.store(next, std::sync::atomic::Ordering::Release);

            Ok(())
        })
        .await?
    }
}

fn compact_temp_path(original: &Path) -> PathBuf {
    let file_name = original
        .file_name()
        .map_or_else(|| "log".to_string(), |n| n.to_string_lossy().into_owned());
    original.with_file_name(format!("noa-compact-{file_name}.tmp"))
}

fn cleanup_stale_temp(path: &Path) {
    let temp_path = compact_temp_path(path);
    if temp_path.exists() {
        if let Err(e) = std::fs::remove_file(&temp_path) {
            tracing::warn!(
                "failed to remove stale compact temp file {}: {e}",
                temp_path.display()
            );
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

    /// Issue #71 (Bugs 1+2): seq allocation must be durable and shared.
    /// Part 1 mirrors the compact → reopen → append repro: after compacting
    /// everything away, a reopened handle must stay monotonic and the new
    /// entry must be visible to `read_since(prior_last_seq)`. Part 2 mirrors
    /// the concurrent-handles repro: two handles opened on the same file
    /// must allocate distinct seqs.
    #[tokio::test]
    async fn test_seq_allocator_durable_and_shared_across_handles() {
        use crate::log::AgentLog;
        let tmp = TempDir::new().unwrap();
        let log_path = tmp.path().join("issue71.log");

        // --- Bug 1: compact-all, drop, reopen, append stays monotonic ---
        let e1_seq = {
            let log = FileAgentLog::create(&log_path).unwrap();
            let s = log
                .append(&make_entry(0, OpType::Write, "e1.txt", 100))
                .await
                .unwrap();
            let next = log.next_seq().await.unwrap();
            log.compact_to(next.saturating_sub(1)).await.unwrap();
            assert!(
                log.read_all().await.unwrap().is_empty(),
                "compact-all must empty the log"
            );
            s
        };
        assert_eq!(e1_seq, 1);
        let reopened_next = FileAgentLog::create(&log_path)
            .unwrap()
            .next_seq()
            .await
            .unwrap();
        assert_eq!(
            reopened_next, 2,
            "reopened next_seq must survive compaction (got {reopened_next})"
        );
        let log2 = FileAgentLog::create(&log_path).unwrap();
        let e2_seq = log2
            .append(&make_entry(0, OpType::Write, "e2.txt", 200))
            .await
            .unwrap();
        assert!(e2_seq > e1_seq, "E2.seq ({e2_seq}) must exceed E1.seq ({e1_seq})");
        let since: Vec<u64> = log2
            .read_since(e1_seq)
            .await
            .unwrap()
            .iter()
            .map(|e| e.seq)
            .collect();
        assert!(
            since.contains(&e2_seq),
            "E2 (seq {e2_seq}) must be visible to read_since({e1_seq}), got {since:?}"
        );

        // --- Bug 2: two handles on the same file allocate distinct seqs ---
        let log_b = FileAgentLog::create(&log_path).unwrap();
        let ea = make_entry(0, OpType::Write, "a.txt", 300);
        let eb = make_entry(0, OpType::Write, "b.txt", 400);
        let (seq_a, seq_b) = tokio::join!(log2.append(&ea), log_b.append(&eb),);
        let (seq_a, seq_b) = (seq_a.unwrap(), seq_b.unwrap());
        assert_ne!(
            seq_a, seq_b,
            "concurrent handles must allocate distinct seqs (got {seq_a} and {seq_b})"
        );
        let mut seqs: Vec<u64> = log2
            .read_all()
            .await
            .unwrap()
            .iter()
            .map(|e| e.seq)
            .collect();
        seqs.sort_unstable();
        assert!(
            seqs.windows(2).all(|w| w[0] != w[1]),
            "read_all must contain no duplicate seqs, got {seqs:?}"
        );
    }
}
