use anyhow::Result;

use crate::{
    config::{TransportConfig, TransportMode, TransportProtocol},
    repo::Repository,
};

pub fn run_add(
    repo: &mut Repository,
    name: &str,
    mode: &str,
    protocol: &str,
    url: Option<String>,
    endpoint: Option<String>,
    gateway: Option<String>,
    auth_token: Option<String>,
    auto_pin: bool,
    bucket: Option<String>,
    access_key: Option<String>,
    secret_key: Option<String>,
    region: Option<String>,
    username: Option<String>,
    password: Option<String>,
    port: u16,
    use_tls: bool,
) -> Result<()> {
    let mode = match mode {
        "vcs" => TransportMode::Vcs,
        "raw" => TransportMode::Raw,
        other => anyhow::bail!("unknown mode '{other}': expected 'vcs' or 'raw'"),
    };
    let protocol = match protocol {
        "git" => TransportProtocol::Git,
        "svn" => TransportProtocol::Svn,
        "s3" => TransportProtocol::S3,
        "minio" => TransportProtocol::Minio,
        "ipfs" => TransportProtocol::Ipfs,
        "ftp" => TransportProtocol::Ftp,
        "ftps" => TransportProtocol::Ftps,
        "sftp" => TransportProtocol::Sftp,
        other => anyhow::bail!("unknown type '{other}': expected git, svn, s3, minio, ipfs, ftp, ftps, or sftp"),
    };

    let mut cfg = match (mode, protocol) {
        (TransportMode::Vcs, TransportProtocol::Git) => {
            let url = url.ok_or_else(|| anyhow::anyhow!("--url is required for vcs git"))?;
            TransportConfig::vcs_git(name, &url)
        }
        (TransportMode::Vcs, TransportProtocol::Svn) => {
            let url = url.ok_or_else(|| anyhow::anyhow!("--url is required for vcs svn"))?;
            TransportConfig::vcs_svn(name, &url)
        }
        (TransportMode::Raw, TransportProtocol::Git) => {
            let url = url.ok_or_else(|| anyhow::anyhow!("--url is required for raw git"))?;
            TransportConfig::raw_git(name, &url)
        }
        (TransportMode::Raw, TransportProtocol::Ipfs) => {
            let ep = endpoint.unwrap_or_else(|| "http://127.0.0.1:5001".to_string());
            TransportConfig::raw_ipfs(name, &ep)
        }
        (TransportMode::Raw, TransportProtocol::S3) => {
            let ep = endpoint.ok_or_else(|| anyhow::anyhow!("--endpoint is required for s3"))?;
            let bucket = bucket.ok_or_else(|| anyhow::anyhow!("--bucket is required for s3"))?;
            TransportConfig::raw_s3(name, &ep, &bucket)
        }
        (TransportMode::Raw, TransportProtocol::Minio) => {
            let ep = endpoint.ok_or_else(|| anyhow::anyhow!("--endpoint is required for minio"))?;
            let bucket = bucket.ok_or_else(|| anyhow::anyhow!("--bucket is required for minio"))?;
            TransportConfig::raw_minio(name, &ep, &bucket)
        }
        (TransportMode::Raw, TransportProtocol::Ftp) => {
            let ep = endpoint.ok_or_else(|| anyhow::anyhow!("--endpoint is required for ftp"))?;
            TransportConfig::raw_ftp(name, &ep)
        }
        (TransportMode::Raw, TransportProtocol::Ftps) => {
            let ep = endpoint.ok_or_else(|| anyhow::anyhow!("--endpoint is required for ftps"))?;
            TransportConfig::raw_ftps(name, &ep)
        }
        (TransportMode::Raw, TransportProtocol::Sftp) => {
            let ep = endpoint.ok_or_else(|| anyhow::anyhow!("--endpoint is required for sftp"))?;
            TransportConfig::raw_sftp(name, &ep)
        }
        (m, p) => anyhow::bail!("unsupported combination: mode={m}, type={p}"),
    };

    if let Some(gw) = gateway { cfg.gateway = Some(gw); }
    if let Some(tok) = auth_token { cfg.auth_token = Some(tok); }
    cfg.auto_pin = auto_pin;
    if let Some(ak) = access_key { cfg.access_key = Some(ak); }
    if let Some(sk) = secret_key { cfg.secret_key = Some(sk); }
    if let Some(r) = region { cfg.region = Some(r); }
    if let Some(u) = username { cfg.username = Some(u); }
    if let Some(p) = password { cfg.password = Some(p); }
    if port > 0 { cfg.port = port; }
    cfg.use_tls = use_tls;

    let desc = match (&cfg.mode, &cfg.protocol) {
        (TransportMode::Vcs, _) => format!("vcs {} -> {}", cfg.protocol, cfg.url.as_deref().unwrap_or("?")),
        (TransportMode::Raw, TransportProtocol::Git) => format!("raw git backup -> {}", cfg.url.as_deref().unwrap_or("?")),
        (TransportMode::Raw, _) => format!("raw {} -> {}", cfg.protocol, cfg.effective_endpoint()),
    };
    println!("Added transport '{}' ({})", cfg.name, desc);

    repo.config.add_transport(cfg);
    repo.save_config()?;
    Ok(())
}

pub fn run_remove(repo: &mut Repository, name: &str) -> Result<()> {
    if repo.config.get_transport(name).is_none() {
        anyhow::bail!("transport '{name}' not found");
    }
    repo.config.remove_transport(name);
    repo.save_config()?;
    println!("Removed transport '{name}'");
    Ok(())
}

pub fn run_list(repo: &Repository) -> Result<()> {
    if repo.config.transports.is_empty() {
        println!("No transports configured.");
        println!("Examples:");
        println!("  noa transport add github --mode vcs --type git --url https://github.com/user/repo.git");
        println!("  noa transport add ipfs   --mode raw --type ipfs --endpoint http://127.0.0.1:5001");
        println!("  noa transport add s3     --mode raw --type s3 --endpoint ... --bucket ...");
        println!("  noa transport add sftp   --mode raw --type sftp --endpoint sftp.example.com --username ...");
        return Ok(());
    }

    for t in &repo.config.transports {
        let target = if t.mode == TransportMode::Vcs {
            t.url.as_deref().unwrap_or("?")
        } else {
            &t.effective_endpoint()
        };
        let extra = match t.protocol {
            TransportProtocol::Ipfs => format!(", pin={}", if t.auto_pin { "on" } else { "off" }),
            TransportProtocol::S3 | TransportProtocol::Minio => format!(", bucket={}", t.bucket.as_deref().unwrap_or("?")),
            TransportProtocol::Ftp | TransportProtocol::Ftps | TransportProtocol::Sftp => format!(", user={}", t.username.as_deref().unwrap_or("?")),
            _ => String::new(),
        };
        println!("{}\t{} ({}/{}){}", t.name, target, t.mode, t.protocol, extra);
    }
    Ok(())
}
