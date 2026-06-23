use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::Deserialize;

use crate::{
    error::{NoaError, Result},
    object::{sha256_hex, BlobId, ObjectStore, TreeEntries, TreeId},
};

const API_PATH: &str = "/api/v0";

fn base32_encode_lower_no_pad(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"abcdefghijklmnopqrstuvwxyz234567";
    let mut result = String::with_capacity((data.len() * 8).div_ceil(5));
    let mut buffer: u64 = 0;
    let mut bits_left: u32 = 0;

    for &byte in data {
        buffer = (buffer << 8) | byte as u64;
        bits_left += 8;
        while bits_left >= 5 {
            bits_left -= 5;
            let index = ((buffer >> bits_left) & 0x1f) as usize;
            result.push(ALPHABET[index] as char);
        }
    }

    if bits_left > 0 {
        let index = ((buffer << (5 - bits_left)) & 0x1f) as usize;
        result.push(ALPHABET[index] as char);
    }

    result
}

pub fn sha256_hex_to_cid(hex: &str) -> Result<String> {
    let hash = hex::decode(hex).map_err(|e| NoaError::InvalidCid {
        cid: format!("invalid SHA-256 hex '{hex}': {e}"),
    })?;
    if hash.len() != 32 {
        return Err(NoaError::InvalidCid {
            cid: format!("SHA-256 hash must be 32 bytes, got {}", hash.len()),
        }
        .into());
    }

    let mut cid_bytes = Vec::with_capacity(36);
    cid_bytes.push(0x01);
    cid_bytes.push(0x55);
    cid_bytes.push(0x12);
    cid_bytes.push(0x20);
    cid_bytes.extend_from_slice(&hash);

    Ok(format!("b{}", base32_encode_lower_no_pad(&cid_bytes)))
}

#[derive(Debug, Deserialize)]
struct BlockPutResponse {
    key: String,
}

pub struct IpfsObjectStore {
    client: Client,
    api_url: String,
    auth_token: Option<String>,
}

impl Clone for IpfsObjectStore {
    fn clone(&self) -> Self {
        IpfsObjectStore {
            client: self.client.clone(),
            api_url: self.api_url.clone(),
            auth_token: self.auth_token.clone(),
        }
    }
}

impl IpfsObjectStore {
    #[must_use]
    pub fn new(endpoint: &str, auth_token: Option<String>) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .unwrap_or_else(|_| Client::new());

