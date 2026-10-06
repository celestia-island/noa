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

impl OpType {
    pub fn as_op_str(&self) -> &'static str {
        match self {
            OpType::Write => "write",
            OpType::Delete => "delete",
            OpType::Rename => "rename",
            OpType::Snapshot => "snapshot",
            OpType::Merge => "merge",
            OpType::Resolve => "resolve",
        }
    }
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
    /// Identity of the remote event this entry was synced from, if any.
    /// Single source of truth for sync idempotency: receivers record the
    /// sender's `(sender, seq)` here at commit time and skip re-applying
    /// events whose identity is already present. Local (non-synced) entries
    /// leave both as `None` and never match an incoming remote identity.
    /// Optional + serde-defaulted so pre-existing log lines still parse and
    /// old binaries ignore the unknown fields when reading new lines.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_seq: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_sender: Option<String>,
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
