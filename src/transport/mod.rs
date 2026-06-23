pub mod git_raw;
pub mod sftp_impl;

use anyhow::Result;

use crate::{
    config::TransportConfig,
    object::{BlobId, ObjectStore, TreeId},
    repo::Repository,
    snapshot::SnapshotStore,
};

pub async fn create_raw_store(config: &TransportConfig) -> Result<Box<dyn ObjectStore>> {
    match config.protocol.as_str() {
        "ipfs" => {
            let endpoint = config.effective_endpoint();
            Ok(Box::new(crate::object::IpfsObjectStore::new(
                &endpoint,
                config.auth_token.clone(),
            )))
        }
        "s3" | "minio" => {
            let endpoint = config.effective_endpoint();
            let bucket = config
                .bucket
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("transport '{}' is missing 'bucket'", config.name))?;
            let access_key = config
                .access_key
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("transport '{}' is missing 'access_key'", config.name))?;
            let secret_key = config
                .secret_key
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("transport '{}' is missing 'secret_key'", config.name))?;
            let region = config.region.as_deref().unwrap_or("us-east-1");
            let store = crate::object::MinioObjectStore::from_config(
                &endpoint, bucket, access_key, secret_key, region,
            )
            .await?;
            Ok(Box::new(store))
        }
        #[cfg(feature = "ftp")]
        "ftp" | "ftps" => {
            let store = crate::object::FtpObjectStore::from_config(config)?;
            Ok(Box::new(store))
        }
        "sftp" => {
            let store = sftp_impl::SftpObjectStore::from_config(config)?;
            Ok(Box::new(store))
        }
        "git" => {
            let url = config
                .url
                .as_deref()
                .ok_or_else(|| anyhow::anyhow!("raw git transport requires 'url'"))?;
            let store = git_raw::GitRawObjectStore::new(url, config).await?;
            Ok(Box::new(store))
        }
        other => Err(anyhow::anyhow!(
            "unsupported protocol '{}' for raw mode: expected git, s3, ipfs, ftp, or sftp",
            other
        )),
    }
}

pub async fn push_vcs(repo: &Repository, transport: &TransportConfig) -> Result<()> {
    if transport.protocol != "git" {
        anyhow::bail!(
            "VCS push is only supported for git protocol, not '{}' (svn is import-only)",
            transport.protocol
        );
    }
    let url = transport
        .url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("VCS transport requires 'url'"))?;

    let root = repo.root.clone();
    let db = std::sync::Arc::clone(&repo.db);
    crate::git::export_noa_to_git(&root, db).await?;

    crate::git::export::validate_git_url(url)?;
    let url_owned = url.to_string();
    let root_push = root.clone();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("git")
            .args(["push", &url_owned])
            .current_dir(&root_push)
            .output()
    })
    .await??;

    if output.status.success() {
        println!("Pushed to {} ({})", transport.name, url);
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git push failed: {}", stderr.trim());
    }
    Ok(())
}

pub async fn pull_vcs(repo: &mut Repository, transport: &TransportConfig) -> Result<()> {
    if transport.protocol != "git" {
        anyhow::bail!(
            "VCS pull is only supported for git protocol, not '{}' (svn is import-only)",
            transport.protocol
        );
    }
    let url = transport
        .url
        .as_deref()
        .ok_or_else(|| anyhow::anyhow!("VCS transport requires 'url'"))?;

    crate::git::export::validate_git_url(url)?;
    let root = repo.root.clone();
    let url_owned = url.to_string();
    let output = tokio::task::spawn_blocking(move || {
        std::process::Command::new("git")
            .args(["pull", &url_owned])
            .current_dir(&root)
            .output()
    })
    .await??;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("git pull failed: {}", stderr.trim());
    }

    let root = repo.root.clone();
    let db = std::sync::Arc::clone(&repo.db);
    let head_ws = repo.read_head()?;
    let ws_mgr_before = crate::workspace::WorkspaceManager::new(db.clone())?;
    let before_head = ws_mgr_before
        .get(&head_ws)
        .await?
        .map_or_else(crate::snapshot::empty_snapshot_id, |ws| ws.head.clone());

    crate::git::import::import_git_to_noa(&root, db.clone()).await?;

    let ref_store = crate::refs::RedbRefStore::new(db.clone())?;
    let head_ref = crate::refs::RefStore::get(&ref_store, "HEAD")
        .await
        .ok()
        .flatten();
    if let Some(snap_id) = head_ref {
        if snap_id != before_head {
            let ws_mgr = crate::workspace::WorkspaceManager::new(db)?;
            let _ = ws_mgr.update_head(&head_ws, &snap_id).await;
            println!("Pulled from {} and re-imported", transport.name);
        } else {
            println!("Already up to date.");
        }
    } else {
        println!("Pulled from {} (no new changes)", transport.name);
    }
    Ok(())
}

