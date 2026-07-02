use anyhow::Result;
use std::sync::Arc;

use crate::repo::Repository;

fn validate_svn_url(url: &str) -> Result<()> {
    if url.is_empty() {
        anyhow::bail!("empty SVN URL");
    }
    if url.contains('\0') || url.contains('\n') || url.contains('\r') {
        anyhow::bail!("SVN URL contains control characters");
    }
    if url.starts_with('-') {
        anyhow::bail!("SVN URL must not start with '-'");
    }
    if !url.starts_with("http://")
        && !url.starts_with("https://")
        && !url.starts_with("svn://")
        && !url.starts_with("svn+ssh://")
        && !url.starts_with("file://")
    {
        anyhow::bail!("invalid SVN URL format: {url}");
    }
    Ok(())
}

fn inject_git_creds(url: &str) -> String {
    if url.starts_with("https://") || url.starts_with("http://") {
        if let Ok(user) = std::env::var("NOA_GIT_USER") {
            if let Ok(pass) = std::env::var("NOA_GIT_PASS") {
                let rest = url
                    .strip_prefix("https://")
                    .or_else(|| url.strip_prefix("http://"))
                    .unwrap_or(url);
                return format!("https://{user}:{pass}@{rest}");
            }
        }
    }
    url.to_string()
}

pub async fn run_push(remote_name: &str) -> Result<()> {
    let root = Repository::find(std::path::Path::new("."))?;
    let repo = Repository::open(&root)?;
    let remote = repo
        .config
        .get_remote(remote_name)
        .ok_or_else(|| anyhow::anyhow!("remote '{remote_name}' not found"))?
        .clone();
    let db = Arc::clone(&repo.db);
    let remote_url = remote.url.clone();
    let remote_name_owned = remote_name.to_string();
    drop(repo);
    crate::git::export_noa_to_git(&root, db).await?;
    crate::git::export::validate_git_url(&remote_url)?;
    let root_push = root.clone();
    let url_for_push = inject_git_creds(&remote_url);
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("git")
            .args(["push", &url_for_push])
            .current_dir(&root_push)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
    })
    .await??;
    if output.status.success() {
        println!("Pushed to {remote_name_owned} ({remote_url})");
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git push failed: {}", stderr.trim());
    }
    Ok(())
}

pub async fn run_pull(remote_name: &str) -> Result<()> {
    let root = Repository::find(std::path::Path::new("."))?;
    let repo = Repository::open(&root)?;
    let remote = repo
        .config
        .get_remote(remote_name)
        .ok_or_else(|| anyhow::anyhow!("remote '{remote_name}' not found"))?
        .clone();
    crate::git::export::validate_git_url(&remote.url)?;
    let root_pull = root.clone();
    let url_for_pull = inject_git_creds(&remote.url);
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("git")
            .args(["pull", &url_for_pull])
            .current_dir(&root_pull)
            .env("GIT_TERMINAL_PROMPT", "0")
            .output()
    })
    .await??;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git pull failed: {}", stderr.trim());
    }
    let db = Arc::clone(&repo.db);
    let head_ws = repo.read_head()?;
    let ws_mgr_before = crate::workspace::WorkspaceManager::new(db.clone())?;
    let before_head = ws_mgr_before
        .get(&head_ws)
        .await?
        .map_or_else(crate::snapshot::empty_snapshot_id, |ws| ws.head.clone());
    drop(repo);
    crate::git::import::import_git_to_noa(&root, db.clone()).await?;
    let ref_store = crate::refs::RedbRefStore::new(db.clone())?;
    let head_ref = crate::refs::RefStore::get(&ref_store, "HEAD")
        .await
        .ok()
        .flatten();
    if let Some(snap_id) = head_ref {
        if snap_id != before_head {
            let ws_mgr = crate::workspace::WorkspaceManager::new(db)?;
            if let Err(e) = ws_mgr.update_head(&head_ws, &snap_id).await {
                eprintln!("warning: failed to update workspace head '{head_ws}': {e}");
            }
            println!("Pulled from {remote_name} and re-imported");
        } else {
            println!("Already up to date.");
        }
    } else {
        println!("Pulled from {remote_name} (no new changes)");
    }
    Ok(())
}

pub async fn run_fetch(target_name: &str) -> Result<()> {
    let root = Repository::find(std::path::Path::new("."))?;
    let repo = Repository::open(&root)?;

    let remote = repo
        .config
        .get_remote(target_name)
        .ok_or_else(|| anyhow::anyhow!("remote '{target_name}' not found"))?
        .clone();

    let backend = crate::git::GitBackend::new();
    let refs = crate::remote::RemoteBackend::list_refs(&backend, &remote.url).await?;

    if refs.is_empty() {
        println!("No remote refs found.");
        return Ok(());
    }

    println!("Remote refs from {target_name}:");
    for r in &refs {
        println!(
            "  {} -> {}",
            r.name,
            &r.commit_hash[..12.min(r.commit_hash.len())]
        );
    }
    Ok(())
}

