use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::error::Result;
use crate::forge::ForgeConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum RemoteProtocol {
    #[default]
    Git,
    Svn,
}

impl std::fmt::Display for RemoteProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Git => write!(f, "git"),
            Self::Svn => write!(f, "svn"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum StorageProtocol {
    #[default]
    Ipfs,
    S3,
    Minio,
    Ftp,
    Ftps,
    Sftp,
}

impl std::fmt::Display for StorageProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Ipfs => write!(f, "ipfs"),
            Self::S3 => write!(f, "s3"),
            Self::Minio => write!(f, "minio"),
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
    pub remotes: Vec<RemoteConfig>,
    #[serde(default)]
    pub noa_remote: Option<String>,
    #[serde(default)]
    pub sync: Option<SyncConfig>,
    #[serde(default)]
    pub storage: Vec<StorageConfig>,
}

fn default_repo_name() -> String {
    "default".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteConfig {
    pub name: String,
    pub url: String,
    #[serde(default)]
    pub protocol: RemoteProtocol,
    /// Optional per-remote PR/forge configuration (`[remote.<name>.pr]`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr: Option<ForgeConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageConfig {
    pub name: String,
    #[serde(rename = "type", default)]
    pub backend_type: StorageProtocol,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub endpoint: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auth_token: Option<String>,
    #[serde(default)]
    pub auto_pin: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bucket: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub access_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secret_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub password: Option<String>,
    #[serde(default)]
    pub port: u16,
    #[serde(default)]
    pub use_tls: bool,
}

impl StorageConfig {
    #[must_use]
    pub fn ipfs(name: &str, endpoint: Option<&str>) -> Self {
        StorageConfig {
            name: name.to_string(),
            backend_type: StorageProtocol::Ipfs,
            endpoint: endpoint.map(|s| s.to_string()),
            gateway: Some("https://ipfs.io".to_string()),
            auth_token: None,
            auto_pin: false,
            bucket: None,
            access_key: None,
            secret_key: None,
            region: None,
            username: None,
            password: None,
            port: 0,
            use_tls: false,
        }
    }
    #[must_use]
    pub fn s3(name: &str, endpoint: Option<&str>, bucket: &str) -> Self {
        StorageConfig {
            name: name.to_string(),
            backend_type: StorageProtocol::S3,
            endpoint: endpoint.map(|s| s.to_string()),
            bucket: Some(bucket.to_string()),
            gateway: None,
            auth_token: None,
            auto_pin: false,
            access_key: None,
            secret_key: None,
            region: None,
            username: None,
            password: None,
            port: 0,
            use_tls: false,
        }
    }
    #[must_use]
    pub fn minio(name: &str, endpoint: Option<&str>, bucket: &str) -> Self {
        let mut c = Self::s3(name, endpoint, bucket);
        c.backend_type = StorageProtocol::Minio;
        c
    }
    #[must_use]
    pub fn ftp(name: &str, endpoint: &str) -> Self {
        StorageConfig {
            name: name.to_string(),
            backend_type: StorageProtocol::Ftp,
            endpoint: Some(endpoint.to_string()),
            port: 21,
            gateway: None,
            auth_token: None,
            auto_pin: false,
            bucket: None,
            access_key: None,
            secret_key: None,
            region: None,
            username: None,
            password: None,
            use_tls: false,
        }
    }
    #[must_use]
    pub fn ftps(name: &str, endpoint: &str) -> Self {
        let mut c = Self::ftp(name, endpoint);
        c.backend_type = StorageProtocol::Ftps;
        c.use_tls = true;
        c
    }
    #[must_use]
    pub fn sftp(name: &str, endpoint: &str) -> Self {
        StorageConfig {
            name: name.to_string(),
            backend_type: StorageProtocol::Sftp,
            endpoint: Some(endpoint.to_string()),
            port: 22,
            gateway: None,
            auth_token: None,
            auto_pin: false,
            bucket: None,
            access_key: None,
            secret_key: None,
            region: None,
            username: None,
            password: None,
            use_tls: false,
        }
    }
    #[must_use]
    pub fn effective_endpoint(&self) -> String {
        self.endpoint.clone().unwrap_or_default()
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
            remotes: vec![],
            noa_remote: None,
            sync: None,
            storage: vec![],
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
        let content = std::fs::read_to_string(dir.join("config"))?;
        Self::from_toml(&content)
    }
    pub fn save_to_dir(&self, dir: &Path) -> Result<()> {
        let config_path = dir.join("config");
        let tmp_path = dir.join("config.tmp");
        std::fs::write(&tmp_path, &self.to_toml()?)?;
        std::fs::rename(&tmp_path, &config_path)?;
        Ok(())
    }
    pub fn add_remote(&mut self, remote: RemoteConfig) {
        self.remotes.retain(|r| r.name != remote.name);
        self.remotes.push(remote);
    }
    pub fn remove_remote(&mut self, name: &str) {
        self.remotes.retain(|r| r.name != name);
    }
    #[must_use]
    pub fn get_remote(&self, name: &str) -> Option<&RemoteConfig> {
        self.remotes.iter().find(|r| r.name == name)
    }
    pub fn add_storage(&mut self, s: StorageConfig) {
        self.storage.retain(|x| x.name != s.name);
        self.storage.push(s);
    }
    pub fn remove_storage(&mut self, name: &str) {
        self.storage.retain(|x| x.name != name);
    }
    #[must_use]
    pub fn get_storage(&self, name: &str) -> Option<&StorageConfig> {
        self.storage.iter().find(|s| s.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_roundtrip() {
        let config = RepoConfig::default();
        let s = config.to_toml().unwrap();
        let parsed = RepoConfig::from_toml(&s).unwrap();
        assert_eq!(parsed.name, "default");
    }
    #[test]
    fn test_add_remove_remote() {
        let mut c = RepoConfig::default();
        c.add_remote(RemoteConfig {
            name: "o".to_string(),
            url: "u".to_string(),
            protocol: RemoteProtocol::Git,
            pr: None,
        });
        assert!(c.get_remote("o").is_some());
        c.remove_remote("o");
        assert!(c.get_remote("o").is_none());
    }
    #[test]
    fn test_save_and_load() {
        let tmp = TempDir::new().unwrap();
        let d = tmp.path().join(".noa");
        std::fs::create_dir_all(&d).unwrap();
        let mut c = RepoConfig::default();
        c.add_remote(RemoteConfig {
            name: "o".to_string(),
            url: "https://g.com/r.git".to_string(),
            protocol: RemoteProtocol::Git,
            pr: None,
        });
        c.add_storage(StorageConfig::ipfs("i", Some("http://127.0.0.1:5001")));
        c.save_to_dir(&d).unwrap();
        let l = RepoConfig::load_from_dir(&d).unwrap();
        assert_eq!(l.remotes.len(), 1);
        assert_eq!(l.storage.len(), 1);
    }
    #[test]
    fn test_storage_constructors() {
        assert_eq!(
            StorageConfig::ipfs("a", Some("e")).backend_type,
            StorageProtocol::Ipfs
        );
        assert_eq!(
            StorageConfig::s3("a", Some("e"), "b").backend_type,
            StorageProtocol::S3
        );
        assert_eq!(
            StorageConfig::ftp("a", "e").backend_type,
            StorageProtocol::Ftp
        );
        assert_eq!(
            StorageConfig::ftps("a", "e").backend_type,
            StorageProtocol::Ftps
        );
        assert!(StorageConfig::ftps("a", "e").use_tls);
        assert_eq!(
            StorageConfig::sftp("a", "e").backend_type,
            StorageProtocol::Sftp
        );
        assert_eq!(
            StorageConfig::minio("a", Some("e"), "b").backend_type,
            StorageProtocol::Minio
        );
    }

    #[test]
    fn test_remote_pr_config_roundtrip() {
        let mut c = RepoConfig::default();
        c.add_remote(RemoteConfig {
            name: "origin".to_string(),
            url: "git@github.com:celestia-island/noa.git".to_string(),
            protocol: RemoteProtocol::Git,
            pr: Some(crate::forge::ForgeConfig {
                kind: crate::forge::ForgeKind::Github,
                base_url: Some("https://api.github.com".to_string()),
                token_env: Some("GH_TOKEN".to_string()),
                repo: None,
            }),
        });
        let s = c.to_toml().unwrap();
        assert!(
            s.contains("[[remotes]]"),
            "toml missing remotes array:\n{s}"
        );
        assert!(
            s.contains("kind = \"github\""),
            "toml missing pr kind:\n{s}"
        );
        let parsed = RepoConfig::from_toml(&s).unwrap();
        let pr = parsed.get_remote("origin").unwrap().pr.as_ref().unwrap();
        assert_eq!(pr.kind, crate::forge::ForgeKind::Github);
        assert_eq!(pr.base_url.as_deref(), Some("https://api.github.com"));
        assert_eq!(pr.token_env.as_deref(), Some("GH_TOKEN"));
    }
}
