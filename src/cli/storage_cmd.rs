use anyhow::Result;

use crate::{
    config::{StorageConfig, StorageProtocol},
    object::{create_remote_store, BlobId, ObjectStore, TreeId},
    repo::Repository,
    snapshot::SnapshotStore,
};

pub struct StorageAddOptions {
    pub endpoint: Option<String>,
    pub gateway: Option<String>,
    pub auth_token: Option<String>,
    pub auto_pin: bool,
    pub bucket: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
    pub region: Option<String>,
    pub username: Option<String>,
    pub password: Option<String>,
    pub port: u16,
    pub use_tls: bool,
}

pub fn run_add(
    repo: &mut Repository,
    name: &str,
    backend_type: &str,
    opts: StorageAddOptions,
) -> Result<()> {
    let protocol = match backend_type {
        "ipfs" => StorageProtocol::Ipfs,
        "s3" => StorageProtocol::S3,
        "minio" => StorageProtocol::Minio,
        "ftp" => StorageProtocol::Ftp,
        "ftps" => StorageProtocol::Ftps,
        "sftp" => StorageProtocol::Sftp,
        other => anyhow::bail!(
            "unknown storage type '{other}': expected ipfs, s3, minio, ftp, ftps, or sftp"
        ),
    };

    let mut cfg = match protocol {
        StorageProtocol::Ipfs => {
            let ep = opts
                .endpoint
                .unwrap_or_else(|| "http://127.0.0.1:5001".to_string());
            StorageConfig::ipfs(name, Some(&ep))
        }
        StorageProtocol::S3 => {
            let bucket = opts
                .bucket
                .ok_or_else(|| anyhow::anyhow!("--bucket is required for s3"))?;
            StorageConfig::s3(name, opts.endpoint.as_deref(), &bucket)
        }
        StorageProtocol::Minio => {
            let bucket = opts
                .bucket
                .ok_or_else(|| anyhow::anyhow!("--bucket is required for minio"))?;
            StorageConfig::minio(name, opts.endpoint.as_deref(), &bucket)
        }
        StorageProtocol::Ftp => {
            let ep = opts
                .endpoint
                .ok_or_else(|| anyhow::anyhow!("--endpoint is required for ftp"))?;
            StorageConfig::ftp(name, &ep)
        }
        StorageProtocol::Ftps => {
            let ep = opts
                .endpoint
                .ok_or_else(|| anyhow::anyhow!("--endpoint is required for ftps"))?;
            StorageConfig::ftps(name, &ep)
        }
        StorageProtocol::Sftp => {
            let ep = opts
                .endpoint
                .ok_or_else(|| anyhow::anyhow!("--endpoint is required for sftp"))?;
            StorageConfig::sftp(name, &ep)
        }
    };

    if let Some(gw) = opts.gateway {
        cfg.gateway = Some(gw);
    }
    if let Some(tok) = opts.auth_token {
        cfg.auth_token = Some(tok);
    }
    cfg.auto_pin = opts.auto_pin;
    if let Some(ak) = opts.access_key {
        cfg.access_key = Some(ak);
    }
    if let Some(sk) = opts.secret_key {
        cfg.secret_key = Some(sk);
    }
    if let Some(r) = opts.region {
        cfg.region = Some(r);
    }
    if let Some(u) = opts.username {
        cfg.username = Some(u);
    }
    if let Some(p) = opts.password {
        cfg.password = Some(p);
    }
    if opts.port > 0 {
        cfg.port = opts.port;
    }
    cfg.use_tls = opts.use_tls;

    println!(
        "Added storage '{}' (type: {}, endpoint: {})",
        cfg.name,
        cfg.backend_type,
        cfg.effective_endpoint()
    );

    repo.config.add_storage(cfg);
    repo.save_config()?;
    Ok(())
}

pub fn run_remove(repo: &mut Repository, name: &str) -> Result<()> {
    if repo.config.get_storage(name).is_none() {
        anyhow::bail!("storage '{name}' not found");
    }
    repo.config.remove_storage(name);
    repo.save_config()?;
    println!("Removed storage '{name}'");
    Ok(())
}

