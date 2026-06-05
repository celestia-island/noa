use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use crate::error::{NoaError, Result};
use crate::object::ObjectStore;
use crate::refs::{RedbRefStore, RefStore};
use crate::snapshot::{RedbSnapshotStore, Snapshot, SnapshotId, SnapshotStore};

pub async fn export_noa_to_git(
    repo_root: &Path,
    db: Arc<redb::Database>,
) -> Result<()> {
    let git_dir = repo_root.join(".git");
    if !git_dir.exists() {
        return Err(NoaError::Remote(".git directory not found".to_string()));
    }

    let snap_store = RedbSnapshotStore::new(Arc::clone(&db))?;
    let ref_store = RedbRefStore::new(Arc::clone(&db))?;
    let ws_mgr = crate::workspace::WorkspaceManager::new(Arc::clone(&db))?;
    let obj_store = crate::object::RedbObjectStore::new(db)?;

    let head_ws = std::fs::read_to_string(repo_root.join(".noa").join("HEAD"))
        .map_err(|e| NoaError::Io(e))?
        .trim()
        .to_string();

    let ws = ws_mgr.get(&head_ws).await?;
    let snap_id = match ws {
        Some(w) => w.head,
        None => {
            let head_ref = ref_store.get("HEAD").await?;
            match head_ref {
                Some(id) => id,
                None => return Err(NoaError::Remote("no HEAD snapshot found".to_string())),
            }
        }
    };
    let snapshot = snap_store.get(&snap_id).await?;

    let tree = obj_store
        .get_tree(&crate::object::TreeId(snapshot.tree_hash.clone()))
        .await?;

    for entry in &tree.0 {
        let file_path = repo_root.join(&entry.name);
        if let Some(parent) = file_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let blob = obj_store.get_blob(&crate::object::BlobId(entry.id.clone())).await?;
        std::fs::write(&file_path, &blob)?;
    }

    let status_output = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(repo_root)
        .output()
        .map_err(|e| NoaError::Remote(format!("git status failed: {}", e)))?;

    let has_changes = !status_output.stdout.is_empty();
    if has_changes {
        Command::new("git")
            .args(["add", "-A"])
            .current_dir(repo_root)
            .status()
            .map_err(|e| NoaError::Remote(format!("git add failed: {}", e)))?;

        let msg = format!(
            "[noa export] snapshot {} from workspace {}",
            snapshot.id, snapshot.workspace
        );
        Command::new("git")
            .args(["commit", "-m", &msg])
            .current_dir(repo_root)
            .env("GIT_AUTHOR_NAME", &snapshot.author)
            .env("GIT_AUTHOR_EMAIL", "noa@noa.local")
            .env("GIT_COMMITTER_NAME", &snapshot.author)
            .env("GIT_COMMITTER_EMAIL", "noa@noa.local")
            .status()
            .map_err(|e| NoaError::Remote(format!("git commit failed: {}", e)))?;
    }

    Ok(())
}

pub async fn clone_git_to_noa(
    url: &str,
    target: &Path,
) -> Result<()> {
    Command::new("git")
        .args(["clone", url, &target.to_string_lossy()])
        .status()
        .map_err(|e| NoaError::Remote(format!("git clone failed: {}", e)))
        .and_then(|s| {
            if s.success() {
                Ok(())
            } else {
                Err(NoaError::Remote("git clone exited with non-zero status".to_string()))
            }
        })?;

    let db_path = target.join(".noa").join("noa.redb");
    std::fs::create_dir_all(target.join(".noa").join("agent-logs"))?;

    let db = Arc::new(
        redb::Database::builder()
            .create(&db_path)
            .map_err(|e| NoaError::Redb(e.to_string()))?,
    );

    {
        let txn = db.begin_write().map_err(|e| NoaError::Redb(e.to_string()))?;
        {
            let _ = txn.open_table(redb::TableDefinition::<&[u8], &[u8]>::new("blobs"));
            let _ = txn.open_table(redb::TableDefinition::<&[u8], &[u8]>::new("trees"));
            let _ = txn.open_table(redb::TableDefinition::<&str, &[u8]>::new("snapshots"));
            let _ = txn.open_table(redb::TableDefinition::<&str, &[u8]>::new("workspaces"));
            let _ = txn.open_table(redb::TableDefinition::<&str, &[u8]>::new("refs"));
        }
        txn.commit().map_err(|e| NoaError::Redb(e.to_string()))?;
    }

    let config = crate::config::RepoConfig {
        name: "default".to_string(),
        remotes: vec![crate::config::RemoteConfig {
            name: "origin".to_string(),
            url: url.to_string(),
            protocol: "git".to_string(),
        }],
        noa_remote: None,
    };
    config.save_to_dir(&target.join(".noa"))?;

    std::fs::write(target.join(".noa").join("HEAD"), "default\n")?;

    super::import::import_git_to_noa(target, Arc::clone(&db)).await?;

    let snap_store = crate::snapshot::RedbSnapshotStore::new(Arc::clone(&db))?;
    let ref_store = crate::refs::RedbRefStore::new(Arc::clone(&db))?;
    let head_ref = ref_store.get("HEAD").await.ok().flatten();
    let head_snap_id = head_ref.unwrap_or_else(|| crate::snapshot::SnapshotId("noa_empty".to_string()));

    let ws_mgr = crate::workspace::WorkspaceManager::new(Arc::clone(&db))?;
    let now = chrono::Utc::now().timestamp_micros() as u64;
    ws_mgr.create(&crate::workspace::Workspace {
        name: "default".to_string(),
        head: head_snap_id.clone(),
        base: head_snap_id.clone(),
        agent_id: None,
        created_at: now,
        updated_at: now,
    }).await.ok();

    let gitignore_path = target.join(".gitignore");
    if !gitignore_path.exists() {
        std::fs::write(&gitignore_path, "# Added by noa\n.noa/\n")?;
    } else {
        let content = std::fs::read_to_string(&gitignore_path)?;
        if !content.lines().any(|l| l.trim() == ".noa/" || l.trim() == ".noa") {
            std::fs::write(&gitignore_path, format!("{}\n.noa/\n", content.trim_end()))?;
        }
    }

    Ok(())
}
