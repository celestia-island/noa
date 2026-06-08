use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::net::UnixListener;
use tokio::sync::Mutex;

use crate::error::{NoaError, Result};

use super::handshake::NoaAuthResponse;
use super::transport::JsonRpcMessage;
use super::sync::EventSyncEngine;
use super::{
    NoaEventSyncAck, NoaEventSyncMessage, RequestNoaHandshake, NoaAuthRequest, NoaReady,
};

pub struct PolemosServer {
    socket_path: PathBuf,
    workspace_root: PathBuf,
    workspace_name: String,
}

impl PolemosServer {
    pub fn new(socket_path: &Path, workspace_root: &Path, workspace_name: &str) -> Self {
        PolemosServer {
            socket_path: socket_path.to_path_buf(),
            workspace_root: workspace_root.to_path_buf(),
            workspace_name: workspace_name.to_string(),
        }
    }

    pub async fn listen(&self) -> Result<()> {
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path)?;
        }

        let listener = UnixListener::bind(&self.socket_path)
            .map_err(|e| NoaError::Polemos(format!("failed to bind {}: {}", self.socket_path.display(), e)))?;

        tracing::info!("Polemos server listening on {}", self.socket_path.display());

        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    let workspace_root = self.workspace_root.clone();
                    let workspace_name = self.workspace_name.clone();
                    tokio::spawn(async move {
                        if let Err(e) =
                            Self::handle_connection(stream, &workspace_root, &workspace_name).await
                        {
                            tracing::error!("connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("accept error: {}", e);
                }
            }
        }
    }

    async fn handle_connection(
        stream: tokio::net::UnixStream,
        workspace_root: &Path,
        workspace_name: &str,
    ) -> Result<()> {
        let (reader, writer) = stream.into_split();
        let reader = Arc::new(Mutex::new(reader));
        let writer = Arc::new(Mutex::new(writer));

        loop {
            let msg = Self::read_message(reader.clone()).await?;
            let response = Self::dispatch(msg, workspace_root, workspace_name).await?;
            Self::write_message(writer.clone(), &response).await?;
        }
    }

    async fn read_message(
        reader: Arc<Mutex<tokio::net::unix::OwnedReadHalf>>,
    ) -> Result<JsonRpcMessage> {
        use tokio::io::AsyncReadExt;

        let mut reader = reader.lock().await;
        let mut len_buf = [0u8; 4];
        reader
            .read_exact(&mut len_buf)
            .await
            .map_err(|e| NoaError::Polemos(format!("read length: {}", e)))?;

        let len = u32::from_be_bytes(len_buf) as usize;
        let mut body_buf = vec![0u8; len];
        reader
            .read_exact(&mut body_buf)
            .await
            .map_err(|e| NoaError::Polemos(format!("read body: {}", e)))?;

        let json = std::str::from_utf8(&body_buf)
            .map_err(|e| NoaError::Serialization(e.to_string()))?;
        JsonRpcMessage::from_json(json)
    }

    async fn write_message(
        writer: Arc<Mutex<tokio::net::unix::OwnedWriteHalf>>,
        msg: &JsonRpcMessage,
    ) -> Result<()> {
        use tokio::io::AsyncWriteExt;

        let frame = msg.to_frame()?;
        let mut writer = writer.lock().await;
        writer
            .write_all(&frame)
            .await
            .map_err(|e| NoaError::Polemos(format!("write frame: {}", e)))?;
        writer
            .flush()
            .await
            .map_err(|e| NoaError::Polemos(format!("flush: {}", e)))
    }

    async fn dispatch(
        msg: JsonRpcMessage,
        workspace_root: &Path,
        workspace_name: &str,
    ) -> Result<JsonRpcMessage> {
        let id = msg.id.unwrap_or(0);
        let method = match msg.method {
            Some(m) => m,
            None => {
                return Ok(JsonRpcMessage::error_response(
                    id,
                    -32600,
                    "missing method",
                ))
            }
        };

        let params = msg.params.unwrap_or(serde_json::Value::Null);

        match method.as_str() {
            "noa.handshake" => {
                let req: RequestNoaHandshake = serde_json::from_value(params)
                    .map_err(|e| NoaError::Serialization(e.to_string()))?;
                let resp = super::handshake::handle_handshake_request(workspace_root, &req)?;
                Ok(JsonRpcMessage::response(id, serde_json::to_value(resp)?))
            }
            "noa.auth" => {
                let req: NoaAuthRequest = serde_json::from_value(params)
                    .map_err(|e| NoaError::Serialization(e.to_string()))?;
                tracing::info!(
                    "auth request: workspace={} suggested_branch={}",
                    req.workspace_id,
                    req.suggested_branch
                );
                let resp = NoaAuthResponse {
                    workspace_id: req.workspace_id.clone(),
                    selected_branch: req.suggested_branch.clone(),
                    branch_base: String::new(),
                    approved: true,
                };
                Ok(JsonRpcMessage::response(id, serde_json::to_value(resp)?))
            }
            "noa.ready" => {
                let req: NoaReady = serde_json::from_value(params)
                    .map_err(|e| NoaError::Serialization(e.to_string()))?;
                let ack = super::handshake::handle_ready(
                    &req.workspace_id,
                    &req.branch,
                    &req.snapshot_id,
                )?;
                Ok(JsonRpcMessage::response(id, serde_json::to_value(ack)?))
            }
            "noa.event_sync" => {
                let sync_msg: NoaEventSyncMessage = serde_json::from_value(params)
                    .map_err(|e| NoaError::Serialization(e.to_string()))?;
                let engine = EventSyncEngine::new(workspace_root, workspace_name);
                let applied = engine
                    .apply_pull_events(&sync_msg.events)
                    .await
                    .unwrap_or(0);
                let ack = NoaEventSyncAck {
                    workspace_id: sync_msg.workspace_id,
                    applied,
                    ok: true,
                };
                Ok(JsonRpcMessage::response(id, serde_json::to_value(ack)?))
            }
            _ => Ok(JsonRpcMessage::error_response(
                id,
                -32601,
                &format!("method not found: {}", method),
            )),
        }
    }
}
