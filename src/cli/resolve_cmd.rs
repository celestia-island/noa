use anyhow::Result;

use crate::{
    log::AgentLog,
    merge::{extract_conflicts, ConflictResolution},
    object::{EntryKind, ObjectStore, TreeEntries, TreeEntry, TreeId},
    repo::Repository,
    snapshot::{content_addressed_snapshot_id_with_ts, SnapshotStore},
};

/// Recursively walk the tree hierarchy to resolve a nested path.
fn resolve_nested_entry<'a, O: ObjectStore + 'a>(
    obj_store: &'a O,
    entries: &'a mut [TreeEntry],
    source: &'a TreeEntries,
    path_parts: &'a [&'a str],
    filter: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + 'a>> {
    Box::pin(async move {
        if path_parts.is_empty() {
            return Ok(false);
        }
        let first = path_parts[0];
        let rest = &path_parts[1..];
        let is_leaf = rest.is_empty();

        for target_entry in entries.iter_mut() {
            if target_entry.name != first {
                continue;
            }
            if is_leaf {
                if !target_entry.kind.is_file_like() {
                    anyhow::bail!("'{}' is not a file", filter);
                }
                let source_entry = source
                    .0
                    .iter()
                    .find(|e| e.name == first)
                    .ok_or_else(|| anyhow::anyhow!("path '{}' not found in source tree", filter))?;
                target_entry.id = source_entry.id.clone();
                target_entry.kind = source_entry.kind;
                return Ok(true);
            }
            if target_entry.kind != EntryKind::Tree {
                anyhow::bail!("'{}' is not a directory", first);
            }
            let source_sub = obj_store
                .get_tree(&TreeId(
                    source
                        .0
                        .iter()
                        .find(|e| e.name == first)
                        .map(|e| e.id.clone())
                        .unwrap_or_default(),
                ))
                .await?;
            let mut target_sub = obj_store.get_tree(&TreeId(target_entry.id.clone())).await?;
            let found =
                resolve_nested_entry(obj_store, &mut target_sub.0, &source_sub, rest, filter)
                    .await?;
            if found {
                let new_sub_id = obj_store.put_tree(&target_sub).await?;
                target_entry.id = new_sub_id.0;
                return Ok(true);
            }
            return Ok(false);
        }
        anyhow::bail!("path component '{}' not found in merge tree", first);
    })
}

