use crate::config::TransportConfig;

/// Unified credential handling for command-based transports.
///
/// Extracts authentication information from [`TransportConfig`] and provides
/// methods to inject credentials into git, svn, and other CLI operations.
/// This is the "general facility" that all command-based backends share.
pub struct Credentials {
    pub username: Option<String>,
    pub password: Option<String>,
    pub token: Option<String>,
}

impl Credentials {
    pub fn from_config(config: &TransportConfig) -> Self {
        Self {
            username: config.username.clone(),
            password: config.password.clone(),
            token: config.auth_token.clone(),
        }
    }

    #[must_use]
    pub fn none() -> Self {
        Self {
            username: None,
            password: None,
            token: None,
        }
    }

    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.username.is_some() || self.password.is_some() || self.token.is_some()
    }

    fn secret(&self) -> Option<&str> {
        self.password.as_deref().or(self.token.as_deref())
    }

    /// Inject credentials into an HTTPS URL for non-interactive git operations.
    ///
    /// For HTTPS URLs: embeds `user:secret@` into the URL.
    /// For SSH / git / file URLs: returned unchanged (SSH agent handles auth).
    pub fn for_git_url(&self, url: &str) -> String {
        if !url.starts_with("https://") && !url.starts_with("http://") {
            return url.to_string();
        }
        let Some(secret) = self.secret() else {
            return url.to_string();
        };
        let user = self.username.as_deref().unwrap_or("git");

        if let Some(rest) = url.strip_prefix("https://") {
            format!("https://{user}:{secret}@{rest}")
        } else if let Some(rest) = url.strip_prefix("http://") {
            format!("http://{user}:{secret}@{rest}")
        } else {
            url.to_string()
        }
    }

    /// Returns CLI arguments for svn commands (`--username`, `--password`).
    pub fn svn_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(user) = &self.username {
            args.push("--username".to_string());
            args.push(user.clone());
        }
        if let Some(pass) = self.secret() {
            args.push("--password".to_string());
            args.push(pass.to_string());
            args.push("--non-interactive".to_string());
        }
        args
    }

    /// Returns environment variables for git credential operations.
    /// Sets `GIT_TERMINAL_PROMPT=0` to prevent interactive prompts.
    pub fn git_env(&self) -> Vec<(&'static str, String)> {
        let mut env = vec![("GIT_TERMINAL_PROMPT", "0".to_string())];
        if self.username.is_some() || self.secret().is_some() {
            env.push(("GIT_HTTP_USER", self.username.clone().unwrap_or_default()));
            env.push(("GIT_HTTP_PASSWORD", self.secret().unwrap_or("").to_string()));
        }
        env
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_credentials() {
        let creds = Credentials::none();
        assert!(!creds.has_credentials());
        assert_eq!(creds.for_git_url("https://github.com/repo.git"), "https://github.com/repo.git");
    }

    #[test]
    fn test_https_injection() {
        let creds = Credentials {
            username: Some("user".to_string()),
            password: Some("secret".to_string()),
            token: None,
        };
        assert_eq!(
            creds.for_git_url("https://github.com/repo.git"),
            "https://user:secret@github.com/repo.git"
        );
    }

    #[test]
    fn test_token_as_secret() {
        let creds = Credentials {
            username: Some("oauth2".to_string()),
            password: None,
            token: Some("ghp_xxxx".to_string()),
        };
        assert_eq!(
            creds.for_git_url("https://github.com/repo.git"),
            "https://oauth2:ghp_xxxx@github.com/repo.git"
        );
    }

    #[test]
    fn test_ssh_url_unchanged() {
        let creds = Credentials {
            username: Some("user".to_string()),
            password: Some("secret".to_string()),
            token: None,
        };
        assert_eq!(
            creds.for_git_url("git@github.com:user/repo.git"),
            "git@github.com:user/repo.git"
        );
    }

    #[test]
    fn test_svn_args() {
        let creds = Credentials {
            username: Some("user".to_string()),
            password: Some("pass".to_string()),
            token: None,
        };
        let args = creds.svn_args();
        assert!(args.contains(&"--username".to_string()));
        assert!(args.contains(&"user".to_string()));
        assert!(args.contains(&"--password".to_string()));
        assert!(args.contains(&"--non-interactive".to_string()));
    }

    #[test]
    fn test_from_config() {
        let config = TransportConfig::raw_git("test", "https://github.com/repo.git");
        let creds = Credentials::from_config(&config);
        assert!(!creds.has_credentials());

        let config = TransportConfig {
            username: Some("user".to_string()),
            password: Some("pass".to_string()),
            ..TransportConfig::raw_git("test", "https://github.com/repo.git")
        };
        let creds = Credentials::from_config(&config);
        assert!(creds.has_credentials());
        assert_eq!(creds.for_git_url("https://github.com/repo.git"), "https://user:pass@github.com/repo.git");
    }
}
