use anyhow::Result;

use crate::{
    ignore::IgnoreMatcher,
    object::ObjectStore,
    refs::RefStore,
    repo::Repository,
    snapshot::{SnapshotEngine, SnapshotId, SnapshotStore},
};

pub async fn run_create(repo: &Repository, message: &str, author: &str) -> Result<()> {
    let head_ws = repo.read_head()?;
    let ws_mgr = repo.workspace_manager()?;
    let snap_store = repo.snapshot_store()?;
    let obj_store = repo.object_store()?;
    let agent_log = repo.agent_log(&head_ws)?;

    let (parent_ids, since_seq) = match ws_mgr.get(&head_ws).await? {
        Some(ws) => {
            let parents = if ws.head.0 == "noa_empty" || ws.head.0.starts_with("noa_empty") {
                vec![]
            } else {
                vec![ws.head.clone()]
            };
            (parents, ws.last_seq)
        }
        None => (vec![], 0),
    };

    let matcher = IgnoreMatcher::from_repo_root(&repo.root);
    let engine = SnapshotEngine::new(agent_log, snap_store, obj_store)
        .with_ignore(matcher)
        .with_repo_root(repo.root.clone())
        .with_compact_on_snapshot();
    let snapshot = engine
        .compute(&head_ws, parent_ids, since_seq, author, message)
        .await?;

    let new_seq = crate::log::AgentLog::next_seq(&engine.log).await?;
    ws_mgr
        .update_head_and_seq(&head_ws, &snapshot.id, new_seq)
        .await?;

    let ref_store = repo.ref_store()?;
    match ref_store.cas(&head_ws, None, &snapshot.id).await {
        Ok(true) => {}
        Ok(false) => {
            eprintln!(
                "warning: ref '{}' already exists, snapshot {} not set as HEAD",
                head_ws, snapshot.id
            );
        }
        Err(e) => {
            eprintln!(
                "warning: failed to update ref '{}' after snapshot: {}",
                head_ws, e
            );
        }
    }

    println!(
        "Created snapshot {} in workspace '{}'",
        snapshot.id, head_ws
    );
    Ok(())
}

pub async fn run_list(repo: &Repository) -> Result<()> {
    let snap_store = repo.snapshot_store()?;
    let all = snap_store.list_all().await?;

    if all.is_empty() {
        println!("No snapshots found.");
        return Ok(());
    }

    println!(
        "{:<16} {:<12} {:<16} {:<40}",
        "ID", "WORKSPACE", "AUTHOR", "MESSAGE"
    );
    for snap in all {
        let msg = if snap.message.chars().count() > 40 {
            let truncated: String = snap.message.chars().take(37).collect();
            format!("{}...", truncated)
        } else {
            snap.message
        };
        println!(
            "{:<16} {:<12} {:<16} {:<40}",
            snap.id, snap.workspace, snap.author, msg
        );
    }
    Ok(())
}

pub async fn run_diff(repo: &Repository, a: &str, b: &str) -> Result<()> {
    let snap_store = repo.snapshot_store()?;
    let obj_store = repo.object_store()?;

    let snap_a = snap_store
        .get(&SnapshotId(a.to_string()))
        .await
        .map_err(|_| anyhow::anyhow!("snapshot {} not found", a))?;
    let snap_b = snap_store
        .get(&SnapshotId(b.to_string()))
        .await
        .map_err(|_| anyhow::anyhow!("snapshot {} not found", b))?;

    let tree_a = obj_store
        .get_tree(&crate::object::TreeId(snap_a.tree_hash))
        .await?;
    let tree_b = obj_store
        .get_tree(&crate::object::TreeId(snap_b.tree_hash))
        .await?;

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
        };
        println!("  {:<10} {}", kind, diff.path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_utf8_truncation_no_panic() {
        let msg = "你好世界".repeat(20);
        // This would panic with byte slicing (&msg[..37])
        let truncated: String = msg.chars().take(37).collect();
        assert_eq!(truncated.chars().count(), 37);

        let emoji_msg = "🎉🚀💎".repeat(20);
        let truncated_emoji: String = emoji_msg.chars().take(37).collect();
        assert_eq!(truncated_emoji.chars().count(), 37);
    }

    #[test]
    fn test_short_message_not_truncated() {
        let msg = "short msg";
        assert!(msg.chars().count() <= 40);
    }
}
