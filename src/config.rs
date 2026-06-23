use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::error::Result;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TransportMode {
    Vcs,
    Raw,
}

impl Default for TransportMode {
    fn default() -> Self {
        Self::Vcs
    }
}

impl std::fmt::Display for TransportMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vcs => write!(f, "vcs"),
            Self::Raw => write!(f, "raw"),
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TransportProtocol {
    Git,
    Svn,
    S3,
    Minio,
    Ipfs,
    Ftp,
    Ftps,
    Sftp,
}

impl Default for TransportProtocol {
    fn default() -> Self {
        Self::Git
    }
}

impl std::fmt::Display for TransportProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Git => write!(f, "git"),
            Self::Svn => write!(f, "svn"),
            Self::S3 => write!(f, "s3"),
            Self::Minio => write!(f, "minio"),
            Self::Ipfs => write!(f, "ipfs"),
            Self::Ftp => write!(f, "ftp"),
            Self::Ftps => write!(f, "ftps"),
            Self::Sftp => write!(f, "sftp"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    #[serde(default = "default_repo_name")]
    pub name: String,

    #[serde(default)]
    pub transports: Vec<TransportConfig>,

    #[serde(default)]
    pub noa_remote: Option<String>,

    #[serde(default)]
    pub sync: Option<SyncConfig>,
}

