use crate::error::{NoaError, Result};
use crate::object::{BlobId, EntryKind, ObjectStore, TreeEntries, TreeEntry, TreeId};
use crate::remote::{FetchResult, FetchSpec, PushResult, PushSpec, RemoteBackend, RemoteRef};
use crate::snapshot::{Snapshot, SnapshotId};
use crate::refs::RefStore;

pub struct GitBackend;

impl GitBackend {
    pub fn new() -> Self {
        GitBackend
    }
}

#[async_trait::async_trait]
impl RemoteBackend for GitBackend {
    fn protocol(&self) -> &str {
        "git"
    }

    async fn push(&self, url: &str, specs: &[PushSpec]) -> Result<PushResult> {
        todo!("git push via git2")
    }

    async fn fetch(&self, url: &str, specs: &[FetchSpec]) -> Result<FetchResult> {
        todo!("git fetch via git2")
    }

    async fn list_refs(&self, url: &str) -> Result<Vec<RemoteRef>> {
        todo!("git ls-remote via git2")
    }
}
