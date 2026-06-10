use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::{net::UnixListener, sync::Mutex};

use super::{
    events::EventSyncEngine, transport::JsonRpcMessage, NoaAuthRequest, NoaEventSyncAck,
    NoaEventSyncMessage, NoaReady, RequestNoaHandshake,
};
use crate::error::{is_eof_error, Result};
use anyhow::Context;

pub struct SyncServer {
    socket_path: PathBuf,
    workspace_root: PathBuf,
    workspace_name: String,
    auth_token: String,
    authenticated_sessions: Arc<Mutex<std::collections::HashMap<String, Instant>>>,
}

const MAX_MESSAGE_SIZE: usize = 16 * 1024 * 1024;
const MAX_CONNECTIONS: usize = 32;
const SESSION_TTL: Duration = Duration::from_secs(3600);

fn generate_sync_token() -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut buf = [0u8; 32];
    getrandom::getrandom(&mut buf)
        .map_err(|e| anyhow::anyhow!("failed to generate random sync token: {e}"))?;
    let hash = Sha256::digest(buf);
    Ok(hex::encode(hash))
}

impl SyncServer {
    pub fn new(socket_path: &Path, workspace_root: &Path, workspace_name: &str) -> Result<Self> {
        let auth_token = match std::env::var("NOA_SYNC_TOKEN") {
            Ok(token) => token,
            Err(_) => {
                let token = generate_sync_token()?;
                tracing::info!(
                    "Noa sync server generated a random auth token. \
                     Set NOA_SYNC_TOKEN env var to use a fixed token."
                );
                token
            }
        };
        Ok(SyncServer {
            socket_path: socket_path.to_path_buf(),
            workspace_root: workspace_root.to_path_buf(),
            workspace_name: workspace_name.to_string(),
            auth_token,
            authenticated_sessions: Arc::new(Mutex::new(std::collections::HashMap::new())),
        })
    }

