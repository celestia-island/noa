use thiserror::Error;

#[derive(Error, Debug)]
pub enum NoaError {
    #[error("repository not found at {0}")]
    RepoNotFound(String),

    #[error("repository already exists at {0}")]
    RepoAlreadyExists(String),

    #[error("invalid repository: {0}")]
    InvalidRepo(String),

    #[error("object not found: {0}")]
    ObjectNotFound(String),

    #[error("snapshot not found: {0}")]
    SnapshotNotFound(String),

    #[error("workspace not found: {0}")]
    WorkspaceNotFound(String),

    #[error("workspace already exists: {0}")]
    WorkspaceAlreadyExists(String),

    #[error("ref not found: {0}")]
    RefNotFound(String),

    #[error("ref conflict: expected {expected:?}, found {actual:?}")]
    RefConflict {
        expected: Option<String>,
        actual: Option<String>,
    },

    #[error("merge conflict in {path}: {detail}")]
    MergeConflict { path: String, detail: String },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("database error: {0}")]
    Redb(String),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("remote error: {0}")]
    Remote(String),
}

pub type Result<T> = std::result::Result<T, NoaError>;

impl From<serde_json::Error> for NoaError {
    fn from(e: serde_json::Error) -> Self {
        NoaError::Serialization(e.to_string())
    }
}

impl From<rmp_serde::encode::Error> for NoaError {
    fn from(e: rmp_serde::encode::Error) -> Self {
        NoaError::Serialization(e.to_string())
    }
}

impl From<rmp_serde::decode::Error> for NoaError {
    fn from(e: rmp_serde::decode::Error) -> Self {
        NoaError::Serialization(e.to_string())
    }
}

impl From<toml::de::Error> for NoaError {
    fn from(e: toml::de::Error) -> Self {
        NoaError::Config(e.to_string())
    }
}

impl From<toml::ser::Error> for NoaError {
    fn from(e: toml::ser::Error) -> Self {
        NoaError::Config(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_repo_not_found_display() {
        let err = NoaError::RepoNotFound("/path/.noa".to_string());
        assert!(err.to_string().contains("/path/.noa"));
    }

    #[test]
    fn test_repo_already_exists_display() {
        let err = NoaError::RepoAlreadyExists("/path/.noa".to_string());
        assert!(err.to_string().contains("already exists"));
    }

    #[test]
    fn test_invalid_repo_display() {
        let err = NoaError::InvalidRepo("missing noa.redb".to_string());
        assert!(err.to_string().contains("missing noa.redb"));
    }

    #[test]
    fn test_object_not_found_display() {
        let err = NoaError::ObjectNotFound("hash123".to_string());
        assert!(err.to_string().contains("hash123"));
    }

    #[test]
    fn test_snapshot_not_found_display() {
        let err = NoaError::SnapshotNotFound("noa_abc".to_string());
        assert!(err.to_string().contains("noa_abc"));
    }

    #[test]
    fn test_workspace_not_found_display() {
        let err = NoaError::WorkspaceNotFound("feature".to_string());
        assert!(err.to_string().contains("feature"));
    }

    #[test]
    fn test_workspace_already_exists_display() {
        let err = NoaError::WorkspaceAlreadyExists("feature".to_string());
        assert!(err.to_string().contains("feature"));
    }

    #[test]
    fn test_ref_conflict_display() {
        let err = NoaError::RefConflict {
            expected: Some("noa_a".to_string()),
            actual: Some("noa_b".to_string()),
        };
        let msg = err.to_string();
        assert!(msg.contains("noa_a"));
        assert!(msg.contains("noa_b"));
    }

    #[test]
    fn test_merge_conflict_display() {
        let err = NoaError::MergeConflict {
            path: "src/main.rs".to_string(),
            detail: "both modified".to_string(),
        };
        let msg = err.to_string();
        assert!(msg.contains("src/main.rs"));
        assert!(msg.contains("both modified"));
    }

    #[test]
    fn test_io_error_from() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let noa_err: NoaError = io_err.into();
        assert!(matches!(noa_err, NoaError::Io(_)));
    }

    #[test]
    fn test_serde_json_error_conversion() {
        let json_err = serde_json::from_str::<i32>("not a number").unwrap_err();
        let noa_err: NoaError = json_err.into();
        assert!(matches!(noa_err, NoaError::Serialization(_)));
    }

    #[test]
    fn test_redb_error_display() {
        let err = NoaError::Redb("write failed".to_string());
        assert!(err.to_string().contains("write failed"));
    }

    #[test]
    fn test_remote_error_display() {
        let err = NoaError::Remote("connection refused".to_string());
        assert!(err.to_string().contains("connection refused"));
    }

    #[test]
    fn test_config_error_display() {
        let err = NoaError::Config("invalid toml".to_string());
        assert!(err.to_string().contains("invalid toml"));
    }
}
