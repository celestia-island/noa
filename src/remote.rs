use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushSpec {
    pub src: String,
    pub dst: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchSpec {
    pub src: String,
    pub dst: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteRef {
    pub name: String,
    pub commit_hash: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PushResult {
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchResult {
    pub refs: Vec<RemoteRef>,
}

#[async_trait]
pub trait RemoteBackend: Send + Sync {
    fn protocol(&self) -> &str;
    async fn push(&self, url: &str, specs: &[PushSpec]) -> Result<PushResult>;
    async fn fetch(&self, url: &str, specs: &[FetchSpec]) -> Result<FetchResult>;
    async fn list_refs(&self, url: &str) -> Result<Vec<RemoteRef>>;
}
