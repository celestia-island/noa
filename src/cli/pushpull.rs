use anyhow::Result;

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

pub async fn run_push(target_name: &str) -> Result<()> {
    let root = Repository::find(std::path::Path::new("."))?;
    let repo = Repository::open(&root)?;

    let transport = repo
        .config
        .get_transport(target_name)
        .ok_or_else(|| anyhow::anyhow!("transport '{target_name}' not found"))?
        .clone();

    crate::transport::push_vcs(&repo, &transport).await
}

pub async fn run_pull(target_name: &str) -> Result<()> {
    let root = Repository::find(std::path::Path::new("."))?;
    let mut repo = Repository::open(&root)?;

    let transport = repo
        .config
        .get_transport(target_name)
        .ok_or_else(|| anyhow::anyhow!("transport '{target_name}' not found"))?
        .clone();

    crate::transport::pull_vcs(&mut repo, &transport).await
}

pub async fn run_fetch(target_name: &str) -> Result<()> {
    let root = Repository::find(std::path::Path::new("."))?;
    let repo = Repository::open(&root)?;

    let transport = repo
        .config
        .get_transport(target_name)
        .ok_or_else(|| anyhow::anyhow!("transport '{target_name}' not found"))?
        .clone();

    let url = transport
        .url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("transport has no URL"))?;

    let backend = crate::git::GitBackend::new();
    let refs = crate::remote::RemoteBackend::list_refs(&backend, url).await?;

    if refs.is_empty() {
        println!("No remote refs found.");
        return Ok(());
    }

    println!("Remote refs from {target_name}:");
    for r in &refs {
        println!("  {} -> {}", r.name, &r.commit_hash[..12.min(r.commit_hash.len())]);
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

    let transport_config = crate::config::TransportConfig::vcs_svn("svn-origin", url);
    let repo = crate::repo::Repository::init_with_remotes(&target, vec![transport_config])?;

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
