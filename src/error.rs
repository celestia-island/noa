pub use anyhow::Result;

pub fn is_eof_error(e: &anyhow::Error) -> bool {
    e.downcast_ref::<std::io::Error>()
        .is_some_and(|io_err| io_err.kind() == std::io::ErrorKind::UnexpectedEof)
        || e.to_string().contains("failed to fill whole buffer")
}

pub fn is_object_not_found(e: &anyhow::Error) -> bool {
    e.to_string().contains("object not found:")
}

pub fn is_workspace_already_exists(e: &anyhow::Error) -> bool {
    e.to_string().contains("workspace already exists:")
}
