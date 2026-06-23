use anyhow::Result;

use crate::{config::TransportConfig, repo::Repository};

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
    let mut cfg = match (mode, protocol) {
        ("vcs", "git") => {
            let url = url.ok_or_else(|| anyhow::anyhow!("--url is required for vcs git"))?;
            TransportConfig::vcs_git(name, &url)
        }
        ("vcs", "svn") => {
            let url = url.ok_or_else(|| anyhow::anyhow!("--url is required for vcs svn"))?;
            TransportConfig::vcs_svn(name, &url)
        }
        ("raw", "git") => {
            let url = url.ok_or_else(|| anyhow::anyhow!("--url is required for raw git"))?;
            TransportConfig::raw_git(name, &url)
        }
        ("raw", "ipfs") => {
            let ep = endpoint.unwrap_or_else(|| "http://127.0.0.1:5001".to_string());
            TransportConfig::raw_ipfs(name, &ep)
        }
        ("raw", "s3") | ("raw", "minio") => {
            let ep = endpoint.ok_or_else(|| anyhow::anyhow!("--endpoint is required for s3"))?;
            let bucket = bucket.ok_or_else(|| anyhow::anyhow!("--bucket is required for s3"))?;
            TransportConfig::raw_s3(name, &ep, &bucket)
        }
        ("raw", "ftp") | ("raw", "ftps") => {
            let ep = endpoint.ok_or_else(|| anyhow::anyhow!("--endpoint is required for ftp"))?;
            TransportConfig::raw_ftp(name, &ep)
        }
        ("raw", "sftp") => {
            let ep = endpoint.ok_or_else(|| anyhow::anyhow!("--endpoint is required for sftp"))?;
            TransportConfig::raw_sftp(name, &ep)
        }
        (m, p) => anyhow::bail!("unsupported transport: mode={m}, type={p}"),
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

    let desc = match (cfg.mode.as_str(), cfg.protocol.as_str()) {
        ("vcs", _) => format!("vcs {} -> {}", cfg.protocol, cfg.url.as_deref().unwrap_or("?")),
        ("raw", "git") => format!("raw git backup -> {}", cfg.url.as_deref().unwrap_or("?")),
        ("raw", _) => format!("raw {} -> {}", cfg.protocol, cfg.effective_endpoint()),
        _ => format!("{} {}", cfg.mode, cfg.protocol),
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
        let target = if t.mode == "vcs" {
            t.url.as_deref().unwrap_or("?")
        } else {
            &t.effective_endpoint()
        };
        let extra = match t.protocol.as_str() {
            "ipfs" => format!(", pin={}", if t.auto_pin { "on" } else { "off" }),
            "s3" => format!(", bucket={}", t.bucket.as_deref().unwrap_or("?")),
            "ftp" | "sftp" => format!(", user={}", t.username.as_deref().unwrap_or("?")),
            _ => String::new(),
        };
        println!("{}\t{} ({}/{}){}", t.name, target, t.mode, t.protocol, extra);
    }
    Ok(())
}
