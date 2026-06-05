use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Result;

mod diff;
mod engine;
mod redb_impl;

pub use diff::{diff_snapshots, DiffKind, FileDiff};
pub use engine::SnapshotEngine;
pub use redb_impl::RedbSnapshotStore;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SnapshotId(pub String);

impl SnapshotId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SnapshotId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub fn generate_snapshot_id() -> SnapshotId {
    let raw = uuid::Uuid::now_v7();
    let bytes = raw.as_bytes();
    let encoded = base62_encode(bytes);
    SnapshotId(format!("noa_{}", &encoded[..12]))
}

fn base62_encode(data: &[u8]) -> String {
    const CHARSET: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut result = Vec::with_capacity(data.len() * 2);
    let mut num = 0u128;
    for &byte in data {
        num = (num << 8) | byte as u128;
    }
    while num > 0 {
        result.push(CHARSET[(num % 62) as usize]);
        num /= 62;
    }
    while result.len() < 16 {
        result.push(CHARSET[0]);
    }
    result.reverse();
    String::from_utf8(result).unwrap()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub id: SnapshotId,
    pub tree_hash: String,
    pub parents: Vec<SnapshotId>,
    pub workspace: String,
    pub author: String,
    pub timestamp: u64,
    pub message: String,
}

#[async_trait]
pub trait SnapshotStore: Send + Sync {
    async fn get(&self, id: &SnapshotId) -> Result<Snapshot>;
    async fn store(&self, snapshot: &Snapshot) -> Result<()>;
    async fn children_of(&self, parent: &SnapshotId) -> Result<Vec<SnapshotId>>;
    async fn list_all(&self) -> Result<Vec<Snapshot>>;
}
