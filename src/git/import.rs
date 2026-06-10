use anyhow::Context;
use std::{path::Path, sync::Arc};

use crate::{
    error::Result,
    object::{EntryKind, ObjectStore, TreeEntries, TreeEntry},
    refs::{RedbRefStore, RefStore},
    snapshot::{content_addressed_snapshot_id, RedbSnapshotStore, Snapshot, SnapshotStore},
};

pub async fn import_git_to_noa(git_dir: &Path, db: Arc<redb::Database>) -> Result<()> {
    let git_dir = git_dir.to_path_buf();
    let repo = tokio::task::spawn_blocking(move || gix::open(&git_dir).map_err(Box::new))
        .await?
        .map_err(|e| anyhow::anyhow!("failed to open git repo: {e}"))?;

    let obj_store = crate::object::RedbObjectStore::new(Arc::clone(&db))?;
    let snap_store = RedbSnapshotStore::new(Arc::clone(&db))?;
    let ref_store = RedbRefStore::new(db)?;

    let head_id = repo.head_id()?.detach();

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

    let snapshot = Snapshot {
        id: content_addressed_snapshot_id(&noa_tree_id.0, &[], "default"),
        tree_hash: noa_tree_id.0,
        parents: vec![],
        workspace: "default".to_string(),
        author,
        timestamp: (time.seconds as u64) * 1_000_000,
        message,
    };
    snap_store.store(&snapshot).await?;
    ref_store.cas("HEAD", None, &snapshot.id).await?;

    Ok(())
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
    let file_contents = walk_tree(repo, tree_id)?;
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