pub async fn push_raw(
    repo: &Repository,
    transport: &TransportConfig,
    snapshot_id: Option<&str>,
    workspace: Option<&str>,
    do_pin: bool,
) -> Result<()> {
    let remote = create_raw_store(transport).await?;
    let local = repo.object_store()?;

    let snapshots = match snapshot_id {
        Some(id) => {
            let snap = repo
                .snapshot_store()?
                .get(&crate::snapshot::SnapshotId(id.to_string()))
                .await?;
            vec![snap]
        }
        None => {
            let all = repo.snapshot_store()?.list_all().await?;
            match workspace {
                Some(ws) => all.into_iter().filter(|s| s.workspace == ws).collect(),
                None => all,
            }
        }
    };

    if snapshots.is_empty() {
        println!("No snapshots to push.");
        return Ok(());
    }

    let mut pushed = 0u64;
    let mut failed = 0u64;

    let ipfs_store = if do_pin && transport.protocol == "ipfs" {
        let endpoint = transport.effective_endpoint();
        Some(crate::object::IpfsObjectStore::new(&endpoint, transport.auth_token.clone()))
    } else {
        None
    };

    for snap in &snapshots {
        let tree_id = TreeId(snap.tree_hash.clone());
        match push_tree_recursive(remote.as_ref(), &local, &tree_id).await {
            Ok(()) => {}
            Err(e) => {
                println!("  WARN: tree push error for {}: {e}", snap.id);
                failed += 1;
            }
        }

        if let Some(ref ipfs) = ipfs_store {
            let cid = crate::object::ipfs_impl::sha256_hex_to_cid(&snap.tree_hash)?;
            match ipfs.pin_add(&cid).await {
                Ok(()) => {
                    println!("  Pushed + pinned {} -> {}", snap.id, cid);
                    pushed += 1;
                }
                Err(e) => {
                    println!("  ERROR pinning {}: {e}", snap.id);
                    failed += 1;
                }
            }
        } else {
            println!("  Pushed {}", snap.id);
            pushed += 1;
        }
    }

    if transport.protocol == "git" {
        if let Some(url) = transport.url.as_deref() {
            let _ = git_raw::commit_and_push(url).await;
        }
    }

    println!(
        "\nPushed {} snapshot(s) to '{}'{}",
        pushed,
        transport.name,
        if failed > 0 { format!(" ({failed} failed)") } else { String::new() }
    );
    Ok(())
}

async fn push_tree_recursive(
    remote: &dyn ObjectStore,
    local: &crate::object::RedbObjectStore,
    tree_id: &TreeId,
) -> Result<()> {
    if remote.has_tree(tree_id).await.unwrap_or(false) {
        return Ok(());
    }
    let entries = local.get_tree(tree_id).await?;
    remote.put_tree(&entries).await?;
    for entry in &entries.0 {
        match entry.kind {
            crate::object::EntryKind::Blob => {
                let blob_id = BlobId(entry.id.clone());
                if !remote.has_blob(&blob_id).await.unwrap_or(false) {
                    if let Ok(data) = local.get_blob(&blob_id).await {
                        if let Err(e) = remote.put_blob(&data).await {
                            tracing::warn!("put_blob failed for {}: {e}", entry.id);
                        }
                    }
                }
            }
            crate::object::EntryKind::Tree => {
                Box::pin(push_tree_recursive(remote, local, &TreeId(entry.id.clone()))).await?;
            }
        }
    }
    Ok(())
}

pub async fn fetch_raw(
    repo: &Repository,
    transport: &TransportConfig,
    hash_or_cid: &str,
) -> Result<()> {
    let local = repo.object_store()?;
    print!("Fetching from '{}'... ", transport.name);

    let data = if hash_or_cid.starts_with("bafk") || hash_or_cid.starts_with("Qm") {
        if transport.protocol != "ipfs" {
            anyhow::bail!("CID-style fetch requires an IPFS transport");
        }
        let endpoint = transport.effective_endpoint();
        let ipfs = crate::object::IpfsObjectStore::new(&endpoint, transport.auth_token.clone());
        ipfs.block_get_raw(hash_or_cid).await?
    } else {
        let remote = create_raw_store(transport).await?;
        let blob_id = BlobId(hash_or_cid.to_string());
        remote.get_blob(&blob_id).await?
    };

    println!("OK ({} bytes)", data.len());
    let hash = crate::object::sha256_hex(&data);
    println!("  SHA-256: {hash}");
    local.put_blob(&data).await?;
    println!("  Stored to local object store.");
    Ok(())
}

pub async fn transport_status(repo: &Repository, target: Option<&str>) -> Result<()> {
    let targets: Vec<&TransportConfig> = match target {
        Some(name) => {
            let t = repo
                .config
                .get_transport(name)
                .ok_or_else(|| anyhow::anyhow!("transport '{name}' not found"))?;
            vec![t]
        }
        None => repo.config.transports.iter().collect(),
    };

    if targets.is_empty() {
        println!("No transports configured.");
        return Ok(());
    }

    for t in targets {
        println!("── {} (mode={}, type={}) ──", t.name, t.mode, t.protocol);
        match (t.mode.as_str(), t.protocol.as_str()) {
            ("vcs", "git") | ("vcs", "svn") => {
                println!("  URL: {}", t.url.as_deref().unwrap_or("?"));
                println!("  Mode: full repository sync");
            }
            ("raw", "ipfs") => {
                let endpoint = t.effective_endpoint();
                let store = crate::object::IpfsObjectStore::new(&endpoint, t.auth_token.clone());
                print!("  Connecting to {}... ", endpoint);
                match store.version().await {
                    Ok(v) => println!("OK (v{v})"),
                    Err(e) => println!("FAILED: {e}"),
                }
            }
            ("raw", "s3") | ("raw", "minio") => {
                println!("  Endpoint: {}", t.effective_endpoint());
                println!("  Bucket:   {}", t.bucket.as_deref().unwrap_or("?"));
            }
            ("raw", "ftp") | ("raw", "ftps") => {
                println!("  Endpoint: {}", t.effective_endpoint());
                println!("  Port:     {}", if t.port > 0 { t.port } else { 21 });
            }
            ("raw", "sftp") => {
                println!("  Endpoint: {}", t.effective_endpoint());
                println!("  Port:     {}", if t.port > 0 { t.port } else { 22 });
                println!("  User:     {}", t.username.as_deref().unwrap_or("?"));
            }
            ("raw", "git") => {
                println!("  URL: {}", t.url.as_deref().unwrap_or("?"));
                println!("  Mode: raw object backup via git");
            }
            _ => println!("  (unknown configuration)"),
        }
    }
    Ok(())
}
