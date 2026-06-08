mod handshake;
mod server;
mod sync;
mod transport;

pub use handshake::{
    handle_auth_request, handle_handshake_request, handle_ready, BranchSelection,
    NoaAuthResponse, NoaHandshakeResponse,
};
pub use server::PolemosServer;
pub use sync::{EventSyncEngine, SyncEvent};
pub use transport::{JsonRpcMessage, JsonRpcTransport, UnixSocketTransport};

use serde::{Deserialize, Serialize};

pub const PROTOCOL_VERSION: &str = "0.1.0";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestNoaHandshake {
    pub workspace_id: String,
    pub remote_name: String,
    pub remote_path: String,
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
#[serde(tag = "method", content = "params")]
pub enum PolemosRequest {
    #[serde(rename = "noa.handshake")]
    Handshake(RequestNoaHandshake),
    #[serde(rename = "noa.auth")]
    Auth(NoaAuthRequest),
    #[serde(rename = "noa.ready")]
    Ready(NoaReady),
    #[serde(rename = "noa.event_sync")]
    EventSync(NoaEventSyncMessage),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PolemosResponse {
    Handshake(NoaHandshakeResponse),
    Auth(NoaAuthResponse),
    Ack(NoaAck),
    EventSyncAck(NoaEventSyncAck),
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
