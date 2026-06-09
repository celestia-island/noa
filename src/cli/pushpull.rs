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
        anyhow::bail!("invalid SVN URL format: {}", url);
    }
    Ok(())
}

pub async fn run_push(remote_name: &str) -> Result<()> {
    let root = Repository::find(std::path::Path::new("."))?;
    let repo = Repository::open(&root)?;

    let remote = repo
        .config
        .get_remote(remote_name)
        .ok_or_else(|| anyhow::anyhow!("remote '{}' not found", remote_name))?
        .clone();

    let db = Arc::clone(&repo.db);
    drop(repo);
    crate::git::export_noa_to_git(&root, db).await?;

    crate::git::export::validate_git_url(&remote.url)?;
    let output = std::process::Command::new("git")
        .args(["push", &remote.url])
        .current_dir(&root)
        .output()?;

    if output.status.success() {
        if crate::git::export::detect_lfs_available(&root) {
            crate::git::export::lfs_push_all(&root, &remote.url);
        }
        println!("Pushed to {} ({})", remote_name, remote.url);
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
        .ok_or_else(|| anyhow::anyhow!("remote '{}' not found", remote_name))?
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
        crate::git::export::lfs_pull(&root);
    }

    let db = Arc::clone(&repo.db);
    let head_ws = repo.read_head()?;
    drop(repo);
    crate::git::import::import_git_to_noa(&root, db.clone()).await?;

    let ref_store = crate::refs::RedbRefStore::new(db.clone())?;
    let head_ref = crate::refs::RefStore::get(&ref_store, "HEAD")
        .await
        .ok()
        .flatten();
    if let Some(snap_id) = head_ref {
        let ws_mgr = crate::workspace::WorkspaceManager::new(db)?;
        if let Err(e) = ws_mgr.update_head(&head_ws, &snap_id).await {
            eprintln!("warning: failed to update workspace head after pull: {}", e);
        }
    }

    println!("Pulled from {} and re-imported into noa", remote_name);

    Ok(())
}

pub async fn run_fetch(remote_name: &str) -> Result<()> {
    let root = Repository::find(std::path::Path::new("."))?;
    let repo = Repository::open(&root)?;

    let remote = repo
        .config
        .get_remote(remote_name)
        .ok_or_else(|| anyhow::anyhow!("remote '{}' not found", remote_name))?
        .clone();

    let backend = crate::git::GitBackend::new();
    let refs = crate::remote::RemoteBackend::list_refs(&backend, &remote.url).await?;

    if refs.is_empty() {
        println!("No remote refs found.");
        return Ok(());
    }

    println!("Remote refs from {}:", remote_name);
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
        .output()?;

    if !export_output.status.success() {
        anyhow::bail!("svn export failed");
    }

    std::process::Command::new("git")
        .args(["init"])
        .current_dir(&target)
        .status()?;

    std::process::Command::new("git")
        .args(["add", "-A"])
        .current_dir(&target)
        .status()?;

    let svn_rev_output = std::process::Command::new("svn")
        .args(["info", "--show-item", "revision", &svn_url])
        .output()
        .ok();

    let rev_info = svn_rev_output
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|| "?".to_string());

    let commit_msg = format!("imported from SVN {}@r{}", svn_url, rev_info);

    std::process::Command::new("git")
        .args(["commit", "-m", &commit_msg])
        .current_dir(&target)
        .env("GIT_AUTHOR_NAME", "noa-svn-bridge")
        .env("GIT_AUTHOR_EMAIL", "noa@noa.local")
        .env("GIT_COMMITTER_NAME", "noa-svn-bridge")
        .env("GIT_COMMITTER_EMAIL", "noa@noa.local")
        .status()?;

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
        eprintln!("warning: failed to update default workspace head: {}", e);
    }

    println!(
        "SVN repository exported and imported into noa: {}",
        target.display()
    );
    println!("Note: This is a one-time import. Use 'svn export' + 'noa snapshot create' for incremental sync.");
    Ok(())
}
