use anyhow::Result;

use crate::{
    log::AgentLog,
    merge::{extract_conflicts, ConflictResolution},
    object::ObjectStore,
    repo::Repository,
    snapshot::{content_addressed_snapshot_id_with_ts, SnapshotStore},
};

pub async fn run_create(repo: &Repository, name: &str, agent: Option<&str>) -> Result<()> {
    let ws_mgr = repo.workspace_manager()?;

    let base_snapshot = match ws_mgr.get(&repo.read_head()?).await? {
        Some(ws) => ws.head.clone(),
        None => crate::snapshot::empty_snapshot_id(),
    };

    let now = crate::now_micros();
    let ws = crate::workspace::Workspace {
        name: name.to_string(),
        head: base_snapshot.clone(),
        base: base_snapshot.clone(),
        agent_id: agent.map(std::string::ToString::to_string),
        last_seq: 0,
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
        message: Some(format!("workspace {name} created")),
    })
    .await?;

    println!("Created workspace '{name}' (base: {base_snapshot})");
    Ok(())
}

pub async fn run_switch(repo: &Repository, name: &str) -> Result<()> {
    let ws_mgr = repo.workspace_manager()?;
    ws_mgr
        .get(name)
        .await?
        .ok_or_else(|| anyhow::anyhow!("workspace '{name}' not found"))?;

    let prev = repo.read_head()?;
    repo.write_orig_head(&prev)?;
    repo.write_head(name)?;

    println!("Switched to workspace '{name}'");
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
        anyhow::bail!("cannot delete the active workspace '{name}'");
    }

    let ws_mgr = repo.workspace_manager()?;
    let existed = ws_mgr.delete(name).await?;
    if !existed {
        anyhow::bail!("workspace '{name}' not found");
    }

    println!("Deleted workspace '{name}'");
    Ok(())
}