        IpfsObjectStore {
            client,
            api_url: format!("{}{}", endpoint.trim_end_matches('/'), API_PATH),
            auth_token,
        }
    }

    pub fn from_config(config: &crate::config::TransportConfig) -> Self {
        let endpoint = config.effective_endpoint();
        Self::new(&endpoint, config.auth_token.clone())
    }

    fn request(&self, path: &str) -> reqwest::RequestBuilder {
        let url = format!("{}{}", self.api_url, path);
        let mut req = self.client.post(&url);
        if let Some(ref token) = self.auth_token {
            req = req.header("Authorization", format!("Bearer {token}"));
        }
        req
    }

    async fn block_put(&self, data: Vec<u8>) -> Result<String> {
        let part = reqwest::multipart::Part::bytes(data)
            .file_name("block")
            .mime_str("application/octet-stream")
            .map_err(|e| NoaError::IpfsError {
                message: format!("multipart build error: {e}"),
            })?;

        let form = reqwest::multipart::Form::new().part("file", part);

        let resp = self
            .request("/block/put?cid-codec=raw&hash=sha2-256&mhtype=sha2-256")
            .multipart(form)
            .send()
            .await
            .map_err(|_| NoaError::IpfsDaemonUnreachable {
                endpoint: self.api_url.clone(),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(NoaError::IpfsError {
                message: format!("block/put returned {status}: {body}"),
            }
            .into());
        }

        let parsed: BlockPutResponse = resp.json().await.map_err(|e| NoaError::IpfsError {
            message: format!("failed to parse block/put response: {e}"),
        })?;

        Ok(parsed.key)
    }

    async fn block_get(&self, cid: &str) -> Result<Vec<u8>> {
        let resp = self
            .request(&format!("/block/get?arg={cid}"))
            .send()
            .await
            .map_err(|_| NoaError::IpfsDaemonUnreachable {
                endpoint: self.api_url.clone(),
            })?;

        match resp.status() {
            StatusCode::OK => {
                let bytes = resp.bytes().await.map_err(|e| NoaError::IpfsError {
                    message: format!("failed to read block/get body: {e}"),
                })?;
                Ok(bytes.to_vec())
            }
            s if s.is_client_error() || s == StatusCode::INTERNAL_SERVER_ERROR => {
                let body = resp.text().await.unwrap_or_default();
                if body.contains("not found") {
                    Err(NoaError::ObjectNotFound {
                        id: cid.to_string(),
                    }
                    .into())
                } else {
                    Err(NoaError::IpfsError {
                        message: format!("block/get failed ({s}): {body}"),
                    }
                    .into())
                }
            }
            s => Err(NoaError::IpfsError {
                message: format!("block/get unexpected status {s}"),
            }
            .into()),
        }
    }

    async fn block_stat(&self, cid: &str) -> Result<bool> {
        let resp = self
            .request(&format!("/block/stat?arg={cid}"))
            .send()
            .await
            .map_err(|_| NoaError::IpfsDaemonUnreachable {
                endpoint: self.api_url.clone(),
            })?;

        match resp.status() {
            StatusCode::OK => Ok(true),
            StatusCode::NOT_FOUND => Ok(false),
            StatusCode::INTERNAL_SERVER_ERROR => {
                let body = resp.text().await.unwrap_or_default();
                if body.contains("not found") {
                    Ok(false)
                } else {
                    Err(NoaError::IpfsError {
                        message: format!("block/stat failed: {body}"),
                    }
                    .into())
                }
            }
            s => Err(NoaError::IpfsError {
                message: format!("block/stat unexpected status {s}"),
            }
            .into()),
        }
    }

    pub async fn pin_add(&self, cid: &str) -> Result<()> {
        let resp = self
            .request(&format!("/pin/add?arg={cid}"))
            .send()
            .await
            .map_err(|_| NoaError::IpfsDaemonUnreachable {
                endpoint: self.api_url.clone(),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(NoaError::IpfsError {
                message: format!("pin/add failed ({status}): {body}"),
            }
            .into());
        }
        Ok(())
    }

    pub async fn pin_rm(&self, cid: &str) -> Result<()> {
        let resp = self
            .request(&format!("/pin/rm?arg={cid}"))
            .send()
            .await
            .map_err(|_| NoaError::IpfsDaemonUnreachable {
                endpoint: self.api_url.clone(),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(NoaError::IpfsError {
                message: format!("pin/rm failed ({status}): {body}"),
            }
            .into());
        }
        Ok(())
    }

    pub async fn version(&self) -> Result<String> {
        #[derive(Deserialize)]
        struct VersionResponse {
            #[serde(rename = "Version")]
            version: String,
        }

        let resp =
            self.request("/version")
                .send()
                .await
                .map_err(|_| NoaError::IpfsDaemonUnreachable {
                    endpoint: self.api_url.clone(),
                })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(NoaError::IpfsError {
                message: format!("version failed ({status}): {body}"),
            }
            .into());
        }

        let parsed: VersionResponse = resp.json().await.map_err(|e| NoaError::IpfsError {
            message: format!("failed to parse version response: {e}"),
        })?;

        Ok(parsed.version)
    }

    pub async fn repo_stat(&self) -> Result<RepoStat> {
        let resp = self
            .request("/repo/stat?size-only=true")
            .send()
            .await
            .map_err(|_| NoaError::IpfsDaemonUnreachable {
                endpoint: self.api_url.clone(),
            })?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(NoaError::IpfsError {
                message: format!("repo/stat failed ({status}): {body}"),
            }
            .into());
        }

        let stat: RepoStat = resp.json().await.map_err(|e| NoaError::IpfsError {
            message: format!("failed to parse repo/stat response: {e}"),
        })?;
        Ok(stat)
    }

    pub async fn gateway_url(&self, cid: &str, gateway: &str) -> String {
        format!("{}/ipfs/{}", gateway.trim_end_matches('/'), cid)
    }

    pub async fn block_get_raw(&self, cid: &str) -> Result<Vec<u8>> {
        self.block_get(cid).await
    }
}

#[derive(Debug, Deserialize)]
pub struct RepoStat {
    #[serde(rename = "RepoSize")]
    pub repo_size: u64,
    #[serde(rename = "StorageMax")]
    pub storage_max: u64,
    #[serde(rename = "NumObjects")]
    pub num_objects: u64,
}

