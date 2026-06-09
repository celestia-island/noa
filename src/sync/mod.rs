mod events;
mod handshake;
mod server;
mod transport;

pub use events::{EventSyncEngine, SyncEvent};
pub use handshake::{
    handle_auth_request, handle_handshake_request, handle_ready, BranchSelection, NoaAuthResponse,
    NoaHandshakeResponse,
};
pub use server::SyncServer;
pub use transport::JsonRpcMessage;

use serde::{Deserialize, Serialize};

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
}
