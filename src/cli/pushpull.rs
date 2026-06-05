use std::sync::Arc;

use anyhow::Result;

use crate::object::ObjectStore;
use crate::repo::Repository;

pub async fn run_push(remote_name: &str) -> Result<()> {
    let root = Repository::find(std::path::Path::new("."))?;
    let repo = Repository::open(&root)?;

    let remote = repo
        .config
        .get_remote(remote_name)
        .ok_or_else(|| anyhow::anyhow!("remote '{}' not found", remote_name))?
        .clone();

    let db = Arc::clone(&repo.db);
    crate::git::export_noa_to_git(&root, db).await?;

    let output = std::process::Command::new("git")
        .args(["push", &remote.url])
        .current_dir(&root)
        .output()?;

    if output.status.success() {
        println!("Pushed to {} ({})", remote_name, remote.url);
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git push failed: {}", stderr);
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

    let output = std::process::Command::new("git")
        .args(["pull", &remote.url])
        .current_dir(&root)
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git pull failed: {}", stderr);
    }

    let db = Arc::clone(&repo.db);
    crate::git::import::import_git_to_noa(&root, db).await?;
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
