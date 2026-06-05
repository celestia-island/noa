use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::error::{NoaError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepoConfig {
    #[serde(default = "default_repo_name")]
    pub name: String,

    #[serde(default)]
    pub remotes: Vec<RemoteConfig>,
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

impl Default for RepoConfig {
    fn default() -> Self {
        RepoConfig {
            name: default_repo_name(),
            remotes: Vec::new(),
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
        let content = std::fs::read_to_string(&config_path)
            .map_err(|e| NoaError::Io(e))?;
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

    pub fn get_remote(&self, name: &str) -> Option<&RemoteConfig> {
        self.remotes.iter().find(|r| r.name == name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
