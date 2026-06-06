use anyhow::Result;
use std::sync::Arc;

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
    drop(repo);
    crate::git::export_noa_to_git(&root, db).await?;

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
        ws_mgr.update_head(&head_ws, &snap_id).await.ok();
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
        let stderr = String::from_utf8_lossy(&export_output.stderr);
        anyhow::bail!("svn export failed: {}", stderr);
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

    let db_path = target.join(".noa").join("noa.redb");
    std::fs::create_dir_all(target.join(".noa").join("agent-logs"))?;

    let db = std::sync::Arc::new(
        redb::Database::builder()
            .create(&db_path)
            .map_err(|e| anyhow::anyhow!("db create failed: {}", e))?,
    );

    {
        let txn = db
            .begin_write()
            .map_err(|e| anyhow::anyhow!("db write failed: {}", e))?;
        {
            let _ = txn.open_table(redb::TableDefinition::<&[u8], &[u8]>::new("blobs"));
            let _ = txn.open_table(redb::TableDefinition::<&[u8], &[u8]>::new("trees"));
            let _ = txn.open_table(redb::TableDefinition::<&str, &[u8]>::new("snapshots"));
            let _ = txn.open_table(redb::TableDefinition::<&str, &[u8]>::new("workspaces"));
            let _ = txn.open_table(redb::TableDefinition::<&str, &[u8]>::new("refs"));
        }
        txn.commit()
            .map_err(|e| anyhow::anyhow!("db commit failed: {}", e))?;
    }

    let config = crate::config::RepoConfig {
        name: "default".to_string(),
        remotes: vec![crate::config::RemoteConfig {
            name: "svn-origin".to_string(),
            url: url.to_string(),
            protocol: "svn".to_string(),
        }],
        noa_remote: None,
    };
    config.save_to_dir(&target.join(".noa"))?;
    std::fs::write(target.join(".noa").join("HEAD"), "default\n")?;

    crate::git::import::import_git_to_noa(&target, std::sync::Arc::clone(&db)).await?;

    let ref_store = crate::refs::RedbRefStore::new(std::sync::Arc::clone(&db))?;
    let head_ref = crate::refs::RefStore::get(&ref_store, "HEAD")
        .await
        .ok()
        .flatten();
    let head_snap_id =
        head_ref.unwrap_or_else(|| crate::snapshot::SnapshotId("noa_empty".to_string()));

    let ws_mgr = crate::workspace::WorkspaceManager::new(std::sync::Arc::clone(&db))?;
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

    println!(
        "SVN repository exported and imported into noa: {}",
        target.display()
    );
    println!("Note: This is a one-time import. Use 'svn export' + 'noa snapshot create' for incremental sync.");
    Ok(())
}
