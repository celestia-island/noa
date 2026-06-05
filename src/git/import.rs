use std::path::Path;
use std::sync::Arc;

use crate::error::{NoaError, Result};
use crate::object::ObjectStore;
use crate::refs::{RedbRefStore, RefStore};
use crate::snapshot::{RedbSnapshotStore, Snapshot, SnapshotId, SnapshotStore};

pub async fn import_git_to_noa(
    git_dir: &Path,
    db: Arc<redb::Database>,
) -> Result<()> {
    let repo = git2::Repository::open(git_dir)
        .map_err(|e| NoaError::Remote(e.message().to_string()))?;

    let obj_store = crate::object::RedbObjectStore::new(Arc::clone(&db))?;
    let snap_store = RedbSnapshotStore::new(Arc::clone(&db))?;
    let ref_store = RedbRefStore::new(db)?;

    let head_ref = repo.head()
        .map_err(|e| NoaError::Remote(e.message().to_string()))?;
    let head_target = head_ref.target();

    if let Some(head_oid) = head_target {
        let commit = repo.find_commit(head_oid)
            .map_err(|e| NoaError::Remote(e.message().to_string()))?;

        import_tree_recursive(&repo, &commit.tree()
            .map_err(|e| NoaError::Remote(e.message().to_string()))?, &obj_store)?;

        let snapshot = Snapshot {
            id: SnapshotId(format!("noa_{}", &head_oid.to_string()[..12])),
            tree_hash: head_oid.to_string(),
            parents: vec![],
            workspace: "default".to_string(),
            author: commit.author().name().unwrap_or("unknown").to_string(),
            timestamp: commit.time().seconds() as u64,
            message: commit.message().unwrap_or("").to_string(),
        };
        snap_store.store(&snapshot).await?;
        ref_store.cas("HEAD", None, &snapshot.id).await?;
    }

    Ok(())
}

fn import_tree_recursive(
    repo: &git2::Repository,
    tree: &git2::Tree,
    obj_store: &crate::object::RedbObjectStore,
) -> Result<()> {
    for entry in tree.iter() {
        let obj = entry.to_object(repo)
            .map_err(|e| NoaError::Remote(e.message().to_string()))?;

        match obj.kind() {
            Some(git2::ObjectType::Blob) => {
                if let Some(blob) = obj.as_blob() {
                    let content = blob.content();
                    tokio::task::block_in_place(|| {
                        tokio::runtime::Handle::current().block_on(obj_store.put_blob(content))
                    })?;
                }
            }
            Some(git2::ObjectType::Tree) => {
                if let Some(subtree) = obj.as_tree() {
                    import_tree_recursive(repo, subtree, obj_store)?;
                }
            }
            _ => {}
        }
    }
    Ok(())
}

pub async fn export_noa_to_git(
    _db_path: &std::path::Path,
    _git_dir: &std::path::Path,
) -> Result<()> {
    todo!("noa → git packfile export")
}
