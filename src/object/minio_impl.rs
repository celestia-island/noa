use async_trait::async_trait;

use aws_config::BehaviorVersion;
use aws_sdk_s3::{primitives::ByteStream, Client};
use aws_smithy_http_client::{
    tls::{self, rustls_provider::CryptoMode},
    Builder as HttpBuilder,
};

use crate::{
    error::Result,
    object::{sha256_hex, BlobId, ObjectStore, TreeEntries, TreeId},
};

fn validate_ip(ip: &std::net::IpAddr, allow_private: bool) -> Result<()> {
    match ip {
        std::net::IpAddr::V4(v4) => {
            if v4.is_loopback() || v4.is_link_local() || v4.is_broadcast() || v4.is_multicast() {
                anyhow::bail!("endpoint resolves to forbidden IP: {ip}");
            }
            if !allow_private {
                let octets = v4.octets();
                if octets[0] == 10
                    || (octets[0] == 172 && octets[1] >= 16 && octets[1] <= 31)
                    || (octets[0] == 192 && octets[1] == 168)
                {
                    anyhow::bail!("endpoint resolves to private IP: {ip}");
                }
            }
        }
        std::net::IpAddr::V6(v6) => {
            if v6.is_loopback() || v6.is_multicast() {
                anyhow::bail!("endpoint resolves to forbidden IPv6: {ip}");
            }
            if !allow_private && (v6.octets()[0] == 0xfc || v6.octets()[0] == 0xfd) {
                anyhow::bail!("endpoint resolves to forbidden IPv6: {ip}");
            }
        }
    }
    Ok(())
}

pub struct MinioObjectStore {
    client: Client,
    bucket: String,
}

impl MinioObjectStore {
    #[must_use]
    pub fn new(client: Client, bucket: String) -> Self {
        MinioObjectStore { client, bucket }
    }

