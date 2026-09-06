mod events;
mod handshake;
#[cfg(unix)]
mod server;
mod transport;

use serde::{Deserialize, Serialize};

pub use events::{ApplyPullError, EventSyncEngine, SyncEvent};
pub use handshake::{
    handle_auth_request, handle_handshake_request, handle_ready, BranchSelection, NoaAuthResponse,
    NoaHandshakeResponse,
};
#[cfg(unix)]
pub use server::SyncServer;
pub use transport::JsonRpcMessage;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestNoaHandshake {
    pub workspace_id: String,
    pub remote_name: String,
    pub remote_path: String,
    pub token: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoaAuthRequest {
    pub workspace_id: String,
    pub branches: Vec<String>,
    pub suggested_branch: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoaReady {
    pub workspace_id: String,
    pub branch: String,
    pub snapshot_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoaEventSyncMessage {
    pub workspace_id: String,
    pub events: Vec<SyncEvent>,
    pub direction: SyncDirection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SyncDirection {
    Push,
    Pull,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoaAck {
    pub ok: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NoaEventSyncAck {
    pub workspace_id: String,
    pub applied: u64,
    pub ok: bool,
    /// Human-readable error detail when `ok` is false (e.g. which event of a
    /// partially-applied batch failed and why). Optional + serde-defaulted so
    /// ACKs written by older peers (without this field) still deserialize,
    /// and older peers ignore this unknown field when reading new ACKs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl NoaEventSyncAck {
    /// Build an ACK from an apply outcome, reporting the committed prefix
    /// accurately: full count + `ok=true` on success, committed-prefix count
    /// + `ok=false` + error text on partial failure (never `applied=0` when a
    /// prefix was durably committed).
    #[must_use]
    pub fn from_apply_result(
        workspace_id: String,
        result: std::result::Result<u64, ApplyPullError>,
    ) -> Self {
        match result {
            Ok(applied) => NoaEventSyncAck {
                workspace_id,
                applied,
                ok: true,
                error: None,
            },
            Err(e) => NoaEventSyncAck {
                workspace_id,
                applied: e.applied,
                ok: false,
                error: Some(e.source.to_string()),
            },
        }
    }
}
