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
    Redb(#[from] redb::DatabaseError),

    #[error("database error: {0}")]
    RedbTransaction(#[from] redb::TransactionError),

    #[error("database error: {0}")]
    RedbCommit(#[from] redb::CommitError),

    #[error("database error: {0}")]
    RedbStorage(#[from] redb::StorageError),

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
