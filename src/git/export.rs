use std::{path::Path, process::Command, sync::Arc};

use crate::{
    error::{NoaError, Result},
    object::ObjectStore,
    refs::{RedbRefStore, RefStore},
    snapshot::{RedbSnapshotStore, SnapshotStore},
};

pub fn detect_lfs_available(repo_root: &Path) -> bool {
    Command::new("git")
        .args(["lfs", "version"])
        .current_dir(repo_root)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

pub fn lfs_install(repo_root: &Path) {
    let _ = Command::new("git")
        .args(["lfs", "install"])
        .current_dir(repo_root)
        .status();
}

pub fn lfs_pull(repo_root: &Path) {
    let _ = Command::new("git")
        .args(["lfs", "pull"])
        .current_dir(repo_root)
        .status();
}

pub fn lfs_push_all(repo_root: &Path, remote_url: &str) {
    let _ = Command::new("git")
        .args(["lfs", "push", "--all", remote_url])
        .current_dir(repo_root)
        .status();
}

pub fn has_lfs_tracking(repo_root: &Path) -> bool {
    repo_root.join(".gitattributes").exists()
        || Command::new("git")
            .args(["lfs", "track"])
            .current_dir(repo_root)
            .output()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(false)
}

pub async fn export_noa_to_git(repo_root: &Path, db: Arc<redb::Database>) -> Result<()> {
    let git_dir = repo_root.join(".git");
    if !git_dir.exists() {
        return Err(NoaError::Remote(".git directory not found".to_string()));
    }

    let snap_store = RedbSnapshotStore::new(Arc::clone(&db))?;
    let ref_store = RedbRefStore::new(Arc::clone(&db))?;
    let ws_mgr = crate::workspace::WorkspaceManager::new(Arc::clone(&db))?;
    let obj_store = crate::object::RedbObjectStore::new(db)?;

    let head_ws = std::fs::read_to_string(repo_root.join(".noa").join("HEAD"))
        .map_err(NoaError::Io)?
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
        let blob = obj_store
            .get_blob(&crate::object::BlobId(entry.id.clone()))
            .await?;
        std::fs::write(&file_path, &blob)?;
    }

    if has_lfs_tracking(repo_root) && detect_lfs_available(repo_root) {
        let _ = Command::new("git")
            .args(["lfs", "install"])
            .current_dir(repo_root)
            .status();
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

pub async fn clone_git_to_noa(url: &str, target: &Path) -> Result<()> {
    Command::new("git")
        .args(["clone", url, &target.to_string_lossy()])
        .status()
        .map_err(|e| NoaError::Remote(format!("git clone failed: {}", e)))
        .and_then(|s| {
            if s.success() {
                Ok(())
            } else {
                Err(NoaError::Remote(
                    "git clone exited with non-zero status".to_string(),
                ))
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
        let txn = db
            .begin_write()
            .map_err(|e| NoaError::Redb(e.to_string()))?;
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
        polemos: None,
    };
    config.save_to_dir(&target.join(".noa"))?;

    std::fs::write(target.join(".noa").join("HEAD"), "default\n")?;

    super::import::import_git_to_noa(target, Arc::clone(&db)).await?;

    let ref_store = crate::refs::RedbRefStore::new(Arc::clone(&db))?;
    let head_ref = ref_store.get("HEAD").await.ok().flatten();
    let head_snap_id =
        head_ref.unwrap_or_else(|| crate::snapshot::SnapshotId("noa_empty".to_string()));

    let ws_mgr = crate::workspace::WorkspaceManager::new(Arc::clone(&db))?;
    let now = chrono::Utc::now().timestamp_micros() as u64;
    ws_mgr
        .create(&crate::workspace::Workspace {
            name: "default".to_string(),
            head: head_snap_id.clone(),
            base: head_snap_id.clone(),
            agent_id: None,
            last_seq: 0,
            created_at: now,
            updated_at: now,
        })
        .await
        .ok();

    let gitignore_path = target.join(".gitignore");
    if !gitignore_path.exists() {
        std::fs::write(&gitignore_path, "# Added by noa\n.noa/\n")?;
    } else {
        let content = std::fs::read_to_string(&gitignore_path)?;
        if !content
            .lines()
            .any(|l| l.trim() == ".noa/" || l.trim() == ".noa")
        {
            std::fs::write(&gitignore_path, format!("{}\n.noa/\n", content.trim_end()))?;
        }
    }

    if detect_lfs_available(target) {
        lfs_install(target);
        if has_lfs_tracking(target) {
            lfs_pull(target);
        }
    }

    Ok(())
}
