use async_trait::async_trait;

use aws_config::BehaviorVersion;
use aws_sdk_s3::{primitives::ByteStream, Client};

use crate::{
    error::{NoaError, Result},
    object::{BlobId, ObjectStore, TreeEntries, TreeId},
};

pub struct MinioObjectStore {
    client: Client,
    bucket: String,
}

impl MinioObjectStore {
    pub fn new(client: Client, bucket: String) -> Self {
        MinioObjectStore { client, bucket }
    }

    fn validate_endpoint(endpoint: &str) -> Result<()> {
        let without_scheme = endpoint
            .strip_prefix("http://")
            .or_else(|| endpoint.strip_prefix("https://"))
            .unwrap_or(endpoint);

        let host_port = without_scheme.split('/').next().unwrap_or(without_scheme);
        let host = host_port.split(':').next().unwrap_or(host_port);

        let blocked = [
            "169.254.169.254",
            "metadata.google.internal",
            "metadata",
            "100.100.100.200",
        ];
        for &b in &blocked {
            if host == b {
                return Err(NoaError::Config(format!("blocked SSRF endpoint: {}", host)));
            }
        }
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            match ip {
                std::net::IpAddr::V4(v4) => {
                    if v4.is_loopback()
                        || v4.is_link_local()
                        || v4.is_broadcast()
                        || v4.is_multicast()
                    {
                        return Err(NoaError::Config(format!(
                            "endpoint resolves to forbidden IP: {}",
                            ip
                        )));
                    }
                    let octets = v4.octets();
                    if octets[0] == 10
                        || (octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31)
                        || (octets[0] == 192 && octets[1] == 168)
                    {
                        return Err(NoaError::Config(format!(
                            "endpoint resolves to private IP: {}",
                            ip
                        )));
                    }
                }
                std::net::IpAddr::V6(_) => {}
            }
        }
        Ok(())
    }

    pub async fn from_config(
        endpoint: &str,
        bucket: &str,
        access_key: &str,
        secret_key: &str,
        region: &str,
    ) -> Result<Self> {
        Self::validate_endpoint(endpoint)?;
        let config = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(region.to_string()))
            .endpoint_url(endpoint)
            .credentials_provider(aws_sdk_s3::config::Credentials::new(
                access_key,
                secret_key,
                None,
                None,
                "noa-minio",
            ))
            .load()
            .await;

        let s3_config = aws_sdk_s3::config::Builder::from(&config)
            .force_path_style(true)
            .build();

        let client = Client::from_conf(s3_config);

        Ok(MinioObjectStore {
            client,
            bucket: bucket.to_string(),
        })
    }

    fn blob_key(id: &BlobId) -> String {
        format!("blobs/{}", id.0)
    }

    fn tree_key(id: &TreeId) -> String {
        format!("trees/{}", id.0)
    }
}

#[async_trait]
impl ObjectStore for MinioObjectStore {
    async fn put_blob(&self, content: &[u8]) -> Result<BlobId> {
        use sha2::{Digest, Sha256};
        let hash = hex::encode(Sha256::digest(content));
        let id = BlobId(hash);

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(Self::blob_key(&id))
            .body(ByteStream::from(content.to_vec()))
            .send()
            .await
            .map_err(|e| NoaError::Remote(e.to_string()))?;

        Ok(id)
    }

    async fn get_blob(&self, id: &BlobId) -> Result<Vec<u8>> {
        let output = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(Self::blob_key(id))
            .send()
            .await
            .map_err(|e| NoaError::ObjectNotFound(e.to_string()))?;

        let bytes = output
            .body
            .collect()
            .await
            .map_err(|e| NoaError::Remote(e.to_string()))?;
        Ok(bytes.into_bytes().to_vec())
    }

    async fn has_blob(&self, id: &BlobId) -> Result<bool> {
        let result = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(Self::blob_key(id))
            .send()
            .await;
        Ok(result.is_ok())
    }

    async fn put_tree(&self, entries: &TreeEntries) -> Result<TreeId> {
        let data =
            rmp_serde::to_vec(entries).map_err(|e| NoaError::Serialization(e.to_string()))?;

        use sha2::{Digest, Sha256};
        let hash = hex::encode(Sha256::digest(&data));
        let id = TreeId(hash);

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(Self::tree_key(&id))
            .body(ByteStream::from(data))
            .send()
            .await
            .map_err(|e| NoaError::Remote(e.to_string()))?;

        Ok(id)
    }

    async fn get_tree(&self, id: &TreeId) -> Result<TreeEntries> {
        let output = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(Self::tree_key(id))
            .send()
            .await
            .map_err(|e| NoaError::ObjectNotFound(e.to_string()))?;

        let bytes = output
            .body
            .collect()
            .await
            .map_err(|e| NoaError::Remote(e.to_string()))?;
        rmp_serde::from_slice(&bytes.into_bytes())
            .map_err(|e| NoaError::Serialization(e.to_string()))
    }

    async fn has_tree(&self, id: &TreeId) -> Result<bool> {
        let result = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(Self::tree_key(id))
            .send()
            .await;
        Ok(result.is_ok())
    }
}