pub async fn run_clone(url: &str, path: &str) -> Result<()> {
    let target = std::path::PathBuf::from(path);
    let canonical = if target.exists() {
        target.canonicalize().unwrap_or(target)
    } else {
        target
    };

    println!("Cloning {} into {} ...", url, canonical.display());

    crate::git::clone_git_to_noa(url, &canonical).await?;

    println!("Cloned and imported into noa: {}", canonical.display());
    println!(".git/ and .noa/ coexist — git manages source, noa manages agent data.");
    Ok(())
}

pub async fn run_clone_svn(url: &str, path: &str) -> Result<()> {
    validate_svn_url(url)?;

    let target = std::path::PathBuf::from(path);
    std::fs::create_dir_all(&target)?;

    let svn_url = if url.ends_with("/trunk") || url.contains("/trunk") {
        url.to_string()
    } else {
        format!("{}/trunk", url.trim_end_matches('/'))
    };

    println!(
        "Exporting from SVN {} into {} ...",
        svn_url,
        target.display()
    );

    let export_output = std::process::Command::new("svn")
        .args(["export", "--force", &svn_url, &target.to_string_lossy()])
        .output()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                anyhow::anyhow!("'svn' command not found — please install Subversion")
            } else {
                anyhow::anyhow!("svn export failed: {e}")
            }
        })?;

    if !export_output.status.success() {
        let stderr = String::from_utf8_lossy(&export_output.stderr);
        anyhow::bail!("svn export failed: {}", stderr.trim());
    }

    let git_init = std::process::Command::new("git")
        .args(["init"])
        .current_dir(&target)
        .status()?;
    if !git_init.success() {
        anyhow::bail!("git init failed");
    }

    let git_add = std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(&target)
        .status()?;
    if !git_add.success() {
        anyhow::bail!("git add -A failed");
    }

    let svn_rev_output = std::process::Command::new("svn")
        .args(["info", "--show-item", "revision", &svn_url])
        .output()
        .ok();

    let rev_info = svn_rev_output
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map_or_else(|| "?".to_string(), |s| s.trim().to_string());

    let commit_msg = format!("imported from SVN {svn_url}@r{rev_info}");

    let git_commit = std::process::Command::new("git")
        .args(["commit", "-m", &commit_msg])
        .current_dir(&target)
        .env("GIT_AUTHOR_NAME", "noa-svn-bridge")
        .env("GIT_AUTHOR_EMAIL", "noa@noa.local")
        .env("GIT_COMMITTER_NAME", "noa-svn-bridge")
        .env("GIT_COMMITTER_EMAIL", "noa@noa.local")
        .status()?;
    if !git_commit.success() {
        anyhow::bail!("git commit failed");
    }

    let remote_config = crate::config::RemoteConfig {
        name: "svn-origin".to_string(),
        url: url.to_string(),
        protocol: crate::config::RemoteProtocol::Svn,
    };
    let repo = crate::repo::Repository::init_with_remotes(&target, vec![remote_config])?;
    let db = std::sync::Arc::clone(&repo.db);
    crate::git::import::import_git_to_noa(&target, std::sync::Arc::clone(&db)).await?;

    let ref_store = crate::refs::RedbRefStore::new(std::sync::Arc::clone(&db))?;
    let head_ref = crate::refs::RefStore::get(&ref_store, "HEAD")
        .await
        .ok()
        .flatten();
    let head_snap_id = head_ref.unwrap_or_else(crate::snapshot::empty_snapshot_id);

    // Update the existing default workspace's head (created by init_with_remotes)
    let ws_mgr = crate::workspace::WorkspaceManager::new(db)?;
    if let Err(e) = ws_mgr.update_head("default", &head_snap_id).await {
        eprintln!("warning: failed to update default workspace head: {e}");
    }

    println!(
        "SVN repository exported and imported into noa: {}",
        target.display()
    );
    println!("Note: This is a one-time import. Use 'svn export' + 'noa snapshot create' for incremental sync.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_svn_url_valid_https() {
        assert!(validate_svn_url("https://svn.example.com/repo").is_ok());
    }

    #[test]
    fn test_validate_svn_url_valid_http() {
        assert!(validate_svn_url("http://svn.example.com/repo").is_ok());
    }

    #[test]
    fn test_validate_svn_url_valid_svn() {
        assert!(validate_svn_url("svn://example.com/repo").is_ok());
    }

    #[test]
    fn test_validate_svn_url_valid_svn_ssh() {
        assert!(validate_svn_url("svn+ssh://example.com/repo").is_ok());
    }

    #[test]
    fn test_validate_svn_url_valid_file() {
        assert!(validate_svn_url("file:///path/to/repo").is_ok());
    }

    #[test]
    fn test_validate_svn_url_empty() {
        assert!(validate_svn_url("").is_err());
    }

    #[test]
    fn test_validate_svn_url_control_chars() {
        assert!(validate_svn_url("http://example.com/repo\n").is_err());
        assert!(validate_svn_url("http://example.com/repo\0").is_err());
    }

    #[test]
    fn test_validate_svn_url_dash_prefix() {
        assert!(validate_svn_url("-http://example.com").is_err());
    }

    #[test]
    fn test_validate_svn_url_invalid_scheme() {
        assert!(validate_svn_url("ftp://example.com/repo").is_err());
        assert!(validate_svn_url("git://example.com/repo").is_err());
    }
}