fn default_repo_name() -> String {
    "default".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransportConfig {
    pub name: String,

    #[serde(default)]
    pub mode: TransportMode,

    #[serde(rename = "type", default)]
    pub protocol: TransportProtocol,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_key: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_key: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,

    #[serde(default)]
    pub auto_pin: bool,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,

    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,

    #[serde(default)]
    pub port: u16,

    #[serde(default)]
    pub use_tls: bool,
}

impl TransportConfig {
    #[must_use]
    pub fn vcs_git(name: &str, url: &str) -> Self {
        TransportConfig {
            name: name.to_string(),
            mode: TransportMode::Vcs,
            protocol: TransportProtocol::Git,
            url: Some(url.to_string()),
            endpoint: None, bucket: None, access_key: None, secret_key: None,
            region: None, gateway: None, auth_token: None, auto_pin: false,
            username: None, password: None, port: 0, use_tls: false,
        }
    }

    pub fn vcs_svn(name: &str, url: &str) -> Self {
        TransportConfig {
            name: name.to_string(),
            mode: TransportMode::Vcs,
            protocol: TransportProtocol::Svn,
            url: Some(url.to_string()),
            endpoint: None, bucket: None, access_key: None, secret_key: None,
            region: None, gateway: None, auth_token: None, auto_pin: false,
            username: None, password: None, port: 0, use_tls: false,
        }
    }

    pub fn raw_git(name: &str, url: &str) -> Self {
        TransportConfig {
            name: name.to_string(),
            mode: TransportMode::Raw,
            protocol: TransportProtocol::Git,
            url: Some(url.to_string()),
            endpoint: None, bucket: None, access_key: None, secret_key: None,
            region: None, gateway: None, auth_token: None, auto_pin: false,
            username: None, password: None, port: 0, use_tls: false,
        }
    }

    pub fn raw_ipfs(name: &str, endpoint: &str) -> Self {
        TransportConfig {
            name: name.to_string(),
            mode: TransportMode::Raw,
            protocol: TransportProtocol::Ipfs,
            url: None,
            endpoint: Some(endpoint.to_string()),
            bucket: None, access_key: None, secret_key: None, region: None,
            gateway: Some("https://ipfs.io".to_string()),
            auth_token: None, auto_pin: false,
            username: None, password: None, port: 0, use_tls: false,
        }
    }

    pub fn raw_s3(name: &str, endpoint: &str, bucket: &str) -> Self {
        TransportConfig {
            name: name.to_string(),
            mode: TransportMode::Raw,
            protocol: TransportProtocol::S3,
            url: None,
            endpoint: Some(endpoint.to_string()),
            bucket: Some(bucket.to_string()),
            access_key: None, secret_key: None,
            region: Some("us-east-1".to_string()),
            gateway: None, auth_token: None, auto_pin: false,
            username: None, password: None, port: 0, use_tls: false,
        }
    }

    pub fn raw_minio(name: &str, endpoint: &str, bucket: &str) -> Self {
        let mut c = Self::raw_s3(name, endpoint, bucket);
        c.protocol = TransportProtocol::Minio;
        c
    }

    pub fn raw_ftp(name: &str, endpoint: &str) -> Self {
        TransportConfig {
            name: name.to_string(),
            mode: TransportMode::Raw,
            protocol: TransportProtocol::Ftp,
            url: None,
            endpoint: Some(endpoint.to_string()),
            bucket: None, access_key: None, secret_key: None, region: None,
            gateway: None, auth_token: None, auto_pin: false,
            username: None, password: None, port: 21, use_tls: false,
        }
    }

    pub fn raw_ftps(name: &str, endpoint: &str) -> Self {
        let mut c = Self::raw_ftp(name, endpoint);
        c.protocol = TransportProtocol::Ftps;
        c.use_tls = true;
        c
    }

    pub fn raw_sftp(name: &str, endpoint: &str) -> Self {
        TransportConfig {
            name: name.to_string(),
            mode: TransportMode::Raw,
            protocol: TransportProtocol::Sftp,
            url: None,
            endpoint: Some(endpoint.to_string()),
            bucket: None, access_key: None, secret_key: None, region: None,
            gateway: None, auth_token: None, auto_pin: false,
            username: None, password: None, port: 22, use_tls: false,
        }
    }

    #[must_use]
    pub fn effective_endpoint(&self) -> String {
        if let Some(ref ep) = self.endpoint {
            ep.clone()
        } else if let Some(ref url) = self.url {
            url.clone()
        } else {
            String::new()
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyncConfig {
    #[serde(default = "default_sync_socket")]
    pub socket_path: String,

    #[serde(default = "default_sync_interval")]
    pub sync_interval_secs: u64,

    #[serde(default = "default_branch_prefix")]
    pub default_branch_prefix: String,

    #[serde(default)]
    pub auto_gitignore: bool,
}

fn default_sync_socket() -> String {
    let runtime_dir = std::env::var("XDG_RUNTIME_DIR").unwrap_or_else(|_| {
        format!(
            "/tmp/noa-{}",
            std::env::var("USER").unwrap_or_else(|_| "unknown".to_string())
        )
    });
    format!("{runtime_dir}/noa-sync.sock")
}

fn default_sync_interval() -> u64 {
    30
}

fn default_branch_prefix() -> String {
    "agent/".to_string()
}

impl Default for RepoConfig {
    fn default() -> Self {
        RepoConfig {
            name: default_repo_name(),
            transports: Vec::new(),
            noa_remote: None,
            sync: None,
        }
    }
}

impl RepoConfig {
    pub fn to_toml(&self) -> Result<String> {
        Ok(toml::to_string_pretty(self)?)
    }

    pub fn from_toml(s: &str) -> Result<Self> {
        Ok(toml::from_str::<RepoConfig>(s)?)
    }

    pub fn load_from_dir(dir: &Path) -> Result<Self> {
        let config_path = dir.join("config");
        let content = std::fs::read_to_string(&config_path)?;
        Self::from_toml(&content)
    }

    pub fn save_to_dir(&self, dir: &Path) -> Result<()> {
        let config_path = dir.join("config");
        let tmp_path = dir.join("config.tmp");
        let content = self.to_toml()?;
        std::fs::write(&tmp_path, &content)?;
        std::fs::rename(&tmp_path, &config_path)?;
        Ok(())
    }

    pub fn add_transport(&mut self, transport: TransportConfig) {
        self.transports.retain(|t| t.name != transport.name);
        self.transports.push(transport);
    }

    pub fn remove_transport(&mut self, name: &str) {
        self.transports.retain(|t| t.name != name);
    }

    #[must_use]
    pub fn get_transport(&self, name: &str) -> Option<&TransportConfig> {
        self.transports.iter().find(|t| t.name == name)
    }

    pub fn get_transport_mut(&mut self, name: &str) -> Option<&mut TransportConfig> {
        self.transports.iter_mut().find(|t| t.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config_roundtrip() {
        let config = RepoConfig::default();
        let toml_str = config.to_toml().unwrap();
        let parsed = RepoConfig::from_toml(&toml_str).unwrap();
        assert_eq!(parsed.name, config.name);
    }

    #[test]
    fn test_add_remove_transport() {
        let mut config = RepoConfig::default();
        config.add_transport(TransportConfig::vcs_git("origin", "https://github.com/example/repo.git"));
        assert!(config.get_transport("origin").is_some());
        config.remove_transport("origin");
        assert!(config.get_transport("origin").is_none());
    }

    #[test]
    fn test_vcs_git_transport() {
        let t = TransportConfig::vcs_git("github", "https://github.com/user/repo.git");
        assert_eq!(t.mode, TransportMode::Vcs);
        assert_eq!(t.protocol, TransportProtocol::Git);
        assert_eq!(t.url.as_deref(), Some("https://github.com/user/repo.git"));
    }

    #[test]
    fn test_raw_ipfs_transport() {
        let t = TransportConfig::raw_ipfs("ipfs-local", "http://127.0.0.1:5001");
        assert_eq!(t.mode, TransportMode::Raw);
        assert_eq!(t.protocol, TransportProtocol::Ipfs);
        assert_eq!(t.endpoint.as_deref(), Some("http://127.0.0.1:5001"));
    }

    #[test]
    fn test_raw_git_transport() {
        let t = TransportConfig::raw_git("git-raw", "https://github.com/user/noa-objects.git");
        assert_eq!(t.mode, TransportMode::Raw);
        assert_eq!(t.protocol, TransportProtocol::Git);
        assert_eq!(t.url.as_deref(), Some("https://github.com/user/noa-objects.git"));
    }

    #[test]
    fn test_raw_sftp_transport() {
        let t = TransportConfig::raw_sftp("sftp-server", "sftp.example.com");
        assert_eq!(t.mode, TransportMode::Raw);
        assert_eq!(t.protocol, TransportProtocol::Sftp);
        assert_eq!(t.port, 22);
    }

    #[test]
    fn test_raw_ftps_transport() {
        let t = TransportConfig::raw_ftps("ftps-server", "ftps.example.com");
        assert_eq!(t.protocol, TransportProtocol::Ftps);
        assert!(t.use_tls);
    }

    #[test]
    fn test_raw_minio_transport() {
        let t = TransportConfig::raw_minio("minio", "http://localhost:9000", "noa");
        assert_eq!(t.protocol, TransportProtocol::Minio);
    }

    #[test]
    fn test_transport_roundtrip() {
        let mut config = RepoConfig::default();
        config.add_transport(TransportConfig::vcs_git("origin", "https://github.com/repo.git"));
        config.add_transport(TransportConfig::raw_ipfs("ipfs", "http://127.0.0.1:5001"));
        config.add_transport(TransportConfig::raw_s3("s3", "https://s3.example.com", "noa"));
        let toml_str = config.to_toml().unwrap();
        let parsed = RepoConfig::from_toml(&toml_str).unwrap();
        assert_eq!(parsed.transports.len(), 3);
        assert_eq!(parsed.get_transport("origin").unwrap().mode, TransportMode::Vcs);
        assert_eq!(parsed.get_transport("ipfs").unwrap().mode, TransportMode::Raw);
        assert_eq!(parsed.get_transport("s3").unwrap().protocol, TransportProtocol::S3);
    }

    #[test]
    fn test_save_and_load_to_dir() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".noa");
        std::fs::create_dir_all(&dir).unwrap();

        let mut config = RepoConfig::default();
        config.add_transport(TransportConfig::vcs_git("origin", "https://github.com/repo.git"));
        config.save_to_dir(&dir).unwrap();

        let loaded = RepoConfig::load_from_dir(&dir).unwrap();
        assert_eq!(loaded.transports.len(), 1);
        assert_eq!(loaded.transports[0].name, "origin");
    }

    #[test]
    fn test_noa_remote_roundtrip() {
        let config = RepoConfig {
            noa_remote: Some("https://noa.example.com/repo".to_string()),
            ..Default::default()
        };
        let toml_str = config.to_toml().unwrap();
        let parsed = RepoConfig::from_toml(&toml_str).unwrap();
        assert_eq!(parsed.noa_remote, Some("https://noa.example.com/repo".to_string()));
    }

    #[test]
    fn test_sync_config_roundtrip() {
        let config = RepoConfig {
            sync: Some(SyncConfig {
                socket_path: "/tmp/test.sock".to_string(),
                sync_interval_secs: 60,
                default_branch_prefix: "custom/".to_string(),
                auto_gitignore: true,
            }),
            ..Default::default()
        };
        let toml_str = config.to_toml().unwrap();
        let parsed = RepoConfig::from_toml(&toml_str).unwrap();
        assert_eq!(parsed.sync.unwrap().socket_path, "/tmp/test.sock");
    }

    #[test]
    fn test_add_transport_replaces_existing() {
        let mut config = RepoConfig::default();
        config.add_transport(TransportConfig::vcs_git("origin", "https://old.example.com"));
        config.add_transport(TransportConfig::vcs_git("origin", "https://new.example.com"));
        assert_eq!(config.transports.len(), 1);
        assert_eq!(config.get_transport("origin").unwrap().url.as_deref(), Some("https://new.example.com"));
    }

    #[test]
    fn test_load_missing_dir_fails() {
        assert!(RepoConfig::load_from_dir(std::path::Path::new("/nonexistent/path")).is_err());
    }

    #[test]
    fn test_invalid_toml_fails() {
        assert!(RepoConfig::from_toml("this is not valid toml [[[[[").is_err());
    }

    #[test]
    fn test_effective_endpoint() {
        let t1 = TransportConfig::vcs_git("a", "https://github.com/repo.git");
        assert_eq!(t1.effective_endpoint(), "https://github.com/repo.git");

        let t2 = TransportConfig::raw_ipfs("b", "http://127.0.0.1:5001");
        assert_eq!(t2.effective_endpoint(), "http://127.0.0.1:5001");
    }
}
