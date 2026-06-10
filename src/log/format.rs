use crate::error::Result;
use crate::log::LogEntry;

pub fn serialize_entry(entry: &LogEntry) -> Result<String> {
    Ok(serde_json::to_string(entry)?)
}

pub fn deserialize_entry(line: &str) -> Result<LogEntry> {
    let trimmed = line.trim();
    Ok(serde_json::from_str::<LogEntry>(trimmed)?)
}

pub fn deserialize_entries(content: &str) -> Result<Vec<LogEntry>> {
    let mut entries = Vec::new();
    for (line_num, line) in content.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match deserialize_entry(line) {
            Ok(entry) => entries.push(entry),
            Err(e) => {
                tracing::warn!("skipping corrupted log line {}: {}", line_num + 1, e);
            }
        }
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::OpType;

    #[test]
    fn test_roundtrip() {
        let entry = LogEntry {
            seq: 1,
            op: OpType::Write,
            path: Some("src/main.rs".to_string()),
            blob_id: Some("abc123".to_string()),
            from_path: None,
            resolved_conflict_ours_id: None,
            resolved_conflict_theirs_id: None,
            snapshot_id: None,
            ts: 1_717_592_400_000_000,
            message: None,
        };
        let json = serialize_entry(&entry).unwrap();
        let parsed = deserialize_entry(&json).unwrap();
        assert_eq!(parsed, entry);
    }

    #[test]
    fn test_deserialize_entries_multiline() {
        let content = r#"{"seq":1,"op":"write","path":"a.rs","ts":100}
{"seq":2,"op":"delete","path":"b.rs","ts":200}
"#;
        let entries = deserialize_entries(content).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].seq, 1);
        assert_eq!(entries[1].seq, 2);
    }

    #[test]
    fn test_deserialize_entries_skips_blank() {
        let content = "\n{\"seq\":1,\"op\":\"write\",\"ts\":100}\n\n";
        let entries = deserialize_entries(content).unwrap();
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn test_deserialize_entries_skips_corrupted_lines() {
        let content = r#"{"seq":1,"op":"write","path":"a.rs","ts":100}
CORRUPTED LINE NOT JSON
{"seq":2,"op":"delete","path":"b.rs","ts":200}
also not valid {json
{"seq":3,"op":"write","path":"c.rs","ts":300}
"#;
        let entries = deserialize_entries(content).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].seq, 1);
        assert_eq!(entries[1].seq, 2);
        assert_eq!(entries[2].seq, 3);
    }

    #[test]
    fn test_deserialize_entries_all_corrupted_returns_empty() {
        let content = "not json\nalso not json\n";
        let entries = deserialize_entries(content).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_deserialize_entries_empty_content() {
        let entries = deserialize_entries("").unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn test_deserialize_entries_only_blank_lines() {
        let entries = deserialize_entries("\n\n\n").unwrap();
        assert!(entries.is_empty());
    }
}
