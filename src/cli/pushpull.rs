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

pub async fn run_push(remote_name: &str) -> Result<()> {
    let root = Repository::find(std::path::Path::new("."))?;
    let repo = Repository::open(&root)?;

    let remote = repo
        .config
        .get_remote(remote_name)
        .ok_or_else(|| anyhow::anyhow!("remote '{remote_name}' not found"))?
        .clone();
    let remote_url = remote.url.clone();
    let remote_name_owned = remote_name.to_string();

    let db = Arc::clone(&repo.db);
    drop(repo);
    crate::git::export_noa_to_git(&root, db).await?;

    crate::git::export::validate_git_url(&remote_url)?;
    let root_push = root.clone();
    let root_lfs = root.clone();
    let url_for_push = remote_url.clone();
    let url_for_lfs = remote_url.clone();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("git")
            .args(["push", &url_for_push])
            .current_dir(&root_push)
            .output()
    })
    .await??;

    if output.status.success() {
        tokio::task::spawn_blocking(move || {
            if crate::git::export::detect_lfs_available(&root_lfs) {
                if let Err(e) = crate::git::export::lfs_push_all(&root_lfs, &url_for_lfs) {
                    eprintln!("warning: git lfs push --all failed: {e}");
                }
            }
            Ok::<(), anyhow::Error>(())
        })
        .await??;
        println!("Pushed to {} ({})", remote_name_owned, remote_url);
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
    let output = std::process::Command::new("git")
        .args(["pull", &remote.url])
        .current_dir(&root)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git pull failed: {}", stderr.trim());
    }

    if crate::git::export::detect_lfs_available(&root)
        && crate::git::export::has_lfs_tracking(&root)
    {
        if let Err(e) = crate::git::export::lfs_pull(&root) {
            eprintln!("warning: git lfs pull failed: {e}");
        }
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
        if snap_id == before_head {
            println!("Already up to date.");
        } else {
            let ws_mgr = crate::workspace::WorkspaceManager::new(db)?;
            if let Err(e) = ws_mgr.update_head(&head_ws, &snap_id).await {
                eprintln!("warning: failed to update workspace head after pull: {e}");
            }
            println!("Pulled from {remote_name} and re-imported into noa");
        }
    } else {
        println!("Pulled from {remote_name} (no new changes)");
    }

    Ok(())
}

pub async fn run_fetch(remote_name: &str) -> Result<()> {
    let root = Repository::find(std::path::Path::new("."))?;
    let repo = Repository::open(&root)?;

    let remote = repo
        .config
        .get_remote(remote_name)
        .ok_or_else(|| anyhow::anyhow!("remote '{remote_name}' not found"))?
        .clone();

    let backend = crate::git::GitBackend::new();
    let refs = crate::remote::RemoteBackend::list_refs(&backend, &remote.url).await?;

    if refs.is_empty() {
        println!("No remote refs found.");
        return Ok(());
    }

    println!("Remote refs from {remote_name}:");
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
        protocol: "svn".to_string(),
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