pub async fn run_merge(repo: &Repository, from: &str, strategy: &str) -> Result<()> {
    let resolution = match strategy {
        "ours" => ConflictResolution::Ours,
        "theirs" => ConflictResolution::Theirs,
        _ => anyhow::bail!("unknown strategy '{strategy}', expected 'ours' or 'theirs'"),
    };

    let ws_mgr = repo.workspace_manager()?;
    let current = repo.read_head()?;

    let from_ws = ws_mgr
        .get(from)
        .await?
        .ok_or_else(|| anyhow::anyhow!("workspace '{from}' not found"))?;
    let cur_ws = ws_mgr
        .get(&current)
        .await?
        .ok_or_else(|| anyhow::anyhow!("workspace '{current}' not found"))?;

    let snap_store = repo.snapshot_store()?;
    let obj_store = repo.object_store()?;

    let empty_tree = crate::object::TreeEntries(vec![]);

    // The merge base is the DAG common ancestor of the two heads, never the
    // target workspace's mutable `base` field (stale once a merge from any
    // other branch lands).
    let merge_base_id =
        crate::snapshot::find_merge_base(&snap_store, &cur_ws.head, &from_ws.head).await?;
    let (base_tree, used_base_id) = match &merge_base_id {
        Some(id) => {
            let base_snap = snap_store.get(id).await?;
            let tree = obj_store
                .get_tree(&crate::object::TreeId(base_snap.tree_hash))
                .await?;
            (tree, id.clone())
        }
        // Unrelated histories: merge against the empty tree, as with git.
        None => (empty_tree.clone(), crate::snapshot::empty_snapshot_id()),
    };
    let ours_tree = if cur_ws.head.is_empty() {
        empty_tree.clone()
    } else {
        let ours_snap = snap_store.get(&cur_ws.head).await?;
        obj_store
            .get_tree(&crate::object::TreeId(ours_snap.tree_hash))
            .await?
    };
    let theirs_tree = if from_ws.head.is_empty() {
        empty_tree
    } else {
        let their_snap = snap_store.get(&from_ws.head).await?;
        obj_store
            .get_tree(&crate::object::TreeId(their_snap.tree_hash))
            .await?
    };

    let result = crate::merge::merge_trees_recursive(
        base_tree,
        ours_tree,
        theirs_tree,
        obj_store.clone(),
        &resolution,
    )
    .await?;

    let conflicts = extract_conflicts(&result.output);
    if !conflicts.is_empty() {
        println!("Conflicts detected:");
        for c in &conflicts {
            println!("  CONFLICT: {}", c.path);
        }
        println!(
            "{} conflict(s) found. Resolving with --strategy={}.",
            conflicts.len(),
            strategy
        );
    }

    let resolved_tree = result.into_tree_entries(&resolution);

    let new_tree_id = obj_store.put_tree(&resolved_tree).await?;

    let author = "noa".to_string();
    let message = format!("merge {from} into {current}");
    let now = crate::now_micros();
    let merge_snapshot = crate::snapshot::Snapshot {
        id: content_addressed_snapshot_id_with_ts(
            &new_tree_id.0,
            &[cur_ws.head.clone(), from_ws.head.clone()],
            &current,
            &author,
            &message,
            now,
        ),
        tree_hash: new_tree_id.0,
        parents: vec![cur_ws.head.clone(), from_ws.head.clone()],
        workspace: current.clone(),
        author,
        timestamp: now,
        message,
    };

    snap_store.store(&merge_snapshot).await?;

    let log = repo.agent_log(&current)?;
    let now = crate::now_micros();
    let new_seq = log
        .append(&crate::log::LogEntry {
            seq: 0,
            op: crate::log::OpType::Merge,
            path: None,
            blob_id: None,
            from_path: None,
            resolved_conflict_ours_id: if conflicts.is_empty() {
                None
            } else {
                Some(
                    conflicts
                        .iter()
                        .filter_map(|c| c.ours_id.as_deref())
                        .collect::<Vec<_>>()
                        .join(","),
                )
            },
            resolved_conflict_theirs_id: if conflicts.is_empty() {
                None
            } else {
                Some(
                    conflicts
                        .iter()
                        .filter_map(|c| c.theirs_id.as_deref())
                        .collect::<Vec<_>>()
                        .join(","),
                )
            },
            snapshot_id: Some(merge_snapshot.id.0.clone()),
            ts: now,
            message: Some(format!("merge {from} into {current}")),
        })
        .await?;

    // Record the base actually used for this merge (the DAG merge-base), not
    // the source head: the next merge from another branch must see the same
    // ancestor, not a tree that branch never saw.
    ws_mgr
        .update_head_seq_and_base(&current, &merge_snapshot.id, new_seq, &used_base_id)
        .await?;

    if conflicts.is_empty() {
        println!("Merged {} into {} -> {}", from, current, merge_snapshot.id);
    } else {
        println!(
            "Merged {} into {} -> {} ({} conflict(s) auto-resolved with {})",
            from,
            current,
            merge_snapshot.id,
            conflicts.len(),
            strategy
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::merge::ConflictResolution;
    use crate::object::{EntryKind, ObjectStore, TreeEntries, TreeEntry};
    use crate::snapshot::{Snapshot, SnapshotId, SnapshotStore};

    async fn put_tree_with(
        obj_store: &crate::object::RedbObjectStore,
        files: &[(&str, &str)],
    ) -> TreeEntries {
        let mut entries = Vec::new();
        for (name, content) in files {
            let blob = obj_store.put_blob(content.as_bytes()).await.unwrap();
            entries.push(TreeEntry {
                name: (*name).to_string(),
                kind: EntryKind::Blob,
                id: blob.0,
            });
        }
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        TreeEntries(entries)
    }

    async fn put_snapshot_with(
        snap_store: &crate::snapshot::RedbSnapshotStore,
        obj_store: &crate::object::RedbObjectStore,
        workspace: &str,
        files: &[(&str, &str)],
        parents: Vec<SnapshotId>,
    ) -> SnapshotId {
        let entries = put_tree_with(obj_store, files).await;
        let tree_id = obj_store.put_tree(&entries).await.unwrap();
        let now = crate::now_micros();
        let id = content_addressed_snapshot_id_with_ts(
            &tree_id.0,
            &parents,
            workspace,
            "test",
            "test snapshot",
            now,
        );
        snap_store
            .store(&Snapshot {
                id: id.clone(),
                tree_hash: tree_id.0,
                parents,
                workspace: workspace.to_string(),
                author: "test".to_string(),
                timestamp: now,
                message: "test".to_string(),
            })
            .await
            .unwrap();
        id
    }

    fn sorted_names(entries: &TreeEntries) -> Vec<String> {
        let mut names: Vec<String> =
            entries.0.iter().map(|e| e.name.clone()).collect();
        names.sort();
        names
    }

    async fn blob_content(
        obj_store: &crate::object::RedbObjectStore,
        tree: &TreeEntries,
        name: &str,
    ) -> String {
        let entry = tree.0.iter().find(|e| e.name == name).unwrap_or_else(|| {
            panic!("file '{name}' missing from tree {:?}", sorted_names(tree))
        });
        String::from_utf8(
            obj_store
                .get_blob(&crate::object::BlobId(entry.id.clone()))
                .await
                .unwrap(),
        )
        .unwrap()
    }

    /// Regression test for issue #69: merging C-into-B and then A-into-B must
    /// keep `c.txt`. `run_merge` used to read the base from the mutable
    /// `Workspace.base` field (overwritten with the source head after each
    /// merge), so the second merge ran against a tree branch A never saw and
    /// deleted the already-merged `c.txt`. The base must be the DAG common
    /// ancestor of the two heads, and the recorded base must be the ancestor
    /// actually used.
    #[tokio::test]
    async fn test_second_merge_from_other_branch_keeps_merged_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let repo = crate::repo::Repository::init(tmp.path()).unwrap();
        let snap_store = repo.snapshot_store().unwrap();
        let obj_store = repo.object_store().unwrap();
        let ws_mgr = repo.workspace_manager().unwrap();

        // Base snapshot X, forked by A1/B1/C1 (all parented on X).
        let x = put_snapshot_with(
            &snap_store,
            &obj_store,
            "B",
            &[("shared.txt", "base")],
            vec![],
        )
        .await;
        let a1 = put_snapshot_with(
            &snap_store,
            &obj_store,
            "A",
            &[("a.txt", "A"), ("shared.txt", "A")],
            vec![x.clone()],
        )
        .await;
        let b1 = put_snapshot_with(
            &snap_store,
            &obj_store,
            "B",
            &[("b.txt", "B"), ("shared.txt", "B")],
            vec![x.clone()],
        )
        .await;
        let c1 = put_snapshot_with(
            &snap_store,
            &obj_store,
            "C",
            &[("c.txt", "C"), ("shared.txt", "C")],
            vec![x.clone()],
        )
        .await;

        for (name, head) in [("A", &a1), ("B", &b1), ("C", &c1)] {
            ws_mgr
                .create(&crate::workspace::Workspace {
                    name: name.to_string(),
                    head: head.clone(),
                    base: x.clone(),
                    agent_id: None,
                    last_seq: 0,
                    created_at: 0,
                    updated_at: 0,
                })
                .await
                .unwrap();
        }
        repo.write_head("B").unwrap();

        // Merge 1: C into B.
        run_merge(&repo, "C", "ours").await.unwrap();
        let b_ws = ws_mgr.get("B").await.unwrap().unwrap();
        // The recorded base is the ancestor actually used (X), not C1.
        assert_eq!(b_ws.base, x, "B.base must be the DAG merge-base X");
        let m1 = b_ws.head.clone();
        assert_ne!(m1, c1);

        // Extra change on A, parented on A1.
        let a2 = put_snapshot_with(
            &snap_store,
            &obj_store,
            "A",
            &[("a.txt", "A"), ("a2.txt", "A2-new"), ("shared.txt", "A")],
            vec![a1.clone()],
        )
        .await;
        ws_mgr.update_head("A", &a2).await.unwrap();

        // Merge 2: A into B. c.txt must survive.
        run_merge(&repo, "A", "ours").await.unwrap();
        let b_ws = ws_mgr.get("B").await.unwrap().unwrap();
        assert_eq!(b_ws.base, x, "B.base must still be the DAG merge-base X");
        let m2 = snap_store.get(&b_ws.head).await.unwrap();
        assert_eq!(m2.parents, vec![m1.clone(), a2.clone()]);
        let m2_tree = obj_store
            .get_tree(&crate::object::TreeId(m2.tree_hash.clone()))
            .await
            .unwrap();
        assert_eq!(
            sorted_names(&m2_tree),
            vec!["a.txt", "a2.txt", "b.txt", "c.txt", "shared.txt"]
        );
        assert_eq!(blob_content(&obj_store, &m2_tree, "c.txt").await, "C");
        assert_eq!(blob_content(&obj_store, &m2_tree, "a2.txt").await, "A2-new");
        assert_eq!(blob_content(&obj_store, &m2_tree, "shared.txt").await, "B");

        // M2 equals the reference merge computed on the DAG ancestor X.
        let x_tree = obj_store
            .get_tree(&crate::object::TreeId(
                snap_store.get(&x).await.unwrap().tree_hash,
            ))
            .await
            .unwrap();
        let m1_tree = obj_store
            .get_tree(&crate::object::TreeId(
                snap_store.get(&m1).await.unwrap().tree_hash,
            ))
            .await
            .unwrap();
        let a2_tree = obj_store
            .get_tree(&crate::object::TreeId(
                snap_store.get(&a2).await.unwrap().tree_hash,
            ))
            .await
            .unwrap();
        let expected = crate::merge::three_way_merge(&x_tree, &m1_tree, &a2_tree)
            .unwrap()
            .into_tree_entries(&ConflictResolution::Ours);
        let mut got = m2_tree.0.clone();
        got.sort_by(|a, b| a.name.cmp(&b.name));
        let mut want = expected.0.clone();
        want.sort_by(|a, b| a.name.cmp(&b.name));
        assert_eq!(got, want);
    }
}