    fn validate_endpoint(endpoint: &str) -> Result<()> {
        let without_scheme = endpoint
            .strip_prefix("http://")
            .or_else(|| endpoint.strip_prefix("https://"))
            .unwrap_or(endpoint);

        let host_port = without_scheme.split('/').next().unwrap_or(without_scheme);
        let host = if host_port.starts_with('[') {
            host_port
                .trim_start_matches('[')
                .split(']')
                .next()
                .unwrap_or(host_port)
        } else {
            host_port.split(':').next().unwrap_or(host_port)
        };

        let blocked = [
            "169.254.169.254",
            "metadata.google.internal",
            "metadata",
            "100.100.100.200",
            "localhost",
        ];
        for &b in &blocked {
            if host == b {
                anyhow::bail!("blocked SSRF endpoint: {host}");
            }
        }

        let allow_private = std::env::var("NOA_MINIO_ALLOW_PRIVATE")
            .is_ok_and(|v| v == "true" || v == "1" || v == "yes");

        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            validate_ip(&ip, allow_private)?;
        } else {
            use std::net::ToSocketAddrs;
            let resolved = format!("{host}:443").to_socket_addrs();
            match resolved {
                Ok(addrs) => {
                    for addr in addrs {
                        validate_ip(&addr.ip(), allow_private)?;
                    }
                }
                Err(_) => {
                    anyhow::bail!("could not resolve endpoint host for SSRF check: {host}");
                }
            }
        }
        Ok(())
    }

    pub async fn from_transport_config(config: &crate::config::StorageConfig) -> Result<Self> {
        let bucket = config
            .bucket
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("storage '{}' is missing 'bucket'", config.name))?;
        let region = config.region.as_deref().unwrap_or("us-east-1");

        match (config.access_key.as_deref(), config.secret_key.as_deref()) {
            (Some(ak), Some(sk)) => {
                let endpoint = config.effective_endpoint();
                Self::from_config(&endpoint, bucket, ak, sk, region).await
            }
            _ => Self::from_system_defaults(config.endpoint.as_deref(), bucket, region).await,
        }
    }

    pub async fn from_system_defaults(
        endpoint: Option<&str>,
        bucket: &str,
        region: &str,
    ) -> Result<Self> {
        let http_client = HttpBuilder::new()
            .tls_provider(tls::Provider::Rustls(CryptoMode::Ring))
            .build_https();
        let mut builder = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(region.to_string()))
            .http_client(http_client);
        if let Some(ep) = endpoint {
            builder = builder.endpoint_url(ep);
            Self::validate_endpoint(ep)?;
        }
        let config = builder.load().await;
        let mut s3_builder = aws_sdk_s3::config::Builder::from(&config).force_path_style(true);
        if endpoint.is_some() {
            s3_builder = s3_builder.force_path_style(true);
        }
        let client = Client::from_conf(s3_builder.build());
        Ok(MinioObjectStore {
            client,
            bucket: bucket.to_string(),
        })
    }

    pub async fn from_config(
        endpoint: &str,
        bucket: &str,
        access_key: &str,
        secret_key: &str,
        region: &str,
    ) -> Result<Self> {
        Self::validate_endpoint(endpoint)?;
        let http_client = HttpBuilder::new()
            .tls_provider(tls::Provider::Rustls(CryptoMode::Ring))
            .build_https();
        let config = aws_config::defaults(BehaviorVersion::latest())
            .region(aws_config::Region::new(region.to_string()))
            .http_client(http_client)
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
        let id = BlobId(sha256_hex(content));

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(Self::blob_key(&id))
            .body(ByteStream::from(content.to_vec()))
            .send()
            .await?;

        Ok(id)
    }

    async fn get_blob(&self, id: &BlobId) -> Result<Vec<u8>> {
        let output = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(Self::blob_key(id))
            .send()
            .await?;

        let bytes = output.body.collect().await?;
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
        match result {
            Ok(_) => Ok(true),
            Err(e) => {
                let is_not_found = e.as_service_error().is_some_and(|se| se.is_not_found());
                if is_not_found {
                    Ok(false)
                } else {
                    Err(e.into())
                }
            }
        }
    }

    async fn put_tree(&self, entries: &TreeEntries) -> Result<TreeId> {
        let data = rmp_serde::to_vec(entries)?;

        let id = TreeId(sha256_hex(&data));

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(Self::tree_key(&id))
            .body(ByteStream::from(data))
            .send()
            .await?;

        Ok(id)
    }

    async fn get_tree(&self, id: &TreeId) -> Result<TreeEntries> {
        let output = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(Self::tree_key(id))
            .send()
            .await?;

        let bytes = output.body.collect().await?;
        Ok(rmp_serde::from_slice::<TreeEntries>(&bytes.into_bytes())?)
    }

    async fn has_tree(&self, id: &TreeId) -> Result<bool> {
        let result = self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(Self::tree_key(id))
            .send()
            .await;
        match result {
            Ok(_) => Ok(true),
            Err(e) => {
                let is_not_found = e.as_service_error().is_some_and(|se| se.is_not_found());
                if is_not_found {
                    Ok(false)
                } else {
                    Err(e.into())
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_endpoint_blocks_aws_metadata() {
        assert!(MinioObjectStore::validate_endpoint("http://169.254.169.254/latest").is_err());
    }

    #[test]
    fn test_validate_endpoint_blocks_google_metadata() {
        assert!(MinioObjectStore::validate_endpoint("http://metadata.google.internal").is_err());
    }

    #[test]
    fn test_validate_endpoint_blocks_alicloud_metadata() {
        assert!(MinioObjectStore::validate_endpoint("http://100.100.100.200/latest").is_err());
    }

    #[test]
    fn test_validate_endpoint_blocks_loopback() {
        assert!(MinioObjectStore::validate_endpoint("http://127.0.0.1:9000").is_err());
        assert!(MinioObjectStore::validate_endpoint("http://localhost:9000").is_err());
    }

    #[test]
    fn test_validate_endpoint_blocks_private_ip() {
        assert!(MinioObjectStore::validate_endpoint("http://10.0.0.1:9000").is_err());
        assert!(MinioObjectStore::validate_endpoint("http://172.16.0.1:9000").is_err());
        assert!(MinioObjectStore::validate_endpoint("http://192.168.1.1:9000").is_err());
    }

    #[test]
    fn test_validate_endpoint_blocks_ipv6_loopback() {
        assert!(MinioObjectStore::validate_endpoint("http://[::1]:9000").is_err());
    }

    #[test]
    fn test_validate_endpoint_allows_public_ip() {
        assert!(MinioObjectStore::validate_endpoint("http://1.2.3.4:9000").is_ok());
    }

    #[test]
    fn test_validate_endpoint_rejects_unresolvable_domain() {
        assert!(MinioObjectStore::validate_endpoint("https://minio.example.com").is_err());
    }

    #[test]
    fn test_validate_endpoint_blocks_ipv6_ula() {
        assert!(MinioObjectStore::validate_endpoint("http://[fc00::1]:9000").is_err());
        assert!(MinioObjectStore::validate_endpoint("http://[fd00::1]:9000").is_err());
        assert!(MinioObjectStore::validate_endpoint("http://[fd12:3456::1]:9000").is_err());
    }
}
