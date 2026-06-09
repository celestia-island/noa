mod file_impl;
mod format;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

pub use file_impl::FileAgentLog;

use crate::error::Result;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OpType {
    Write,
    Delete,
    Rename,
    Snapshot,
    Merge,
    Resolve,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogEntry {
    pub seq: u64,
    pub op: OpType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_conflict_ours_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolved_conflict_theirs_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_id: Option<String>,
    /// Timestamp in microseconds since Unix epoch
    pub ts: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

#[async_trait]
pub trait AgentLog: Send + Sync {
    async fn append(&self, entry: &LogEntry) -> Result<u64>;
    async fn read_since(&self, seq: u64) -> Result<Vec<LogEntry>>;
    async fn read_all(&self) -> Result<Vec<LogEntry>>;
    async fn next_seq(&self) -> Result<u64>;
    async fn compact_to(&self, up_to_seq: u64) -> Result<()>;
}
