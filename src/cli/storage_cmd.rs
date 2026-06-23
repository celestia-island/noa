use anyhow::Result;

use crate::{
    config::StorageConfig,
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
    let endpoint = match opts.endpoint {
        Some(ep) => ep,
        None => match backend_type {
            "ipfs" => "http://127.0.0.1:5001".to_string(),
            other => anyhow::bail!("--endpoint is required for '{other}' backend"),
        },
    };

    let mut cfg = match backend_type {
        "ipfs" => StorageConfig::ipfs(name, &endpoint),
        "s3" | "minio" => {
            let bucket = opts
                .bucket
                .ok_or_else(|| anyhow::anyhow!("--bucket is required for S3 backends"))?;
            StorageConfig::s3(name, &endpoint, &bucket)
        }
        "ftp" | "ftps" => StorageConfig::ftp(name, &endpoint),
        other => anyhow::bail!("unknown backend type '{other}': expected 'ipfs', 's3', or 'ftp'"),
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

    let extra = match cfg.backend_type.as_str() {
        "ftp" | "ftps" => {
            format!("user={}", cfg.username.as_deref().unwrap_or("?"))
        }
        "ipfs" => format!("gateway={}", cfg.gateway.as_deref().unwrap_or("default")),
        _ => String::new(),
    };
    println!(
        "Added storage '{}' (type: {}, endpoint: {}, {})",
        cfg.name, cfg.backend_type, cfg.endpoint, extra
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
        println!("Run 'noa storage add <name> --type ipfs', '--type s3', or '--type ftp'.");
        return Ok(());
    }

    for s in &repo.config.storage {
        let extra = match s.backend_type.as_str() {
            "ipfs" => format!("gateway={}", s.gateway.as_deref().unwrap_or("default")),
            "s3" | "minio" => format!("bucket={}", s.bucket.as_deref().unwrap_or("?")),
            "ftp" | "ftps" => {
                let tls = if s.use_tls { "+tls" } else { "" };
                format!("user={}{}", s.username.as_deref().unwrap_or("?"), tls)
            }
            other => other.to_string(),
        };
        println!(
            "{}\t{} ({}) [{}]",
            s.name, s.endpoint, s.backend_type, extra
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
        match cfg.backend_type.as_str() {
            "ipfs" => {
                let store =
                    crate::object::IpfsObjectStore::new(&cfg.endpoint, cfg.auth_token.clone());
                print!("  Connecting to {}... ", cfg.endpoint);
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
            "s3" | "minio" => {
                println!("  Endpoint: {}", cfg.endpoint);
                println!("  Bucket:   {}", cfg.bucket.as_deref().unwrap_or("?"));
                println!(
                    "  Region:   {}",
                    cfg.region.as_deref().unwrap_or("us-east-1")
                );
                match create_remote_store(cfg).await {
                    Ok(_) => println!("  Connection: OK"),
                    Err(e) => println!("  Connection: FAILED ({e})"),
                }
            }
            "ftp" | "ftps" => {
                println!("  Endpoint: {}", cfg.endpoint);
                println!("  Port:     {}", if cfg.port > 0 { cfg.port } else { 21 });
                println!("  User:     {}", cfg.username.as_deref().unwrap_or("?"));
                println!("  TLS:      {}", if cfg.use_tls { "yes" } else { "no" });
                #[cfg(feature = "ftp")]
                match create_remote_store(cfg).await {
                    Ok(_) => println!("  Connection: OK"),
                    Err(e) => println!("  Connection: FAILED ({e})"),
                }
                #[cfg(not(feature = "ftp"))]
                println!("  Connection: N/A (FTP feature not compiled)");
            }
            other => println!("  Unknown backend type: {other}"),
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
            let candidates: Vec<_> = repo
                .config
                .storage
                .iter()
                .filter(|s| s.auto_pin || do_pin)
                .collect();
            match candidates.len() {
                0 => anyhow::bail!(
                    "no storage target specified. Use --target <name> or configure auto-pin."
                ),
                1 => candidates[0].clone(),
                _ => anyhow::bail!(
                    "multiple candidates found. Specify --target <name> to disambiguate."
                ),
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

    let ipfs_store = if do_pin && cfg.backend_type == "ipfs" {
        Some(crate::object::IpfsObjectStore::new(
            &cfg.endpoint,
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
    if remote.has_tree(tree_id).await.unwrap_or(false) {
        return Ok(());
    }

    let entries = local.get_tree(tree_id).await?;
    let _ = remote.put_tree(&entries).await;

    for entry in &entries.0 {
        match entry.kind {
            crate::object::EntryKind::Blob => {
                let blob_id = BlobId(entry.id.clone());
                if !remote.has_blob(&blob_id).await.unwrap_or(false) {
                    if let Ok(data) = local.get_blob(&blob_id).await {
                        let _ = remote.put_blob(&data).await;
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
        if cfg.backend_type != "ipfs" {
            anyhow::bail!("CID-style fetch requires an IPFS backend");
        }
        let ipfs = crate::object::IpfsObjectStore::new(&cfg.endpoint, cfg.auth_token.clone());
        ipfs.block_get_raw(hash_or_cid).await?
    } else {
        let remote = create_remote_store(&cfg).await?;
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