    pub async fn listen(&self) -> Result<()> {
        let _ = std::fs::remove_file(&self.socket_path);

        let socket_dir = self.socket_path.parent();
        if let Some(dir) = socket_dir {
            std::fs::create_dir_all(dir)?;
        }

        let listener = if let Some(dir) = socket_dir {
            let file_name = self
                .socket_path
                .file_name()
                .map_or_else(|| "sock".to_string(), |n| n.to_string_lossy().into_owned());
            let tmp_path = dir.join(format!(".{file_name}.bind.tmp"));

            let _ = std::fs::remove_file(&tmp_path);
            let tmp_listener = UnixListener::bind(&tmp_path)
                .with_context(|| "failed to bind sync socket (temp)")?;

            let mut perms = std::fs::metadata(&tmp_path)?.permissions();
            perms.set_mode(0o600);
            std::fs::set_permissions(&tmp_path, perms)?;

            std::fs::rename(&tmp_path, &self.socket_path)
                .with_context(|| "failed to rename socket to final path")?;

            tmp_listener
        } else {
            UnixListener::bind(&self.socket_path).with_context(|| "failed to bind sync socket")?
        };

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
        authenticated_sessions: &Arc<Mutex<std::collections::HashMap<String, Instant>>>,
    ) -> Result<()> {
        let (reader, writer) = stream.into_split();
        let reader = Arc::new(Mutex::new(reader));
        let writer = Arc::new(Mutex::new(writer));

        loop {
            let msg = match Self::read_message(reader.clone()).await {
                Ok(msg) => msg,
                Err(e) => {
                    if is_eof_error(&e) {
                        tracing::debug!("client disconnected from sync socket");
                        return Ok(());
                    }
                    return Err(e);
                }
            };
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
            .with_context(|| "read length")?;

        let len = u32::from_be_bytes(len_buf) as usize;
        if len > MAX_MESSAGE_SIZE {
            anyhow::bail!("message too large: {len} bytes (max {MAX_MESSAGE_SIZE})");
        }
        let mut body_buf = vec![0u8; len];
        reader
            .read_exact(&mut body_buf)
            .await
            .with_context(|| "read body")?;

        let json = std::str::from_utf8(&body_buf)?;
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
            .with_context(|| "write frame")?;
        writer.flush().await.with_context(|| "flush")
    }

    async fn dispatch(
        msg: JsonRpcMessage,
        workspace_root: &Path,
        workspace_name: &str,
        auth_token: &str,
        authenticated_sessions: &Arc<Mutex<std::collections::HashMap<String, Instant>>>,
    ) -> Result<JsonRpcMessage> {
        let id = msg.id.unwrap_or(0);
        let Some(method) = msg.method else {
            return Ok(JsonRpcMessage::error_response(id, -32600, "missing method"));
        };

        let params = msg.params.unwrap_or(serde_json::Value::Null);

        match method.as_str() {
            "noa.handshake" => {
                let req: RequestNoaHandshake = serde_json::from_value(params)?;
                let resp =
                    super::handshake::handle_handshake_request(workspace_root, &req, auth_token)?;

                let mut sessions = authenticated_sessions.lock().await;
                sessions.insert(resp.workspace_id.clone(), Instant::now());

                Ok(JsonRpcMessage::response(id, serde_json::to_value(resp)?))
            }
            "noa.auth" => {
                let req: NoaAuthRequest = serde_json::from_value(params)?;

                {
                    let mut sessions = authenticated_sessions.lock().await;
                    let is_valid = match sessions.get_mut(&req.workspace_id) {
                        Some(last_seen) if last_seen.elapsed() < SESSION_TTL => {
                            *last_seen = Instant::now();
                            true
                        }
                        Some(_) => {
                            sessions.remove(&req.workspace_id);
                            false
                        }
                        None => false,
                    };
                    if !is_valid {
                        return Ok(JsonRpcMessage::error_response(
                            id,
                            -32001,
                            "unauthorized: workspace not authenticated or session expired",
                        ));
                    }
                }

                tracing::info!(
                    "auth request: workspace={} suggested_branch={}",
                    req.workspace_id,
                    req.suggested_branch
                );
                let branch_prefix = crate::config::RepoConfig::load_from_dir(
                    &workspace_root.join(crate::repo::NOA_DIR_NAME),
                )
                .ok()
                .and_then(|cfg| cfg.sync)
                .map(|s| s.default_branch_prefix)
                .unwrap_or_else(|| "entelecheia/agent-".to_string());

                let resp = super::handshake::handle_auth_request(
                    workspace_root,
                    &req.workspace_id,
                    &super::handshake::BranchSelection::Current,
                    &req.suggested_branch,
                    &branch_prefix,
                )?;
                Ok(JsonRpcMessage::response(id, serde_json::to_value(resp)?))
            }
            "noa.ready" => {
                let req: NoaReady = serde_json::from_value(params)?;
                let ack = super::handshake::handle_ready(
                    &req.workspace_id,
                    &req.branch,
                    &req.snapshot_id,
                )?;
                Ok(JsonRpcMessage::response(id, serde_json::to_value(ack)?))
            }
            "noa.event_sync" => {
                let sync_msg: NoaEventSyncMessage = serde_json::from_value(params)?;

                {
                    let mut sessions = authenticated_sessions.lock().await;
                    let is_valid = match sessions.get_mut(&sync_msg.workspace_id) {
                        Some(last_seen) if last_seen.elapsed() < SESSION_TTL => {
                            *last_seen = Instant::now();
                            true
                        }
                        Some(_) => {
                            sessions.remove(&sync_msg.workspace_id);
                            false
                        }
                        None => false,
                    };
                    if !is_valid {
                        return Ok(JsonRpcMessage::error_response(
                            id,
                            -32001,
                            "unauthorized: workspace not authenticated or session expired",
                        ));
                    }
                }

                let engine = EventSyncEngine::new(workspace_root, workspace_name);
                let (applied, ok, _error_msg) =
                    match engine.apply_pull_events(&sync_msg.events).await {
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
                &format!("method not found: {method}"),
            )),
        }
    }
}