pub async fn run_resolve(
    repo: &Repository,
    strategy: &str,
    path_filter: Option<&str>,
) -> Result<()> {
    let resolution = match strategy {
        "ours" => ConflictResolution::Ours,
        "theirs" => ConflictResolution::Theirs,
        _ => anyhow::bail!("unknown strategy '{strategy}', expected 'ours' or 'theirs'"),
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

    let latest_merge = merge_entries.last().ok_or_else(|| {
        anyhow::anyhow!("internal error: merge_entries unexpectedly empty after non-empty check")
    })?;

    let snap_store = repo.snapshot_store()?;
    let obj_store = repo.object_store()?;
    let ws_mgr = repo.workspace_manager()?;

    let current_ws = ws_mgr
        .get(&current)
        .await?
        .ok_or_else(|| anyhow::anyhow!("workspace '{current}' not found"))?;

    let merge_snap_id = latest_merge
        .snapshot_id
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("merge entry has no snapshot id"))?;
    let merge_snap = snap_store
        .get(&crate::snapshot::SnapshotId(merge_snap_id.to_string()))
        .await?;

    let merge_tree = obj_store
        .get_tree(&crate::object::TreeId(merge_snap.tree_hash.clone()))
        .await?;

    let mut resolved_entries = merge_tree.0.clone();

    if let Some(filter) = path_filter {
        let filter_parts: Vec<&str> = filter.split('/').collect();

        if filter_parts.len() > 1 {
            // Nested path: walk tree hierarchy to find and resolve the entry
            let source_tree = if merge_snap.parents.is_empty() {
                crate::object::TreeEntries(vec![])
            } else {
                let pid = match resolution {
                    ConflictResolution::Ours => merge_snap.parents.first(),
                    ConflictResolution::Theirs => merge_snap.parents.get(1),
                };
                match pid {
                    Some(pid) => {
                        let source_snap = snap_store.get(pid).await?;
                        obj_store
                            .get_tree(&crate::object::TreeId(source_snap.tree_hash.clone()))
                            .await?
                    }
                    None => crate::object::TreeEntries(vec![]),
                }
            };

            resolve_nested_entry(
                &obj_store,
                &mut resolved_entries,
                &source_tree,
                &filter_parts,
                filter,
            )
            .await?;
        } else {
            // Flat path: use existing three-way merge approach
            let parent_snap_id = merge_snap.parents.first();
            let parent_tree = if let Some(pid) = parent_snap_id {
                let parent_snap = snap_store.get(pid).await?;
                obj_store
                    .get_tree(&crate::object::TreeId(parent_snap.tree_hash.clone()))
                    .await?
            } else {
                crate::object::TreeEntries(vec![])
            };

            let theirs_snap_id = merge_snap.parents.get(1);
            let theirs_tree = if let Some(tid) = theirs_snap_id {
                let their_snap = snap_store.get(tid).await?;
                obj_store
                    .get_tree(&crate::object::TreeId(their_snap.tree_hash.clone()))
                    .await?
            } else {
                parent_tree.clone()
            };

            let result = crate::merge::three_way_merge(&parent_tree, &merge_tree, &theirs_tree)?;
            let merge_conflicts = extract_conflicts(&result.output);

            let target_conflict = merge_conflicts
                .iter()
                .find(|c| c.path == filter)
                .ok_or_else(|| anyhow::anyhow!("no conflict found for path '{filter}'"))?;

            let resolved_blob_id = match resolution {
                ConflictResolution::Ours => target_conflict.ours_id.as_deref(),
                ConflictResolution::Theirs => target_conflict.theirs_id.as_deref(),
            }
            .ok_or_else(|| anyhow::anyhow!("no blob id available for resolution"))?;

            for entry in &mut resolved_entries {
                if entry.name == filter {
                    entry.id = resolved_blob_id.to_string();
                }
            }
        }
    } else if merge_snap.parents.len() >= 2 {
        let base_snap = snap_store.get(&current_ws.base).await?;
        let base_tree = obj_store
            .get_tree(&crate::object::TreeId(base_snap.tree_hash.clone()))
            .await?;
        let theirs_snap = snap_store
            .get(
                merge_snap
                    .parents
                    .get(1)
                    .ok_or_else(|| anyhow::anyhow!("merge snapshot has fewer than 2 parents"))?,
            )
            .await?;
        let theirs_tree = obj_store
            .get_tree(&crate::object::TreeId(theirs_snap.tree_hash.clone()))
            .await?;

        let result = crate::merge::three_way_merge(&base_tree, &merge_tree, &theirs_tree)?;
        resolved_entries = result.into_tree_entries(&resolution).0;
    }

    let resolved_tree = crate::object::TreeEntries(resolved_entries);
    let new_tree_id = obj_store.put_tree(&resolved_tree).await?;

    let ours_id = latest_merge
        .resolved_conflict_ours_id
        .as_deref()
        .unwrap_or("none");
    let theirs_id = latest_merge
        .resolved_conflict_theirs_id
        .as_deref()
        .unwrap_or("none");

    let now = crate::now_micros();
    let resolved_path = path_filter
        .map(std::string::ToString::to_string)
        .or_else(|| latest_merge.path.clone())
        .unwrap_or_default();

    let author = "noa-resolve".to_string();
    let message = format!("resolve conflicts with strategy '{strategy}'");
    let new_snap_id = content_addressed_snapshot_id_with_ts(
        &new_tree_id.0,
        std::slice::from_ref(&merge_snap.id),
        &current,
        &author,
        &message,
        now,
    );

    let resolved_snapshot = crate::snapshot::Snapshot {
        id: new_snap_id.clone(),
        tree_hash: new_tree_id.0,
        parents: vec![merge_snap.id.clone()],
        workspace: current.clone(),
        author,
        timestamp: now,
        message,
    };
    snap_store.store(&resolved_snapshot).await?;
    ws_mgr.update_head(&current, &resolved_snapshot.id).await?;

    log.append(&crate::log::LogEntry {
        seq: 0,
        op: crate::log::OpType::Resolve,
        path: Some(resolved_path.clone()),
        blob_id: None,
        from_path: None,
        resolved_conflict_ours_id: Some(ours_id.to_string()),
        resolved_conflict_theirs_id: Some(theirs_id.to_string()),
        snapshot_id: Some(new_snap_id.0),
        ts: now,
        message: Some(format!("resolve {resolved_path} with {strategy}")),
    })
    .await?;

    println!(
        "Resolved conflict on '{resolved_path}' with strategy '{strategy}' (ours={ours_id}, theirs={theirs_id})"
    );
    println!("Created resolution snapshot: {}", resolved_snapshot.id);
    println!("Run 'noa snapshot create' to commit additional changes.");
    Ok(())
}
