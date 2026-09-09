use anyhow::Context;
use std::{path::Path, sync::Arc};

use crate::{
    error::Result,
    object::{EntryKind, ObjectStore, TreeEntries, TreeEntry},
    refs::{RedbRefStore, RefStore},
    snapshot::{
        SnapshotId, content_addressed_snapshot_id_with_ts, RedbSnapshotStore, Snapshot,
        SnapshotStore,
    },
};

pub async fn import_git_to_noa(
    git_dir: &Path,
    db: Arc<redb::Database>,
) -> Result<Option<SnapshotId>> {
    let git_dir = git_dir.to_path_buf();
    let repo = tokio::task::spawn_blocking(move || gix::open(&git_dir).map_err(Box::new))
        .await?
        .map_err(|e| anyhow::anyhow!("failed to open git repo: {e}"))?;

    let obj_store = crate::object::RedbObjectStore::new(Arc::clone(&db))?;
    let snap_store = RedbSnapshotStore::new(Arc::clone(&db))?;
    let ref_store = RedbRefStore::new(Arc::clone(&db))?;
    let ws_mgr = crate::workspace::WorkspaceManager::new(Arc::clone(&db))?;

    let head_id = match repo.head_id() {
        Ok(id) => id.detach(),
        Err(_) => {
            tracing::warn!("git repository has no HEAD (empty repo), skipping import");
            return Ok(None);
        }
    };

    let head_obj = repo.find_object(head_id)?;

    let commit = head_obj
        .try_into_commit()
        .with_context(|| "HEAD is not a commit")?;

    let tree_id = commit.tree_id()?.detach();

    let entries = import_tree_recursive(&repo, tree_id, &obj_store).await?;

    let mut sorted = entries;
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let noa_tree_id = obj_store.put_tree(&TreeEntries(sorted)).await?;

    let author = commit
        .author()
        .ok()
        .map_or_else(|| "unknown".to_string(), |a| a.name.to_string());

    let message = commit
        .message_raw()
        .map(std::string::ToString::to_string)
        .unwrap_or_default();

    let time = commit.time()?;
    let timestamp = (time.seconds as u64) * 1_000_000;

    let snapshot = Snapshot {
        id: content_addressed_snapshot_id_with_ts(
            &noa_tree_id.0,
            &[],
            "default",
            &author,
            &message,
            timestamp,
        ),
        tree_hash: noa_tree_id.0,
        parents: vec![],
        workspace: "default".to_string(),
        author,
        timestamp,
        message,
    };
    snap_store.store(&snapshot).await?;
    // Write-through: advance HEAD from its actual current value so repeated
    // imports track the latest snapshot. The previous `cas("HEAD", None, ...)`
    // only matched when the ref was absent, freezing HEAD at the clone-time
    // snapshot forever (issue #70). Retry on races; only this import writes HEAD.
    let mut current = ref_store.get("HEAD").await?;
    loop {
        if ref_store
            .cas("HEAD", current.as_ref(), &snapshot.id)
            .await?
        {
            break;
        }
        current = ref_store.get("HEAD").await?;
    }

    let now = crate::now_micros();
    let ws = crate::workspace::Workspace {
        name: "default".to_string(),
        head: snapshot.id.clone(),
        base: crate::snapshot::empty_snapshot_id(),
        agent_id: None,
        last_seq: 0,
        created_at: now,
        updated_at: now,
    };
    if let Err(e) = ws_mgr.create(&ws).await {
        // Workspace may already exist from init_with_remotes; update in place
        if crate::error::is_workspace_already_exists(&e) {
            ws_mgr.update_head("default", &snapshot.id).await?;
        } else {
            return Err(e);
        }
    }

    Ok(Some(snapshot.id))
}

#[must_use]
pub fn is_lfs_pointer(content: &[u8]) -> bool {
    if content.len() > 500 {
        return false;
    }
    let Ok(s) = std::str::from_utf8(content) else {
        return false;
    };
    s.starts_with("version https://git-lfs.github.com/spec/")
}

fn walk_tree(
    repo: &gix::Repository,
    tree_id: gix::hash::ObjectId,
) -> Result<Vec<(String, Vec<u8>)>> {
    let mut stack = vec![(tree_id, String::new())];
    let mut results = Vec::new();

    while let Some((current_id, prefix)) = stack.pop() {
        let obj = repo.find_object(current_id)?;
        let tree = obj.try_into_tree().with_context(|| "not a tree")?;

        for entry_result in tree.iter() {
            let entry = entry_result?;
            let mode = entry.mode();
            let entry_id = entry.oid();
            let name = entry.filename().to_string();
            let full_name = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };

            if mode.is_tree() {
                stack.push((entry_id.to_owned(), full_name));
            } else {
                let blob_obj = repo.find_object(entry_id)?;
                let blob = blob_obj.try_into_blob().with_context(|| "not a blob")?;
                results.push((full_name, blob.data.clone()));
            }
        }
    }

    Ok(results)
}

async fn import_tree_recursive(
    repo: &gix::Repository,
    tree_id: gix::hash::ObjectId,
    obj_store: &crate::object::RedbObjectStore,
) -> Result<Vec<TreeEntry>> {
    let git_dir = repo.git_dir().to_path_buf();

    let file_contents = tokio::task::spawn_blocking(move || {
        let opened =
            gix::open(&git_dir).map_err(|e| anyhow::anyhow!("failed to open git repo: {e}"))?;
        walk_tree(&opened, tree_id)
    })
    .await??;

    let mut entries = Vec::with_capacity(file_contents.len());
    for (name, content) in file_contents {
        let blob_id = obj_store.put_blob(&content).await?;
        entries.push(TreeEntry {
            name,
            kind: EntryKind::Blob,
            id: blob_id.0,
        });
    }
    Ok(entries)
}
