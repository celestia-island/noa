use anyhow::Result;

use crate::{log::AgentLog, merge::ConflictResolution, repo::Repository};

pub async fn run_resolve(
    repo: &Repository,
    strategy: &str,
    path_filter: Option<&str>,
) -> Result<()> {
    let _resolution = match strategy {
        "ours" => ConflictResolution::Ours,
        "theirs" => ConflictResolution::Theirs,
        _ => anyhow::bail!(
            "unknown strategy '{}', expected 'ours' or 'theirs'",
            strategy
        ),
    };

    let current = repo.read_head()?;
    let log = repo.agent_log(&current)?;

    let entries = log.read_all().await?;
    let merge_entries: Vec<_> = entries
        .iter()
        .filter(|e| e.op == crate::log::OpType::Merge)
        .collect();

    if merge_entries.is_empty() {
        println!("No pending conflicts found.");
        return Ok(());
    }

    let latest_merge = merge_entries.last().unwrap();
    let ours_id = latest_merge
        .resolved_conflict_ours_id
        .as_deref()
        .unwrap_or("?");
    let theirs_id = latest_merge
        .resolved_conflict_theirs_id
        .as_deref()
        .unwrap_or("?");

    let now = chrono::Utc::now().timestamp_micros() as u64;
    let resolved_path = path_filter
        .map(|p| p.to_string())
        .or_else(|| latest_merge.path.clone())
        .unwrap_or_default();

    log.append(&crate::log::LogEntry {
        seq: 0,
        op: crate::log::OpType::Resolve,
        path: Some(resolved_path.clone()),
        blob_id: None,
        from_path: None,
        resolved_conflict_ours_id: Some(ours_id.to_string()),
        resolved_conflict_theirs_id: Some(theirs_id.to_string()),
        snapshot_id: None,
        ts: now,
        message: Some(format!("resolve {} with {}", resolved_path, strategy)),
    })
    .await?;

    println!(
        "Resolved conflict on '{}' with strategy '{}' (ours={}, theirs={})",
        resolved_path, strategy, ours_id, theirs_id
    );
    println!("Run 'noa snapshot create' to commit the resolution.");
    Ok(())
}
