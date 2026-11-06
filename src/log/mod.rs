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
    Verify,
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
            OpType::Verify => "verify",
        }
    }
}

/// Kind of a recorded verification run (`VerifyRecord::kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VerifyKind {
    /// Full local gate over the whole test surface.
    Full,
    /// Selective run (subset of commands/tests chosen by the executor).
    Partial,
    /// Run performed by an external sampler replaying a claimed record.
    Sampled,
}

/// Toolchain facts a verification run was pinned to (`VerifyRecord::env`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyEnv {
    /// Toolchain identity, e.g. "stable 1.82.0".
    pub toolchain: String,
    /// Platform / target triple of the run, e.g. "x86_64-unknown-linux-gnu".
    pub platform: String,
    /// Hex digest of the lockfile the run was pinned to, when recorded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lock_hash: Option<String>,
    /// Whether the commands ran with `--offline`.
    #[serde(default)]
    pub offline: bool,
    /// Hardware identity of the executing machine: `sha256(/etc/machine-id)`
    /// hex, registered in the workspace hardware registry. Absent on
    /// unregistered machines or non-Linux runs (optional, backwards
    /// compatible both ways per the `lock_hash` precedent).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hardware: Option<String>,
}

/// One executed gate command (`VerifyRecord::commands`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyCommand {
    /// Full argv as executed, e.g. ["cargo", "test", "--offline", "--locked"].
    pub argv: Vec<String>,
    /// Process exit code; absent when the process did not exit normally
    /// (killed, timed out).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Wall-clock duration in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Mechanical digest of the command's `test result` lines, when any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

/// Negative record (`VerifyRecord::not_run`): a surface this run explicitly
/// did NOT cover. The honest disclosure that keeps green `commands` from
/// overclaiming coverage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyNotRun {
    /// The skipped surface, e.g. "--all-features" or "tests/webui".
    pub what: String,
    /// Why it was skipped, when worth recording.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// A produced artifact with its digest (`VerifyRecord::outputs`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyOutput {
    /// Artifact path relative to the repository root.
    pub path: String,
    /// sha256 hex digest of the artifact bytes.
    pub sha256: String,
}

/// Payload of an `OpType::Verify` log entry: one recorded verification run
/// (schema `verify.run/1`). Log-only ledger metadata — verify entries never
/// produce tree changes; snapshot/merge replay treats them as no-ops.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerifyRecord {
    /// Version prefix, e.g. "verify.run/1"; bumped on breaking changes.
    pub schema: String,
    pub kind: VerifyKind,
    /// Repository identity as owner/name.
    pub repo: String,
    /// Exact commit SHA that was verified.
    pub head: String,
    /// Whether the working tree was clean when the run started
    /// (a dirty tree degrades the record).
    pub tree_clean: bool,
    pub env: VerifyEnv,
    #[serde(default)]
    pub commands: Vec<VerifyCommand>,
    /// Negative records: what this run explicitly did NOT cover. Always
    /// serialized (even when empty) so an absent list cannot be mistaken
    /// for "nothing was skipped".
    #[serde(default)]
    pub not_run: Vec<VerifyNotRun>,
    #[serde(default)]
    pub outputs: Vec<VerifyOutput>,
    /// Executor identity (workspace claim-owner semantics).
    pub agent: String,
    /// Hash of the previous record in the same ledger (file or noa), so the
    /// tamper-evidence chain travels WITH the record when the noa ledger
    /// becomes the authority — without it, the authoritative ledger would be
    /// weaker than the file ledger it retires (review R3 on PR #129).
    /// None on the first record of a chain and on pre-chain records; the
    /// chain is computed by the writer, this payload only carries it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev: Option<String>,
    /// Timestamp in microseconds since Unix epoch.
    pub ts: u64,
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
    /// Payload of an `OpType::Verify` entry: one recorded verification run
    /// (schema `verify.run/1`). Log-only metadata; never produces tree
    /// changes. Optional + serde-defaulted so pre-existing log lines still
    /// parse, exactly like the `remote_seq`/`remote_sender` precedent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify: Option<VerifyRecord>,
    /// Timestamp in microseconds since Unix epoch
    pub ts: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

// See src/object/mod.rs: clippy(1.99) double_must_use on async_trait (H15).
#[allow(clippy::double_must_use)]
#[async_trait]
pub trait AgentLog: Send + Sync {
    async fn append(&self, entry: &LogEntry) -> Result<u64>;
    async fn read_since(&self, seq: u64) -> Result<Vec<LogEntry>>;
    async fn read_all(&self) -> Result<Vec<LogEntry>>;
    async fn next_seq(&self) -> Result<u64>;
    async fn compact_to(&self, up_to_seq: u64) -> Result<()>;
}
