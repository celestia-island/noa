use std::path::Path;
use std::sync::Arc;

use crate::error::{NoaError, Result};
use crate::object::{EntryKind, ObjectStore, TreeEntries, TreeEntry};
use crate::refs::{RedbRefStore, RefStore};
use crate::snapshot::{RedbSnapshotStore, Snapshot, SnapshotId, SnapshotStore};

pub async fn import_git_to_noa(
    git_dir: &Path,
    db: Arc<redb::Database>,
) -> Result<()> {
    let repo = gix::open(git_dir)
        .map_err(|e| NoaError::Remote(e.to_string()))?;

    let obj_store = crate::object::RedbObjectStore::new(Arc::clone(&db))?;
    let snap_store = RedbSnapshotStore::new(Arc::clone(&db))?;
    let ref_store = RedbRefStore::new(db)?;

    let head_id = repo.head_id()
        .map_err(|e| NoaError::Remote(e.to_string()))?
        .detach();

    let head_obj = repo.find_object(head_id)
        .map_err(|e| NoaError::Remote(e.to_string()))?;

    let commit = head_obj.try_into_commit()
        .map_err(|e| NoaError::Remote(format!("HEAD is not a commit: {}", e)))?;

    let tree_id = commit.tree_id()
        .map_err(|e| NoaError::Remote(e.to_string()))?
        .detach();

    let entries = import_tree_recursive(&repo, tree_id, &obj_store)?;

    let mut sorted = entries;
    sorted.sort_by(|a, b| a.name.cmp(&b.name));
    let noa_tree_id = obj_store.put_tree(&TreeEntries(sorted)).await?;

    let author = commit.author()
        .ok()
        .map(|a| a.name.to_string())
        .unwrap_or_else(|| "unknown".to_string());

    let message = commit.message_raw()
        .map(|m| m.to_string())
        .unwrap_or_default();

    let time = commit.time()
        .map_err(|e| NoaError::Remote(e.to_string()))?;

    let snapshot = Snapshot {
        id: SnapshotId(format!("noa_{}", &head_id.to_hex().to_string()[..12])),
        tree_hash: noa_tree_id.0,
        parents: vec![],
        workspace: "default".to_string(),
        author,
        timestamp: time.seconds as u64,
        message,
    };
    snap_store.store(&snapshot).await?;
    ref_store.cas("HEAD", None, &snapshot.id).await?;

    Ok(())
}

pub fn is_lfs_pointer(content: &[u8]) -> bool {
    if content.len() > 500 {
        return false;
    }
    let s = match std::str::from_utf8(content) {
        Ok(s) => s,
        Err(_) => return false,
    };
    s.starts_with("version https://git-lfs.github.com/spec/")
}

fn import_tree_recursive(
    repo: &gix::Repository,
    tree_id: gix::hash::ObjectId,
    obj_store: &crate::object::RedbObjectStore,
) -> Result<Vec<TreeEntry>> {
    let obj = repo.find_object(tree_id)
        .map_err(|e| NoaError::Remote(e.to_string()))?;

    let tree = obj.try_into_tree()
        .map_err(|e| NoaError::Remote(format!("not a tree: {}", e)))?;

    let mut entries = Vec::new();

    for entry_result in tree.iter() {
        let entry = entry_result.map_err(|e| NoaError::Remote(e.to_string()))?;
        let mode = entry.mode();
        let entry_id = entry.oid();
        let filename = entry.filename()
            .to_string();

        if mode.is_tree() {
            let sub_entries = import_tree_recursive(repo, entry_id.to_owned(), obj_store)?;
            for mut sub in sub_entries {
                sub.name = format!("{}/{}", filename, sub.name);
                entries.push(sub);
            }
        } else {
            let blob_obj = repo.find_object(entry_id)
                .map_err(|e| NoaError::Remote(e.to_string()))?;
            let blob = blob_obj.try_into_blob()
                .map_err(|e| NoaError::Remote(format!("not a blob: {}", e)))?;
            let content = blob.data.clone();
            let blob_id = tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(obj_store.put_blob(&content))
            })?;
            entries.push(TreeEntry {
                name: filename,
                kind: EntryKind::Blob,
                id: blob_id.0,
            });
        }
    }

    Ok(entries)
}
