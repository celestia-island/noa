use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::net::UnixListener;
use tokio::sync::Mutex;

use crate::error::{NoaError, Result};

use super::events::EventSyncEngine;
use super::transport::JsonRpcMessage;
use super::{NoaAuthRequest, NoaEventSyncAck, NoaEventSyncMessage, NoaReady, RequestNoaHandshake};

pub struct SyncServer {
    socket_path: PathBuf,
    workspace_root: PathBuf,
    workspace_name: String,
    auth_token: String,
    authenticated_sessions: Arc<Mutex<std::collections::HashSet<String>>>,
}

const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;
const MAX_CONNECTIONS: usize = 32;

impl SyncServer {
    pub fn new(socket_path: &Path, workspace_root: &Path, workspace_name: &str) -> Self {
        SyncServer {
            socket_path: socket_path.to_path_buf(),
            workspace_root: workspace_root.to_path_buf(),
            workspace_name: workspace_name.to_string(),
            auth_token: std::env::var("NOA_SYNC_TOKEN").unwrap_or_default(),
            authenticated_sessions: Arc::new(Mutex::new(std::collections::HashSet::new())),
        }
    }

    pub async fn listen(&self) -> Result<()> {
        if self.socket_path.exists() {
            std::fs::remove_file(&self.socket_path)?;
        }

        let listener = UnixListener::bind(&self.socket_path).map_err(|e| {
            NoaError::Sync(format!("failed to bind sync socket: {}", e))
        })?;

        tracing::info!(
            "Noa sync server listening on {}",
            self.socket_path.display()
        );

        let connection_count = Arc::new(Mutex::new(0usize));

        loop {
            match listener.accept().await {
                Ok((stream, _addr)) => {
                    {
                        let mut count = connection_count.lock().await;
                        if *count >= MAX_CONNECTIONS {
                            tracing::warn!(
                                "rejecting connection: max connections ({}) reached",
                                MAX_CONNECTIONS
                            );
                            continue;
                        }
                        *count += 1;
                    }

                    let workspace_root = self.workspace_root.clone();
                    let workspace_name = self.workspace_name.clone();
                    let auth_token = self.auth_token.clone();
                    let authenticated_sessions = self.authenticated_sessions.clone();
                    let conn_count = Arc::clone(&connection_count);

                    tokio::spawn(async move {
                        if let Err(e) = Self::handle_connection(
                            stream,
                            &workspace_root,
                            &workspace_name,
                            &auth_token,
                            &authenticated_sessions,
                        )
                        .await
                        {
                            tracing::error!("connection error: {}", e);
                        }
                        let mut count = conn_count.lock().await;
                        *count = count.saturating_sub(1);
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
        auth_token: &str,
        authenticated_sessions: &Arc<Mutex<std::collections::HashSet<String>>>,
    ) -> Result<()> {
        let (reader, writer) = stream.into_split();
        let reader = Arc::new(Mutex::new(reader));
        let writer = Arc::new(Mutex::new(writer));

        loop {
            let msg = Self::read_message(reader.clone()).await?;
            let response = Self::dispatch(
                msg,
                workspace_root,
                workspace_name,
                auth_token,
                authenticated_sessions,
            )
            .await?;
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
            .map_err(|e| NoaError::Sync(format!("read length: {}", e)))?;

        let len = u32::from_be_bytes(len_buf) as usize;
        if len > MAX_MESSAGE_SIZE {
            return Err(NoaError::Sync(format!(
                "message too large: {} bytes (max {})",
                len, MAX_MESSAGE_SIZE
            )));
        }
        let mut body_buf = vec![0u8; len];
        reader
            .read_exact(&mut body_buf)
            .await
            .map_err(|e| NoaError::Sync(format!("read body: {}", e)))?;

        let json =
            std::str::from_utf8(&body_buf).map_err(|e| NoaError::Serialization(e.to_string()))?;
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
            .map_err(|e| NoaError::Sync(format!("write frame: {}", e)))?;
        writer
            .flush()
            .await
            .map_err(|e| NoaError::Sync(format!("flush: {}", e)))
    }

    async fn dispatch(
        msg: JsonRpcMessage,
        workspace_root: &Path,
        workspace_name: &str,
        auth_token: &str,
        authenticated_sessions: &Arc<Mutex<std::collections::HashSet<String>>>,
    ) -> Result<JsonRpcMessage> {
        let id = msg.id.unwrap_or(0);
        let method = match msg.method {
            Some(m) => m,
            None => return Ok(JsonRpcMessage::error_response(id, -32600, "missing method")),
        };

        let params = msg.params.unwrap_or(serde_json::Value::Null);

        match method.as_str() {
            "noa.handshake" => {
                let req: RequestNoaHandshake = serde_json::from_value(params)
                    .map_err(|e| NoaError::Serialization(e.to_string()))?;
                let resp = super::handshake::handle_handshake_request(workspace_root, &req)?;

                if !auth_token.is_empty() {
                    let mut sessions = authenticated_sessions.lock().await;
                    sessions.insert(resp.workspace_id.clone());
                }

                Ok(JsonRpcMessage::response(id, serde_json::to_value(resp)?))
            }
            "noa.auth" => {
                let req: NoaAuthRequest = serde_json::from_value(params)
                    .map_err(|e| NoaError::Serialization(e.to_string()))?;

                if !auth_token.is_empty() {
                    let sessions = authenticated_sessions.lock().await;
                    if !sessions.contains(&req.workspace_id) {
                        return Ok(JsonRpcMessage::error_response(
                            id,
                            -32001,
                            "unauthorized: workspace not authenticated",
                        ));
                    }
                }

                tracing::info!(
                    "auth request: workspace={} suggested_branch={}",
                    req.workspace_id,
                    req.suggested_branch
                );
                let resp = super::handshake::handle_auth_request(
                    workspace_root,
                    &super::handshake::BranchSelection::Current,
                    &req.suggested_branch,
                )?;
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

                if !auth_token.is_empty() {
                    let sessions = authenticated_sessions.lock().await;
                    if !sessions.contains(&sync_msg.workspace_id) {
                        return Ok(JsonRpcMessage::error_response(
                            id,
                            -32001,
                            "unauthorized: workspace not authenticated",
                        ));
                    }
                }

                let engine = EventSyncEngine::new(workspace_root, workspace_name);
                let (applied, ok, _error_msg) = match engine
                    .apply_pull_events(&sync_msg.events)
                    .await
                {
                    Ok(n) => (n, true, None),
                    Err(e) => {
                        tracing::error!("event sync apply failed: {}", e);
                        (0, false, Some(e.to_string()))
                    }
                };
                let ack = NoaEventSyncAck {
                    workspace_id: sync_msg.workspace_id,
                    applied,
                    ok,
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
