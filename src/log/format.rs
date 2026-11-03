use crate::{error::Result, log::LogEntry};

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

pub fn deserialize_entries_since(
    reader: &mut impl std::io::BufRead,
    since_seq: u64,
) -> Result<Vec<LogEntry>> {
    let mut entries = Vec::new();
    let mut line = String::new();
    let mut line_num = 0u64;
    while reader.read_line(&mut line)? > 0 {
        line_num += 1;
        if line.trim().is_empty() {
            line.clear();
            continue;
        }
        match deserialize_entry(&line) {
            Ok(entry) => {
                if entry.seq > since_seq {
                    entries.push(entry);
                }
            }
            Err(e) => {
                tracing::warn!("skipping corrupted log line {}: {}", line_num, e);
            }
        }
        line.clear();
    }
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::log::{
        OpType, VerifyCommand, VerifyEnv, VerifyKind, VerifyNotRun, VerifyOutput, VerifyRecord,
    };
    use serde::{Deserialize, Serialize};

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
            remote_seq: None,
            remote_sender: None,
            verify: None,
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

    // ---- verify.run/1 compatibility suite -------------------------------

    fn sample_verify_record() -> VerifyRecord {
        VerifyRecord {
            schema: "verify.run/1".to_string(),
            kind: VerifyKind::Partial,
            repo: "celestia-island/noa".to_string(),
            head: "0123456789abcdef0123456789abcdef01234567".to_string(),
            tree_clean: true,
            env: VerifyEnv {
                toolchain: "stable 1.82.0".to_string(),
                platform: "x86_64-unknown-linux-gnu".to_string(),
                lock_hash: Some("deadbeef".to_string()),
                offline: true,
            },
            commands: vec![VerifyCommand {
                argv: vec![
                    "cargo".to_string(),
                    "test".to_string(),
                    "--offline".to_string(),
                    "--locked".to_string(),
                ],
                exit_code: Some(0),
                duration_ms: Some(12_345),
                summary: Some("test result: ok. 45 passed; 0 failed; 0 ignored".to_string()),
            }],
            not_run: vec![
                VerifyNotRun {
                    what: "--all-features".to_string(),
                    reason: Some("unused feature graph on this target".to_string()),
                },
                VerifyNotRun {
                    what: "tests/webui".to_string(),
                    reason: None,
                },
            ],
            outputs: vec![VerifyOutput {
                path: "target/noa/debug/noa".to_string(),
                sha256: "cafebabe".to_string(),
            }],
            agent: "verify-impl".to_string(),
            ts: 1_736_000_000_000_000,
        }
    }

    /// Wire format is pinned: `verify` op string and the payload's schema /
    /// kind / negative-record keys must not silently drift.
    #[test]
    fn test_verify_op_wire_format_pinned() {
        assert_eq!(OpType::Verify.as_op_str(), "verify");

        let entry = LogEntry {
            seq: 7,
            op: OpType::Verify,
            path: None,
            blob_id: None,
            from_path: None,
            resolved_conflict_ours_id: None,
            resolved_conflict_theirs_id: None,
            snapshot_id: None,
            remote_seq: None,
            remote_sender: None,
            verify: Some(sample_verify_record()),
            ts: 1_736_000_000_000_000,
            message: Some("verify.run ledger entry".to_string()),
        };
        let json = serialize_entry(&entry).unwrap();
        assert!(json.contains(r#""op":"verify""#), "op wire form: {json}");
        assert!(json.contains(r#""schema":"verify.run/1""#));
        assert!(json.contains(r#""kind":"partial""#));
        assert!(json.contains(r#""not_run":[{"what":"--all-features""#));
        // The *outer* field name is part of the wire contract too: renaming
        // `LogEntry::verify` would silently orphan every payload ever written
        // (review R2: only a hand-written-JSON test caught this before).
        assert!(json.contains(r#""verify":"#), "outer key: {json}");
    }

    /// All three VerifyKind variants are the wire contract (full / partial /
    /// sampled are distinguished by consumers); removing one must fail here
    /// first (review R3-M1: `Sampled` had no pin).
    #[test]
    fn test_verify_kind_wire_forms_pinned() {
        for (kind, wire) in [
            (VerifyKind::Full, r#""kind":"full""#),
            (VerifyKind::Partial, r#""kind":"partial""#),
            (VerifyKind::Sampled, r#""kind":"sampled""#),
        ] {
            let mut record = sample_verify_record();
            record.kind = kind;
            let entry = LogEntry {
                seq: 1,
                op: OpType::Verify,
                path: None,
                blob_id: None,
                from_path: None,
                resolved_conflict_ours_id: None,
                resolved_conflict_theirs_id: None,
                snapshot_id: None,
                remote_seq: None,
                remote_sender: None,
                verify: Some(record),
                ts: 1_736_000_000_000_000,
                message: None,
            };
            let json = serialize_entry(&entry).unwrap();
            assert!(json.contains(wire), "kind wire form: {json}");
        }
    }

    /// An empty not_run must still serialize as `"not_run":[]`: an absent
    /// list reads "coverage unknown", an empty one reads "nothing skipped".
    /// Skipping serialization on empty conflates the two (review R3-M12).
    #[test]
    fn test_empty_not_run_still_serialized() {
        let mut record = sample_verify_record();
        record.not_run = Vec::new();
        let json = serde_json::to_string(&record).unwrap();
        assert!(
            json.contains(r#""not_run":[]"#),
            "empty not_run on wire: {json}"
        );
    }

    /// Old log lines (written before the verify field existed) must keep
    /// parsing, with `verify == None` — new binary reads old lines.
    #[test]
    fn test_old_line_without_verify_field_parses() {
        // Line from the remote_seq era.
        let line = r#"{"seq":1,"op":"write","path":"a.rs","blob_id":"abc","remote_seq":9,"remote_sender":"peer","ts":100}"#;
        let e = deserialize_entry(line).unwrap();
        assert_eq!(e.seq, 1);
        assert_eq!(e.verify, None);

        // Line from before even remote_seq existed.
        let ancient = r#"{"seq":2,"op":"snapshot","snapshot_id":"snap-1","ts":200}"#;
        let e2 = deserialize_entry(ancient).unwrap();
        assert_eq!(e2.seq, 2);
        assert_eq!(e2.verify, None);
    }

    /// New lines carrying a full verify payload survive a
    /// serialize -> deserialize roundtrip without field loss.
    #[test]
    fn test_verify_entry_roundtrip_full_payload() {
        let entry = LogEntry {
            seq: 7,
            op: OpType::Verify,
            path: None,
            blob_id: None,
            from_path: None,
            resolved_conflict_ours_id: None,
            resolved_conflict_theirs_id: None,
            snapshot_id: None,
            remote_seq: None,
            remote_sender: None,
            verify: Some(sample_verify_record()),
            ts: 1_736_000_000_000_000,
            message: None,
        };
        let json = serialize_entry(&entry).unwrap();
        let parsed = deserialize_entry(&json).unwrap();
        assert_eq!(parsed, entry);

        let v = parsed.verify.unwrap();
        assert_eq!(v.kind, VerifyKind::Partial);
        assert_eq!(v.not_run.len(), 2);
        assert_eq!(v.not_run[0].what, "--all-features");
        assert_eq!(
            v.not_run[0].reason.as_deref(),
            Some("unused feature graph on this target")
        );
        assert_eq!(v.not_run[1].reason, None);
        assert_eq!(v.commands[0].exit_code, Some(0));
        assert_eq!(v.commands[0].duration_ms, Some(12_345));
        assert_eq!(
            v.commands[0].argv,
            ["cargo", "test", "--offline", "--locked"]
        );
    }

    /// A verify payload written without its optional collection fields
    /// (commands / not_run / outputs omitted entirely, e.g. by a minimal
    /// writer) must still parse, with those fields defaulting to empty.
    #[test]
    fn test_verify_record_without_optional_collection_fields_parses() {
        let line = r#"{"seq":1,"op":"verify","verify":{"schema":"verify.run/1","kind":"full","repo":"celestia-island/noa","head":"abc","tree_clean":true,"env":{"toolchain":"stable 1.82.0","platform":"x86_64-unknown-linux-gnu"},"agent":"verify-impl","ts":1},"ts":1}"#;
        let e = deserialize_entry(line).unwrap();
        let v = e.verify.unwrap();
        assert!(v.not_run.is_empty());
        assert!(v.commands.is_empty());
        assert!(v.outputs.is_empty());
        assert_eq!(v.kind, VerifyKind::Full);
        // env.offline is serde-defaulted to false when omitted.
        assert!(!v.env.offline);
    }

    /// Old-binary simulation, JSON layer: an old binary reading a new line
    /// sees unknown fields (`verify`) as extra JSON keys it simply drops;
    /// the core seq/op/ts of the line stay readable and the projected line
    /// is still valid JSON. Old binaries do not choke on the new shape.
    #[test]
    fn test_old_binary_value_view_drops_unknown_verify_field() {
        let entry = LogEntry {
            seq: 7,
            op: OpType::Verify,
            path: None,
            blob_id: None,
            from_path: None,
            resolved_conflict_ours_id: None,
            resolved_conflict_theirs_id: None,
            snapshot_id: None,
            remote_seq: None,
            remote_sender: None,
            verify: Some(sample_verify_record()),
            ts: 1_736_000_000_000_000,
            message: None,
        };
        let json = serialize_entry(&entry).unwrap();

        let mut value: serde_json::Value = serde_json::from_str(&json).unwrap();
        let obj = value.as_object_mut().unwrap();
        obj.remove("verify");
        assert_eq!(obj.get("seq").and_then(|v| v.as_u64()), Some(7));
        assert_eq!(obj.get("op").and_then(|v| v.as_str()), Some("verify"));
        assert_eq!(
            obj.get("ts").and_then(|v| v.as_u64()),
            Some(1_736_000_000_000_000)
        );
        let projected = serde_json::to_string(&value).unwrap();
        assert!(serde_json::from_str::<serde_json::Value>(&projected).is_ok());
    }

    // Legacy view of a log entry as compiled before this change: no
    // `verify` field, `OpType` without the Verify variant.
    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    enum LegacyOpType {
        Write,
        Delete,
        Rename,
        Snapshot,
        Merge,
        Resolve,
    }

    #[derive(Debug, Deserialize)]
    struct LegacyLogEntry {
        seq: u64,
        op: LegacyOpType,
        #[serde(default)]
        path: Option<String>,
        ts: u64,
    }

    /// Old-binary simulation, struct layer (the remote_seq precedent): a new
    /// line whose op is a legacy op carries the verify payload as an unknown
    /// field; the legacy struct ignores it and parses seq/op/ts fine.
    #[test]
    fn test_legacy_binary_parses_new_line_with_verify_payload() {
        let entry = LogEntry {
            seq: 3,
            op: OpType::Write,
            path: Some("a.rs".to_string()),
            blob_id: None,
            from_path: None,
            resolved_conflict_ours_id: None,
            resolved_conflict_theirs_id: None,
            snapshot_id: None,
            remote_seq: None,
            remote_sender: None,
            verify: Some(sample_verify_record()),
            ts: 300,
            message: None,
        };
        let json = serialize_entry(&entry).unwrap();
        let legacy: LegacyLogEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(legacy.seq, 3);
        assert_eq!(legacy.op, LegacyOpType::Write);
        assert_eq!(legacy.path.as_deref(), Some("a.rs"));
        assert_eq!(legacy.ts, 300);
    }

    /// Honest limitation, pinned: an old binary's OpType has no Verify
    /// variant, so a verify-op line fails its parse — and the log reader's
    /// existing skip-corrupted-line path (see deserialize_entries) drops the
    /// line with a warning instead of crashing. A mixed file keeps every
    /// legacy-parseable line.
    #[test]
    fn test_legacy_binary_skips_verify_op_line_gracefully() {
        let entry = LogEntry {
            seq: 2,
            op: OpType::Verify,
            path: None,
            blob_id: None,
            from_path: None,
            resolved_conflict_ours_id: None,
            resolved_conflict_theirs_id: None,
            snapshot_id: None,
            remote_seq: None,
            remote_sender: None,
            verify: Some(sample_verify_record()),
            ts: 200,
            message: None,
        };
        let verify_line = serialize_entry(&entry).unwrap();
        // The legacy struct cannot parse the verify op — by design.
        assert!(serde_json::from_str::<LegacyLogEntry>(&verify_line).is_err());

        // deserialize_entries semantics: per-line warn + skip.
        let mixed = format!(
            "{{\"seq\":1,\"op\":\"write\",\"path\":\"a.rs\",\"ts\":100}}\n{verify_line}\n{{\"seq\":3,\"op\":\"delete\",\"path\":\"a.rs\",\"ts\":300}}\n"
        );
        let kept: Vec<LegacyLogEntry> = mixed
            .lines()
            .filter_map(|l| serde_json::from_str::<LegacyLogEntry>(l).ok())
            .collect();
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].seq, 1);
        assert_eq!(kept[1].seq, 3);
    }
}
