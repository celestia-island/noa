use async_trait::async_trait;
use std::io::Cursor;

use crate::{
    error::{remote_err, NoaError, Result},
    object::{sha256_hex, BlobId, ObjectStore, TreeEntries, TreeId},
};

pub struct FtpObjectStore {
    host: String,
    port: u16,
    username: String,
    password: String,
    use_tls: bool,
}

impl Clone for FtpObjectStore {
    fn clone(&self) -> Self {
        FtpObjectStore {
            host: self.host.clone(),
            port: self.port,
            username: self.username.clone(),
            password: self.password.clone(),
            use_tls: self.use_tls,
        }
    }
}

impl FtpObjectStore {
    #[must_use]
    pub fn new(host: &str, port: u16, username: &str, password: &str, use_tls: bool) -> Self {
        FtpObjectStore {
            host: host.to_string(),
            port,
            username: username.to_string(),
            password: password.to_string(),
            use_tls,
        }
    }

    pub fn from_config(config: &crate::config::StorageConfig) -> Result<Self> {
        let endpoint = config.effective_endpoint();
        let username = config.username.as_deref()
            .ok_or_else(|| anyhow::anyhow!("FTP storage requires 'username'"))?;
        let password = config.password.as_deref()
            .ok_or_else(|| anyhow::anyhow!("FTP storage requires 'password'"))?;
        let port = if config.port > 0 { config.port } else { 21 };
        Ok(Self::new(&endpoint, port, username, password, config.use_tls))
    }

    fn addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }

    fn blob_path(id: &BlobId) -> String {
        format!("blobs/{}", id.0)
    }

    fn tree_path(id: &TreeId) -> String {
        format!("trees/{}", id.0)
    }

    fn ftp_err(context: &str, e: impl std::fmt::Display) -> anyhow::Error {
        remote_err("ftp", format!("{context}: {e}"))
    }

    async fn with_ftp<F, T>(&self, f: F) -> Result<T>
    where
        F: for<'a> FnOnce(
                &'a mut suppaftp::tokio::AsyncFtpStream,
            ) -> std::pin::Pin<
                Box<dyn std::future::Future<Output = Result<T>> + Send + 'a>,
            > + Send,
        T: Send,
    {
        if self.use_tls {
            anyhow::bail!("FTPS is not yet supported. Use plain FTP (set use_tls=false).");
        }

        let addr = self.addr();
        let mut ftp = suppaftp::tokio::AsyncFtpStream::connect(&addr)
            .await
            .map_err(|e| Self::ftp_err("connect", e))?;

        ftp.login(&self.username, &self.password)
            .await
            .map_err(|e| Self::ftp_err("login", e))?;

        let result = f(&mut ftp).await;
        let _ = ftp.quit().await;
        result
    }

    async fn ensure_dirs(ftp: &mut suppaftp::tokio::AsyncFtpStream, path: &str) -> Result<()> {
        if let Some(parent) = std::path::Path::new(path).parent().and_then(|p| p.to_str()) {
            if !parent.is_empty() {
                let _ = ftp.mkdir(parent).await;
            }
        }
        Ok(())
    }
}

#[async_trait]
impl ObjectStore for FtpObjectStore {
    async fn put_blob(&self, content: &[u8]) -> Result<BlobId> {
        let id = BlobId(sha256_hex(content));
        let path = Self::blob_path(&id);
        let data = content.to_vec();

        self.with_ftp(move |ftp| {
            let path = path.clone();
            let data = data.clone();
            Box::pin(async move {
                Self::ensure_dirs(ftp, &path).await?;
                let mut reader = Cursor::new(data);
                ftp.put_file(&path, &mut reader)
                    .await
                    .map_err(|e| Self::ftp_err("put_blob", e))?;
                Ok(())
            })
        })
        .await?;

        Ok(id)
    }

