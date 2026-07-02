use async_trait::async_trait;

use crate::{
    error::{remote_err, NoaError, Result},
    object::{sha256_hex, BlobId, ObjectStore, TreeEntries, TreeId},
};

pub struct SftpObjectStore {
    host: String,
    port: u16,
    username: String,
    password: Option<String>,
    base_path: String,
}

impl Clone for SftpObjectStore {
    fn clone(&self) -> Self {
        SftpObjectStore {
            host: self.host.clone(),
            port: self.port,
            username: self.username.clone(),
            password: self.password.clone(),
            base_path: self.base_path.clone(),
        }
    }
}

impl SftpObjectStore {
    pub fn from_config(config: &crate::config::StorageConfig) -> Result<Self> {
        let endpoint = config.effective_endpoint();
        let username = config
            .username
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("SFTP transport requires 'username'"))?;
        let port = if config.port > 0 { config.port } else { 22 };
        Ok(Self::new(
            &endpoint,
            port,
            username,
            config.password.as_deref(),
        ))
    }

    #[must_use]
    pub fn new(host: &str, port: u16, username: &str, password: Option<&str>) -> Self {
        SftpObjectStore {
            host: host.to_string(),
            port,
            username: username.to_string(),
            password: password.map(str::to_string),
            base_path: "noa-objects".to_string(),
        }
    }

    fn remote_addr(&self) -> String {
        format!("{}@{}", self.username, self.host)
    }

    fn sshpass_prefix(&self) -> Vec<String> {
        if let Some(ref pass) = self.password {
            vec!["sshpass".to_string(), "-p".to_string(), pass.clone()]
        } else {
            vec![]
        }
    }

    fn is_sshpass_available(&self) -> bool {
        self.password.is_some()
            && std::process::Command::new("sshpass")
                .arg("-V")
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
    }

    fn blob_path(id: &BlobId) -> String {
        format!("blobs/{}", id.0)
    }

    fn tree_path(id: &TreeId) -> String {
        format!("trees/{}", id.0)
    }

    fn err(ctx: &str, e: impl std::fmt::Display) -> anyhow::Error {
        remote_err("sftp", format!("{ctx}: {e}"))
    }

    async fn ssh_exec(&self, remote_cmd: &str) -> Result<std::process::Output> {
        let addr = self.remote_addr();
        let cmd = remote_cmd.to_string();
        let use_sshpass = self.is_sshpass_available();
        let port = self.port;
        let sshpass_prefix = self.sshpass_prefix();
        tokio::task::spawn_blocking(move || {
            let mut command = if use_sshpass {
                let mut c = std::process::Command::new("sshpass");
                c.args(&sshpass_prefix).arg("ssh");
                c
            } else {
                std::process::Command::new("ssh")
            };
            if port != 22 {
                command.arg(format!("-p{port}"));
            }
            command.arg(&addr).arg(&cmd).output()
        })
        .await
        .map_err(|e| Self::err("ssh spawn", e))?
        .map_err(|e| Self::err("ssh exec", e))
    }

    async fn scp_upload(&self, data: Vec<u8>, remote_path: &str) -> Result<()> {
        let addr = self.remote_addr();
        let remote_full = format!("{addr}:{remote_path}");
        let remote_mkdir = format!("mkdir -p $(dirname {remote_path})");
        let remote_full_clone = remote_full.clone();
        let use_sshpass = self.is_sshpass_available();
        let port = self.port;
        let sshpass_prefix = self.sshpass_prefix();

        let _ = self.ssh_exec(&remote_mkdir).await;

        tokio::task::spawn_blocking(move || {
            let mut args = Vec::new();
            if port != 22 {
                args.push(format!("-P{port}"));
            }
            args.extend_from_slice(&["-".to_string(), remote_full_clone]);

            let mut child = if use_sshpass {
                let mut c = std::process::Command::new("sshpass");
                c.args(&sshpass_prefix).arg("scp").args(&args);
                c
            } else {
                let mut c = std::process::Command::new("scp");
                c.args(&args);
                c
            }
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
            if let Some(mut stdin) = child.stdin.take() {
                use std::io::Write;
                stdin.write_all(&data)?;
            }
            child.wait_with_output()
        })
        .await
        .map_err(|e| Self::err("scp spawn", e))?
        .map_err(|e| Self::err("scp exec", e))?;
        Ok(())
    }

    async fn scp_download(&self, remote_path: &str) -> Result<Vec<u8>> {
        let addr = self.remote_addr();
        let remote_full = format!("{addr}:{remote_path}");
        let remote_full_clone = remote_full.clone();
        let use_sshpass = self.is_sshpass_available();
        let port = self.port;
        let sshpass_prefix = self.sshpass_prefix();

        let output = tokio::task::spawn_blocking(move || {
            let mut args = Vec::new();
            if port != 22 {
                args.push(format!("-P{port}"));
            }
            args.extend_from_slice(&[remote_full_clone, "-".to_string()]);

            if use_sshpass {
                let mut c = std::process::Command::new("sshpass");
                c.args(&sshpass_prefix).arg("scp").args(&args);
                c
            } else {
                let mut c = std::process::Command::new("scp");
                c.args(&args);
                c
            }
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
        })
        .await
        .map_err(|e| Self::err("scp spawn", e))?
        .map_err(|e| Self::err("scp exec", e))?;

        if output.status.success() {
            Ok(output.stdout)
        } else {
            let stderr = String::from_utf8_lossy(&output.stderr);
            if stderr.contains("No such file") || stderr.contains("not found") {
                Err(NoaError::ObjectNotFound {
                    id: remote_path.to_string(),
                }
                .into())
            } else {
                Err(Self::err("scp download", stderr.trim()))
            }
        }
    }

    async fn remote_file_exists(&self, remote_path: &str) -> bool {
        let cmd = format!("test -f {remote_path} && echo yes || echo no");
        match self.ssh_exec(&cmd).await {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout);
                stdout.contains("yes")
            }
            Err(_) => false,
        }
    }
}