#[async_trait]
impl ObjectStore for IpfsObjectStore {
    async fn put_blob(&self, content: &[u8]) -> Result<BlobId> {
        let id = BlobId(sha256_hex(content));
        let cid = sha256_hex_to_cid(&id.0)?;
        let expected_cid = cid.clone();

        let returned_cid = self.block_put(content.to_vec()).await?;

        if returned_cid != expected_cid {
            tracing::warn!(
                returned = %returned_cid,
                expected = %expected_cid,
                "IPFS returned unexpected CID — hash mismatch or daemon uses different codec"
            );
        }

        Ok(id)
    }

    async fn get_blob(&self, id: &BlobId) -> Result<Vec<u8>> {
        let cid = sha256_hex_to_cid(&id.0)?;
        self.block_get(&cid).await
    }

    async fn has_blob(&self, id: &BlobId) -> Result<bool> {
        let cid = sha256_hex_to_cid(&id.0)?;
        self.block_stat(&cid).await
    }

    async fn put_tree(&self, entries: &TreeEntries) -> Result<TreeId> {
        let data = rmp_serde::to_vec(entries)?;
        let id = TreeId(sha256_hex(&data));
        let cid = sha256_hex_to_cid(&id.0)?;

        let returned_cid = self.block_put(data).await?;

        if returned_cid != cid {
            tracing::warn!(
                returned = %returned_cid,
                expected = %cid,
                "IPFS tree CID mismatch"
            );
        }

        Ok(id)
    }

    async fn get_tree(&self, id: &TreeId) -> Result<TreeEntries> {
        let cid = sha256_hex_to_cid(&id.0)?;
        let bytes = self.block_get(&cid).await?;
        Ok(rmp_serde::from_slice::<TreeEntries>(&bytes)?)
    }

    async fn has_tree(&self, id: &TreeId) -> Result<bool> {
        let cid = sha256_hex_to_cid(&id.0)?;
        self.block_stat(&cid).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base32_empty() {
        assert_eq!(base32_encode_lower_no_pad(&[]), "");
    }

    #[test]
    fn test_base32_single_byte() {
        // 0x66 ('f') -> base32: "my"
        // 0x66 = 01100110
        // First 5 bits: 01100 = 12 -> 'm'
        // Remaining 3 bits: 110 -> padded to 11000 = 24 -> 'y'
        assert_eq!(base32_encode_lower_no_pad(&[0x66]), "my");
    }

    #[test]
    fn test_base32_known_values() {
        // RFC 4648 test vectors (lowercase)
        assert_eq!(base32_encode_lower_no_pad(b"f"), "my");
        assert_eq!(base32_encode_lower_no_pad(b"fo"), "mzxq");
        assert_eq!(base32_encode_lower_no_pad(b"foo"), "mzxw6");
        assert_eq!(base32_encode_lower_no_pad(b"foob"), "mzxw6yq");
        assert_eq!(base32_encode_lower_no_pad(b"fooba"), "mzxw6ytb");
        assert_eq!(base32_encode_lower_no_pad(b"foobar"), "mzxw6ytboi");
    }

    #[test]
    fn test_cid_conversion_empty_content() {
        let hex = "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855";
        let cid = sha256_hex_to_cid(hex).unwrap();
        assert!(cid.starts_with('b'));
        assert_eq!(cid.len(), 59); // 'b' prefix + 58 base32 chars for 36 bytes
    }

    #[test]
    fn test_cid_conversion_hello() {
        let hex = "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824";
        let cid = sha256_hex_to_cid(hex).unwrap();
        assert!(cid.starts_with("bafk"));
        assert_eq!(cid.len(), 59);
    }

    #[test]
    fn test_cid_deterministic() {
        let hex = "a591a6d40bf420404a011733cfb7b190d62c65bf0bcda32b57b277d9ad9f146e";
        let cid1 = sha256_hex_to_cid(hex).unwrap();
        let cid2 = sha256_hex_to_cid(hex).unwrap();
        assert_eq!(cid1, cid2);
    }

    #[test]
    fn test_cid_invalid_hex() {
        assert!(sha256_hex_to_cid("xyz").is_err());
    }

    #[test]
    fn test_cid_wrong_length() {
        // 16 bytes instead of 32
        assert!(sha256_hex_to_cid("0123456789abcdef0123456789abcdef").is_err());
    }

    #[test]
    fn test_ipfs_config_default() {
        let config = crate::config::TransportConfig::raw_ipfs("test", "http://127.0.0.1:5001");
        assert_eq!(config.protocol, "ipfs");
        assert_eq!(config.effective_endpoint(), "http://127.0.0.1:5001");
        assert!(!config.auto_pin);
    }
}