    async fn get_blob(&self, id: &BlobId) -> Result<Vec<u8>> {
        use tokio::io::AsyncReadExt;
        let path = Self::blob_path(id);

        self.with_ftp(move |ftp| {
            let path = path.clone();
            Box::pin(async move {
                let mut stream = ftp.retr_as_stream(&path).await.map_err(|e| {
                    let msg = format!("{e}");
                    if msg.contains("550") || msg.contains("not found") {
                        NoaError::ObjectNotFound { id: path.clone() }.into()
                    } else {
                        Self::ftp_err("get_blob", e)
                    }
                })?;
                let mut buf = Vec::new();
                stream
                    .read_to_end(&mut buf)
                    .await
                    .map_err(|e| remote_err("ftp", format!("read stream: {e}")))?;
                ftp.finalize_retr_stream(stream)
                    .await
                    .map_err(|e| Self::ftp_err("finalize_retr", e))?;
                Ok(buf)
            })
        })
        .await
    }

    async fn has_blob(&self, id: &BlobId) -> Result<bool> {
        let path = Self::blob_path(id);

        self.with_ftp(move |ftp| {
            let path = path.clone();
            Box::pin(async move { Ok(ftp.size(&path).await.is_ok()) })
        })
        .await
    }

    async fn put_tree(&self, entries: &TreeEntries) -> Result<TreeId> {
        let data = rmp_serde::to_vec(entries)?;
        let id = TreeId(sha256_hex(&data));
        let path = Self::tree_path(&id);

        self.with_ftp(move |ftp| {
            let path = path.clone();
            let data = data.clone();
            Box::pin(async move {
                Self::ensure_dirs(ftp, &path).await?;
                let mut reader = Cursor::new(data);
                ftp.put_file(&path, &mut reader)
                    .await
                    .map_err(|e| Self::ftp_err("put_tree", e))?;
                Ok(())
            })
        })
        .await?;

        Ok(id)
    }

    async fn get_tree(&self, id: &TreeId) -> Result<TreeEntries> {
        use tokio::io::AsyncReadExt;
        let path = Self::tree_path(id);

        self.with_ftp(move |ftp| {
            let path = path.clone();
            Box::pin(async move {
                let mut stream = ftp.retr_as_stream(&path).await.map_err(|e| {
                    let msg = format!("{e}");
                    if msg.contains("550") || msg.contains("not found") {
                        NoaError::ObjectNotFound { id: path.clone() }.into()
                    } else {
                        Self::ftp_err("get_tree", e)
                    }
                })?;
                let mut buf = Vec::new();
                stream
                    .read_to_end(&mut buf)
                    .await
                    .map_err(|e| remote_err("ftp", format!("read stream: {e}")))?;
                ftp.finalize_retr_stream(stream)
                    .await
                    .map_err(|e| Self::ftp_err("finalize_retr", e))?;
                Ok(rmp_serde::from_slice::<TreeEntries>(&buf)?)
            })
        })
        .await
    }

    async fn has_tree(&self, id: &TreeId) -> Result<bool> {
        let path = Self::tree_path(id);

        self.with_ftp(move |ftp| {
            let path = path.clone();
            Box::pin(async move { Ok(ftp.size(&path).await.is_ok()) })
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ftp_paths() {
        let blob_id = BlobId("abc123".to_string());
        assert_eq!(FtpObjectStore::blob_path(&blob_id), "blobs/abc123");

        let tree_id = TreeId("def456".to_string());
        assert_eq!(FtpObjectStore::tree_path(&tree_id), "trees/def456");
    }

    #[test]
    fn test_ftp_config_constructor() {
        let cfg = crate::config::StorageConfig::ftp("my-ftp", "ftp.example.com");
        assert_eq!(cfg.backend_type, crate::config::StorageProtocol::Ftp);
        assert_eq!(cfg.effective_endpoint(), "ftp.example.com");
        assert_eq!(cfg.port, 21);
        assert!(!cfg.use_tls);
    }

    #[test]
    fn test_ftp_object_store_new() {
        let store = FtpObjectStore::new("ftp.example.com", 21, "user", "pass", false);
        assert_eq!(store.host, "ftp.example.com");
        assert_eq!(store.port, 21);
        assert!(!store.use_tls);
    }

    #[test]
    fn test_ftps_rejected() {
        let store = FtpObjectStore::new("ftp.example.com", 21, "user", "pass", true);
        assert!(store.use_tls);
    }
}