#[async_trait]
impl ObjectStore for SftpObjectStore {
    async fn put_blob(&self, content: &[u8]) -> Result<BlobId> {
        let id = BlobId(sha256_hex(content));
        let path = format!("{}/{}", self.base_path, Self::blob_path(&id));
        self.scp_upload(content.to_vec(), &path).await?;
        Ok(id)
    }

    async fn get_blob(&self, id: &BlobId) -> Result<Vec<u8>> {
        let path = format!("{}/{}", self.base_path, Self::blob_path(id));
        self.scp_download(&path).await
    }

    async fn has_blob(&self, id: &BlobId) -> Result<bool> {
        let path = format!("{}/{}", self.base_path, Self::blob_path(id));
        Ok(self.remote_file_exists(&path).await)
    }

    async fn put_tree(&self, entries: &TreeEntries) -> Result<TreeId> {
        let data = rmp_serde::to_vec(entries)?;
        let id = TreeId(sha256_hex(&data));
        let path = format!("{}/{}", self.base_path, Self::tree_path(&id));
        self.scp_upload(data, &path).await?;
        Ok(id)
    }

    async fn get_tree(&self, id: &TreeId) -> Result<TreeEntries> {
        let path = format!("{}/{}", self.base_path, Self::tree_path(id));
        let data = self.scp_download(&path).await?;
        Ok(rmp_serde::from_slice::<TreeEntries>(&data)?)
    }

    async fn has_tree(&self, id: &TreeId) -> Result<bool> {
        let path = format!("{}/{}", self.base_path, Self::tree_path(id));
        Ok(self.remote_file_exists(&path).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sftp_paths() {
        assert_eq!(
            SftpObjectStore::blob_path(&BlobId("abc".to_string())),
            "blobs/abc"
        );
        assert_eq!(
            SftpObjectStore::tree_path(&TreeId("def".to_string())),
            "trees/def"
        );
    }

    #[test]
    fn test_sftp_new() {
        let store = SftpObjectStore::new("sftp.example.com", 22, "user", Some("pass"));
        assert_eq!(store.host, "sftp.example.com");
        assert_eq!(store.port, 22);
        assert_eq!(store.username, "user");
        assert_eq!(store.remote_addr(), "user@sftp.example.com");
    }
}