pub fn run_list(repo: &Repository) -> Result<()> {
    if repo.config.storage.is_empty() {
        println!("No storage backends configured.");
        return Ok(());
    }
    for s in &repo.config.storage {
        let extra = match s.backend_type {
            StorageProtocol::Ipfs => {
                format!("gateway={}", s.gateway.as_deref().unwrap_or("default"))
            }
            StorageProtocol::S3 | StorageProtocol::Minio => {
                format!("bucket={}", s.bucket.as_deref().unwrap_or("?"))
            }
            StorageProtocol::Ftp | StorageProtocol::Ftps | StorageProtocol::Sftp => {
                format!("user={}", s.username.as_deref().unwrap_or("?"))
            }
        };
        println!(
            "{}\t{} ({}) [{}]",
            s.name,
            s.effective_endpoint(),
            s.backend_type,
            extra
        );
    }
    Ok(())
}

pub async fn run_status(repo: &Repository, target: Option<&str>) -> Result<()> {
    let targets: Vec<&StorageConfig> = match target {
        Some(name) => {
            let cfg = repo
                .config
                .get_storage(name)
                .ok_or_else(|| anyhow::anyhow!("storage '{name}' not found"))?;
            vec![cfg]
        }
        None => repo.config.storage.iter().collect(),
    };

    if targets.is_empty() {
        println!("No storage backends configured.");
        return Ok(());
    }

    for cfg in targets {
        println!("── {} ({}) ──", cfg.name, cfg.backend_type);
        match cfg.backend_type {
            StorageProtocol::Ipfs => {
                let store = crate::object::IpfsObjectStore::new(
                    &cfg.effective_endpoint(),
                    cfg.auth_token.clone(),
                );
                print!("  Connecting to {}... ", cfg.effective_endpoint());
                match store.version().await {
                    Ok(v) => {
                        println!("OK (v{v})");
                        match store.repo_stat().await {
                            Ok(stat) => {
                                println!("    Objects: {}", stat.num_objects);
                                println!("    Size:    {} bytes", stat.repo_size);
                            }
                            Err(e) => println!("    Stats unavailable: {e}"),
                        }
                    }
                    Err(e) => println!("FAILED: {e}"),
                }
            }
            StorageProtocol::S3 | StorageProtocol::Minio => {
                println!("  Endpoint: {}", cfg.effective_endpoint());
                println!("  Bucket:   {}", cfg.bucket.as_deref().unwrap_or("?"));
                match create_remote_store(cfg).await {
                    Ok(_) => println!("  Connection: OK"),
                    Err(e) => println!("  Connection: FAILED ({e})"),
                }
            }
            #[cfg(feature = "ftp")]
            StorageProtocol::Ftp | StorageProtocol::Ftps => {
                println!("  Endpoint: {}", cfg.effective_endpoint());
                println!("  Port: {}", if cfg.port > 0 { cfg.port } else { 21 });
                match create_remote_store(cfg).await {
                    Ok(_) => println!("  Connection: OK"),
                    Err(e) => println!("  Connection: FAILED ({e})"),
                }
            }
            StorageProtocol::Sftp => {
                println!("  Endpoint: {}", cfg.effective_endpoint());
                println!("  Port: {}", if cfg.port > 0 { cfg.port } else { 22 });
                println!("  User: {}", cfg.username.as_deref().unwrap_or("?"));
                match create_remote_store(cfg).await {
                    Ok(_) => println!("  Connection: OK"),
                    Err(e) => println!("  Connection: FAILED ({e})"),
                }
            }
        }
    }
    Ok(())
}

