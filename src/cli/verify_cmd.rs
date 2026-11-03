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

    // Same gate every other workspace-facing entry point uses (review R2 on
    // #130): without it `--workspace ../escaped` writes outside agent-logs/
    // and a typo silently mints an orphan ledger nobody consumes.
    crate::workspace::validate_workspace_name(&ws)?;
    let ws_mgr = repo.workspace_manager()?;
    if ws_mgr.get(&ws).await?.is_none() {
        anyhow::bail!(
            "workspace {ws:?} does not exist — verify records must land in a real ledger, not an orphan"
        );
    }

    let log = repo.agent_log(&ws)?;
    // append() re-derives the seq under the OS lock and returns the real one;
    // printing the pre-read estimate would lie under concurrency (R2 S3-1).
    let seq = log
        .append(&LogEntry {
            seq: 0, // placeholder; the append impl assigns under lock
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
