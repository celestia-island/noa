use anyhow::Result;

use crate::log::AgentLog;
use crate::object::ObjectStore;
use crate::refs::RefStore;
use crate::repo::Repository;
use crate::snapshot::{SnapshotEngine, SnapshotId, SnapshotStore};
use crate::workspace::WorkspaceManager;

pub async fn run_create(repo: &Repository, message: &str, author: &str) -> Result<()> {
    let head_ws = repo.read_head()?;
    let ws_mgr = repo.workspace_manager()?;
    let snap_store = repo.snapshot_store()?;
    let obj_store = repo.object_store()?;
    let agent_log = repo.agent_log(&head_ws)?;

    let parent_ids = match ws_mgr.get(&head_ws).await? {
        Some(ws) => vec![ws.head],
        None => vec![],
    };

    let engine = SnapshotEngine::new(agent_log, snap_store, obj_store);
    let snapshot = engine.compute(&head_ws, parent_ids, author, message).await?;

    ws_mgr.update_head(&head_ws, &snapshot.id).await?;

    let ref_store = repo.ref_store()?;
    ref_store.cas(&head_ws, None, &snapshot.id).await.ok();

    println!("Created snapshot {} in workspace '{}'", snapshot.id, head_ws);
    Ok(())
}

pub async fn run_list(repo: &Repository) -> Result<()> {
    let snap_store = repo.snapshot_store()?;
    let all = snap_store.list_all().await?;

    if all.is_empty() {
        println!("No snapshots found.");
        return Ok(());
    }

    println!("{:<16} {:<12} {:<16} {:<40}", "ID", "WORKSPACE", "AUTHOR", "MESSAGE");
    for snap in all {
        let msg = if snap.message.len() > 40 {
            format!("{}...", &snap.message[..37])
        } else {
            snap.message
        };
        println!("{:<16} {:<12} {:<16} {:<40}", snap.id, snap.workspace, snap.author, msg);
    }
    Ok(())
}

pub async fn run_diff(repo: &Repository, a: &str, b: &str) -> Result<()> {
    let snap_store = repo.snapshot_store()?;
    let obj_store = repo.object_store()?;

    let snap_a = snap_store.get(&SnapshotId(a.to_string())).await
        .map_err(|_| anyhow::anyhow!("snapshot {} not found", a))?;
    let snap_b = snap_store.get(&SnapshotId(b.to_string())).await
        .map_err(|_| anyhow::anyhow!("snapshot {} not found", b))?;

    let tree_a = obj_store.get_tree(&crate::object::TreeId(snap_a.tree_hash)).await?;
    let tree_b = obj_store.get_tree(&crate::object::TreeId(snap_b.tree_hash)).await?;

    let diffs = crate::snapshot::diff_snapshots(&tree_a.0, &tree_b.0);

    if diffs.is_empty() {
        println!("No differences between {} and {}", a, b);
        return Ok(());
    }

    for diff in &diffs {
        let kind = match &diff.kind {
            crate::snapshot::DiffKind::Added => "added",
            crate::snapshot::DiffKind::Modified => "modified",
            crate::snapshot::DiffKind::Deleted => "deleted",
            crate::snapshot::DiffKind::Renamed { .. } => "renamed",
        };
        println!("  {:<10} {}", kind, diff.path);
    }
    Ok(())
}