pub async fn run_push(
    repo: &Repository,
    target: Option<&str>,
    snapshot_id: Option<&str>,
    workspace: Option<&str>,
    do_pin: bool,
) -> Result<()> {
    let cfg = match target {
        Some(name) => repo
            .config
            .get_storage(name)
            .ok_or_else(|| anyhow::anyhow!("storage '{name}' not found"))?
            .clone(),
        None => {
            let cs: Vec<_> = repo
                .config
                .storage
                .iter()
                .filter(|s| s.auto_pin || do_pin)
                .collect();
            match cs.len() {
                0 => anyhow::bail!(
                    "no storage target specified. Use --target <name> or configure auto-pin."
                ),
                1 => cs[0].clone(),
                _ => anyhow::bail!("multiple candidates. Specify --target <name> to disambiguate."),
            }
        }
    };

    let remote = create_remote_store(&cfg).await?;
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

    let ipfs_store = if do_pin && cfg.backend_type == StorageProtocol::Ipfs {
        Some(crate::object::IpfsObjectStore::new(
            &cfg.effective_endpoint(),
            cfg.auth_token.clone(),
        ))
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

    println!(
        "\nPushed {} snapshot(s) to '{}'{}",
        pushed,
        cfg.name,
        if failed > 0 {
            format!(" ({failed} failed)")
        } else {
            String::new()
        }
    );
    Ok(())
}

async fn push_tree_recursive(
    remote: &dyn ObjectStore,
    local: &crate::object::RedbObjectStore,
    tree_id: &TreeId,
) -> Result<()> {
    match remote.has_tree(tree_id).await {
        Ok(true) => return Ok(()),
        Ok(false) => {}
        Err(e) => tracing::warn!("has_tree check failed for {tree_id}: {e}"),
    }
    let entries = local.get_tree(tree_id).await?;
    remote.put_tree(&entries).await?;
    for entry in &entries.0 {
        match entry.kind {
            crate::object::EntryKind::Blob
            | crate::object::EntryKind::Executable
            | crate::object::EntryKind::Symlink => {
                let blob_id = BlobId(entry.id.clone());
                match remote.has_blob(&blob_id).await {
                    Ok(true) => continue,
                    Ok(false) => {}
                    Err(e) => tracing::warn!("has_blob check failed for {}: {e}", entry.id),
                }
                if let Ok(data) = local.get_blob(&blob_id).await {
                    if let Err(e) = remote.put_blob(&data).await {
                        tracing::warn!("put_blob failed for {}: {e}", entry.id);
                    }
                }
            }
            crate::object::EntryKind::Tree => {
                Box::pin(push_tree_recursive(
                    remote,
                    local,
                    &TreeId(entry.id.clone()),
                ))
                .await?;
            }
            crate::object::EntryKind::Gitlink => {
                // A gitlink id is a git commit oid from another repository,
                // not a noa object: there is nothing to fetch or push. The
                // reference itself already travelled inside the parent tree,
                // which is synced above.
            }
        }
    }
    Ok(())
}

pub async fn run_fetch(repo: &Repository, target: &str, hash_or_cid: &str) -> Result<()> {
    let cfg = repo
        .config
        .get_storage(target)
        .ok_or_else(|| anyhow::anyhow!("storage '{target}' not found"))?
        .clone();
    let local = repo.object_store()?;
    print!("Fetching from '{}'... ", cfg.name);

    let data = if hash_or_cid.starts_with("bafk") || hash_or_cid.starts_with("Qm") {
        if cfg.backend_type != StorageProtocol::Ipfs {
            anyhow::bail!("CID-style fetch requires an IPFS backend");
        }
        let ipfs =
            crate::object::IpfsObjectStore::new(&cfg.effective_endpoint(), cfg.auth_token.clone());
        ipfs.block_get_raw(hash_or_cid).await?
    } else {
        let remote = create_remote_store(&cfg).await?;
        let blob_id = BlobId(hash_or_cid.to_string());
        remote.get_blob(&blob_id).await?
    };

    println!("OK ({} bytes)", data.len());
    println!("  SHA-256: {}", crate::object::sha256_hex(&data));
    local.put_blob(&data).await?;
    println!("  Stored to local object store.");
    Ok(())
}
