use anyhow::{Context, Result};

use crate::log::{AgentLog, LogEntry, OpType, VerifyRecord};
use crate::repo::Repository;

/// Append a verify.run/1 record to the current workspace's agent log.
///
/// The record JSON arrives on stdin or `--file` (the Python ledger writer
/// `_tools/verify_record.py` produces exactly this shape). The command is a
/// thin transport: it parses strictly (a malformed record must fail loudly,
/// not land half-read in the ledger), assigns the next seq, and appends one
/// `OpType::Verify` entry. It prints the assigned seq so callers can chain.
pub async fn run_append(file: Option<&str>, workspace: Option<&str>) -> Result<()> {
    let raw = match file {
        Some(path) => std::fs::read_to_string(path)
            .with_context(|| format!("read verify record from {path}"))?,
        None => {
            let mut buf = String::new();
            std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
            buf
        }
    };
    let record: VerifyRecord = serde_json::from_str(&raw).with_context(|| {
        "verify record must be a verify.run/1 payload (Rust side is the wire authority)"
    })?;

    let root = Repository::find(std::path::Path::new("."))?;
    let repo = Repository::open(&root)?;
    let ws = workspace
        .map(std::string::ToString::to_string)
        .or_else(|| repo.read_head().ok())
        .context("no workspace: pass --workspace or set a head first")?;

    let log = repo.agent_log(&ws)?;
    let seq = log.next_seq().await?;
    log.append(&LogEntry {
        seq,
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
        ts: crate::now_micros(),
        message: Some("verify.run/1 record".to_string()),
    })
    .await?;

    println!("appended verify entry seq={seq} workspace={ws}");
    Ok(())
}
