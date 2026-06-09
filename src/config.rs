use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::error::{NoaError, Result};

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
}

fn default_repo_name() -> String {
    "default".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteConfig {
    pub name: String,
    pub url: String,
    #[serde(default = "default_protocol")]
    pub protocol: String,
}

fn default_protocol() -> String {
    "git".to_string()
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
    "entelecheia/agent-".to_string()
}

impl Default for RepoConfig {
    fn default() -> Self {
        RepoConfig {
            name: default_repo_name(),
            remotes: Vec::new(),
            noa_remote: None,
            sync: None,
        }
    }
}

impl RepoConfig {
    pub fn to_toml(&self) -> Result<String> {
        toml::to_string_pretty(self).map_err(NoaError::from)
    }

    pub fn from_toml(s: &str) -> Result<Self> {
        toml::from_str(s).map_err(NoaError::from)
    }

    pub fn load_from_dir(dir: &Path) -> Result<Self> {
        let config_path = dir.join("config");
        let content = std::fs::read_to_string(&config_path).map_err(NoaError::Io)?;
        Self::from_toml(&content)
    }

    pub fn save_to_dir(&self, dir: &Path) -> Result<()> {
        let config_path = dir.join("config");
        let content = self.to_toml()?;
        std::fs::write(&config_path, content).map_err(NoaError::Io)
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
    fn test_add_remove_remote() {
        let mut config = RepoConfig::default();
        config.add_remote(RemoteConfig {
            name: "origin".to_string(),
            url: "https://github.com/example/repo.git".to_string(),
            protocol: "git".to_string(),
        });
        assert!(config.get_remote("origin").is_some());
        config.remove_remote("origin");
        assert!(config.get_remote("origin").is_none());
    }

    #[test]
    fn test_noa_remote_roundtrip() {
        let config = RepoConfig {
            noa_remote: Some("https://noa.example.com/repo".to_string()),
            ..Default::default()
        };
        let toml_str = config.to_toml().unwrap();
        let parsed = RepoConfig::from_toml(&toml_str).unwrap();
        assert_eq!(
            parsed.noa_remote,
            Some("https://noa.example.com/repo".to_string())
        );
    }

    #[test]
    fn test_noa_remote_none_by_default() {
        let config = RepoConfig::default();
        assert!(config.noa_remote.is_none());
    }

    #[test]
    fn test_save_and_load_to_dir() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".noa");
        std::fs::create_dir_all(&dir).unwrap();

        let mut config = RepoConfig::default();
        config.add_remote(RemoteConfig {
            name: "origin".to_string(),
            url: "https://github.com/example/repo.git".to_string(),
            protocol: "git".to_string(),
        });
        config.noa_remote = Some("https://noa.host/repo".to_string());
        config.save_to_dir(&dir).unwrap();

        let loaded = RepoConfig::load_from_dir(&dir).unwrap();
        assert_eq!(loaded.name, "default");
        assert_eq!(loaded.remotes.len(), 1);
        assert_eq!(loaded.remotes[0].name, "origin");
        assert_eq!(loaded.noa_remote, Some("https://noa.host/repo".to_string()));
    }

    #[test]
    fn test_add_remote_replaces_existing() {
        let mut config = RepoConfig::default();
        config.add_remote(RemoteConfig {
            name: "origin".to_string(),
            url: "https://old.example.com".to_string(),
            protocol: "git".to_string(),
        });
        config.add_remote(RemoteConfig {
            name: "origin".to_string(),
            url: "https://new.example.com".to_string(),
            protocol: "git".to_string(),
        });
        assert_eq!(config.remotes.len(), 1);
        assert_eq!(
            config.get_remote("origin").unwrap().url,
            "https://new.example.com"
        );
    }

    #[test]
    fn test_multiple_remotes() {
        let mut config = RepoConfig::default();
        config.add_remote(RemoteConfig {
            name: "origin".to_string(),
            url: "https://github.com/repo.git".to_string(),
            protocol: "git".to_string(),
        });
        config.add_remote(RemoteConfig {
            name: "upstream".to_string(),
            url: "https://gitlab.com/repo.git".to_string(),
            protocol: "git".to_string(),
        });
        assert_eq!(config.remotes.len(), 2);
        assert!(config.get_remote("origin").is_some());
        assert!(config.get_remote("upstream").is_some());
    }

    #[test]
    fn test_load_missing_dir_fails() {
        let result = RepoConfig::load_from_dir(std::path::Path::new("/nonexistent/path"));
        assert!(result.is_err());
    }

    #[test]
    fn test_remote_config_default_protocol() {
        let proto = default_protocol();
        assert_eq!(proto, "git");
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
        let pc = parsed.sync.unwrap();
        assert_eq!(pc.socket_path, "/tmp/test.sock");
        assert_eq!(pc.sync_interval_secs, 60);
        assert_eq!(pc.default_branch_prefix, "custom/");
        assert!(pc.auto_gitignore);
    }

    #[test]
    fn test_sync_config_none_by_default() {
        let config = RepoConfig::default();
        assert!(config.sync.is_none());
    }

    #[test]
    fn test_default_sync_socket_path() {
        let socket = default_sync_socket();
        assert!(socket.contains("noa-sync.sock"));
    }

    #[test]
    fn test_default_sync_interval() {
        assert_eq!(default_sync_interval(), 30);
    }

    #[test]
    fn test_default_branch_prefix() {
        assert_eq!(default_branch_prefix(), "entelecheia/agent-");
    }

    #[test]
    fn test_sync_config_partial_defaults() {
        let toml_str = r#"
name = "myrepo"
[sync]
socket_path = "/custom/path.sock"
"#;
        let config = RepoConfig::from_toml(toml_str).unwrap();
        let sync = config.sync.unwrap();
        assert_eq!(sync.socket_path, "/custom/path.sock");
        assert_eq!(sync.sync_interval_secs, 30);
        assert_eq!(sync.default_branch_prefix, "entelecheia/agent-");
        assert!(!sync.auto_gitignore);
    }

    #[test]
    fn test_config_with_noa_remote_url() {
        let toml_str = r#"
name = "test"
noa_remote = "https://noa.cloud/repo"
"#;
        let config = RepoConfig::from_toml(toml_str).unwrap();
        assert_eq!(
            config.noa_remote,
            Some("https://noa.cloud/repo".to_string())
        );
    }

    #[test]
    fn test_remove_nonexistent_remote_is_noop() {
        let mut config = RepoConfig::default();
        config.add_remote(RemoteConfig {
            name: "origin".to_string(),
            url: "https://example.com".to_string(),
            protocol: "git".to_string(),
        });
        config.remove_remote("nonexistent");
        assert_eq!(config.remotes.len(), 1);
    }

    #[test]
    fn test_get_nonexistent_remote() {
        let config = RepoConfig::default();
        assert!(config.get_remote("nonexistent").is_none());
    }

    #[test]
    fn test_invalid_toml_fails() {
        let result = RepoConfig::from_toml("this is not valid toml [[[[");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_corrupted_config_fails() {
        let tmp = TempDir::new().unwrap();
        let dir = tmp.path().join(".noa");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("config"), "not valid toml {{{{").unwrap();
        let result = RepoConfig::load_from_dir(&dir);
        assert!(result.is_err());
    }
}
