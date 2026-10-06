use anyhow::Result;

use crate::{
    cli::log_cmd::truncate_message,
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
            let parents = if ws.head.is_empty() {
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

    // Compute the new sequence number first so we fail early
    // if the log is unavailable. This avoids partial updates.
    let new_seq = crate::log::AgentLog::next_seq(&engine.log).await?;

    // Update the ref store via CAS first. If this fails, nothing
    // has been modified yet — the system is fully consistent.
    let ref_store = repo.ref_store()?;
    let current_ref = ref_store.get(&head_ws).await?;
    match ref_store
        .cas(&head_ws, current_ref.as_ref(), &snapshot.id)
        .await
    {
        Ok(true) => {}
        Ok(false) => {
            anyhow::bail!(
                "concurrent modification detected: ref '{head_ws}' was modified during snapshot creation"
            );
        }
        Err(e) => {
            anyhow::bail!("failed to update ref '{head_ws}': {e}");
        }
    }

    // Then update workspace head and seq. The ref has already been
    // updated atomically by CAS; if this step fails the ref still
    // points to the new snapshot, keeping the system consistent.
    ws_mgr
        .update_head_and_seq(&head_ws, &snapshot.id, new_seq)
        .await?;

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
        let msg = truncate_message(&snap.message, 40, "...");
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
        .map_err(|_| anyhow::anyhow!("snapshot {a} not found"))?;
    let snap_b = snap_store
        .get(&SnapshotId(b.to_string()))
        .await
        .map_err(|_| anyhow::anyhow!("snapshot {b} not found"))?;

    let tree_a = obj_store
        .get_tree(&crate::object::TreeId(snap_a.tree_hash))
        .await?;
    let tree_b = obj_store
        .get_tree(&crate::object::TreeId(snap_b.tree_hash))
        .await?;

    let diffs =
        crate::snapshot::diff_snapshots_recursive(&tree_a.0, &tree_b.0, "", &obj_store).await;

    if diffs.is_empty() {
        println!("No differences between {a} and {b}");
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
    use crate::cli::log_cmd::truncate_message;

    /// The snapshot table truncates at 40 chars (not 50 like the log table).
    /// Verify the *production* truncation helper at this width rather than
    /// re-implementing the logic inline.
    #[test]
    fn test_utf8_truncation_no_panic() {
        let msg = "你好世界".repeat(20);
        let truncated = truncate_message(&msg, 40, "...");
        assert!(
            truncated.chars().count() <= 40,
            "snapshot-table truncation must respect the 40-char cap, got {}: {}",
            truncated.chars().count(),
            truncated
        );
        assert!(
            truncated.ends_with("..."),
            "truncated output must end with the ellipsis, got: {}",
            truncated
        );

        let emoji_msg = "🎉🚀💎".repeat(20);
        let truncated_emoji = truncate_message(&emoji_msg, 40, "...");
        assert!(truncated_emoji.chars().count() <= 40);
        assert!(truncated_emoji.ends_with("..."));
    }

    #[test]
    fn test_short_message_not_truncated() {
        let msg = "short msg";
        assert_eq!(truncate_message(msg, 40, "..."), msg);
        // Boundary cases at the 40-char width.
        let exact: String = "a".repeat(40);
        assert_eq!(truncate_message(&exact, 40, "..."), exact);
        let over: String = "a".repeat(41);
        let trunc = truncate_message(&over, 40, "...");
        assert_eq!(trunc.chars().count(), 40);
        assert!(trunc.ends_with("..."));
    }
}
