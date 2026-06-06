use anyhow::Result;

use crate::{
    log::AgentLog,
    merge::{extract_conflicts, ConflictResolution},
    object::ObjectStore,
    repo::Repository,
    snapshot::{content_addressed_snapshot_id, SnapshotStore},
};

pub async fn run_create(repo: &Repository, name: &str, agent: Option<&str>) -> Result<()> {
    let ws_mgr = repo.workspace_manager()?;

    let base_snapshot = match ws_mgr.get(&repo.read_head()?).await? {
        Some(ws) => ws.head.clone(),
        None => crate::snapshot::SnapshotId("noa_empty".to_string()),
    };

    let now = chrono::Utc::now().timestamp_micros() as u64;
    let ws = crate::workspace::Workspace {
        name: name.to_string(),
        head: base_snapshot.clone(),
        base: base_snapshot.clone(),
        agent_id: agent.map(|s| s.to_string()),
        created_at: now,
        updated_at: now,
    };
    ws_mgr.create(&ws).await?;

    let log = repo.agent_log(name)?;
    log.append(&crate::log::LogEntry {
        seq: 1,
        op: crate::log::OpType::Snapshot,
        path: None,
        blob_id: None,
        from_path: None,
        resolved_conflict_ours_id: None,
        resolved_conflict_theirs_id: None,
        snapshot_id: Some(base_snapshot.0.clone()),
        ts: now,
        message: Some(format!("workspace {} created", name)),
    })
    .await?;

    println!("Created workspace '{}' (base: {})", name, base_snapshot);
    Ok(())
}

pub async fn run_switch(repo: &Repository, name: &str) -> Result<()> {
    let ws_mgr = repo.workspace_manager()?;
    ws_mgr
        .get(name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("workspace '{}' not found", name))?;

    let prev = repo.read_head()?;
    repo.write_orig_head(&prev)?;
    repo.write_head(name)?;

    println!("Switched to workspace '{}'", name);
    Ok(())
}

pub async fn run_list(repo: &Repository) -> Result<()> {
    let ws_mgr = repo.workspace_manager()?;
    let list = ws_mgr.list().await?;
    let current = repo.read_head()?;

    if list.is_empty() {
        println!("No workspaces found.");
        return Ok(());
    }

    for ws in &list {
        let marker = if ws.name == current { "*" } else { " " };
        println!(
            "{} {:<20} head: {} base: {}",
            marker, ws.name, ws.head, ws.base
        );
    }
    Ok(())
}

pub async fn run_delete(repo: &Repository, name: &str) -> Result<()> {
    let current = repo.read_head()?;
    if name == current {
        anyhow::bail!("cannot delete the active workspace '{}'", name);
    }

    let ws_mgr = repo.workspace_manager()?;
    ws_mgr.delete(name).await?;

    println!("Deleted workspace '{}'", name);
    Ok(())
}

pub async fn run_merge(repo: &Repository, from: &str) -> Result<()> {
    let ws_mgr = repo.workspace_manager()?;
    let current = repo.read_head()?;

    let from_ws = ws_mgr
        .get(from)
        .await?
        .ok_or_else(|| anyhow::anyhow!("workspace '{}' not found", from))?;
    let cur_ws = ws_mgr
        .get(&current)
        .await?
        .ok_or_else(|| anyhow::anyhow!("workspace '{}' not found", current))?;

    let snap_store = repo.snapshot_store()?;
    let obj_store = repo.object_store()?;

    let base_snap = snap_store.get(&cur_ws.head).await?;
    let their_snap = snap_store.get(&from_ws.head).await?;

    let base_tree = obj_store
        .get_tree(&crate::object::TreeId(base_snap.tree_hash))
        .await?;
    let theirs_tree = obj_store
        .get_tree(&crate::object::TreeId(their_snap.tree_hash))
        .await?;

    let result = crate::merge::three_way_merge(&base_tree, &base_tree, &theirs_tree)?;

    let conflicts = extract_conflicts(&result.output);
    if !conflicts.is_empty() {
        println!("Conflicts detected:");
        for c in &conflicts {
            println!("  CONFLICT: {}", c.path);
        }
    }

    let resolved_tree = result.into_tree_entries(&ConflictResolution::Ours);

    let new_tree_id = obj_store.put_tree(&resolved_tree).await?;

    let merge_snapshot = crate::snapshot::Snapshot {
        id: content_addressed_snapshot_id(
            &new_tree_id.0,
            &[cur_ws.head.clone(), from_ws.head.clone()],
            &current,
        ),
        tree_hash: new_tree_id.0,
        parents: vec![cur_ws.head.clone(), from_ws.head.clone()],
        workspace: current.clone(),
        author: "noa".to_string(),
        timestamp: chrono::Utc::now().timestamp_micros() as u64,
        message: format!("merge {} into {}", from, current),
    };

    snap_store.store(&merge_snapshot).await?;
    ws_mgr.update_head(&current, &merge_snapshot.id).await?;

    println!("Merged {} into {} -> {}", from, current, merge_snapshot.id);
    Ok(())
}
