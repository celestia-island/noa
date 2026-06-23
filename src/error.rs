pub use anyhow::Result;

#[derive(Debug, thiserror::Error)]
pub enum NoaError {
    #[error("object not found: {id}")]
    ObjectNotFound { id: String },

    #[error("snapshot not found: {id}")]
    SnapshotNotFound { id: String },

    #[error("workspace already exists: {name}")]
    WorkspaceAlreadyExists { name: String },

    #[error("workspace not found: {name}")]
    WorkspaceNotFound { name: String },

    #[error("repository already exists at {path}")]
    RepositoryAlreadyExists { path: String },

    #[error("repository not found at {path}")]
    RepositoryNotFound { path: String },

    #[error("IPFS error: {message}")]
    IpfsError { message: String },

    #[error("IPFS daemon unreachable: {endpoint}")]
    IpfsDaemonUnreachable { endpoint: String },

    #[error("invalid CID: {cid}")]
    InvalidCid { cid: String },
}

pub fn is_object_not_found(e: &anyhow::Error) -> bool {
    e.downcast_ref::<NoaError>()
        .is_some_and(|ne| matches!(ne, NoaError::ObjectNotFound { .. }))
}

pub fn is_workspace_already_exists(e: &anyhow::Error) -> bool {
    e.downcast_ref::<NoaError>()
        .is_some_and(|ne| matches!(ne, NoaError::WorkspaceAlreadyExists { .. }))
}

pub fn is_snapshot_not_found(e: &anyhow::Error) -> bool {
    e.downcast_ref::<NoaError>()
        .is_some_and(|ne| matches!(ne, NoaError::SnapshotNotFound { .. }))
}

pub fn is_eof_error(e: &anyhow::Error) -> bool {
    e.downcast_ref::<std::io::Error>()
        .is_some_and(|io_err| io_err.kind() == std::io::ErrorKind::UnexpectedEof)
}

pub fn is_ipfs_daemon_unreachable(e: &anyhow::Error) -> bool {
    e.downcast_ref::<NoaError>()
        .is_some_and(|ne| matches!(ne, NoaError::IpfsDaemonUnreachable { .. }))
}
