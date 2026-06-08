use serde::{Deserialize, Serialize};

use crate::error::{NoaError, Result};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcMessage {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl JsonRpcMessage {
    pub fn request(id: u64, method: &str, params: serde_json::Value) -> Self {
        JsonRpcMessage {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method: Some(method.to_string()),
            params: Some(params),
            result: None,
            error: None,
        }
    }

    pub fn response(id: u64, result: serde_json::Value) -> Self {
        JsonRpcMessage {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method: None,
            params: None,
            result: Some(result),
            error: None,
        }
    }

    pub fn error_response(id: u64, code: i64, message: &str) -> Self {
        JsonRpcMessage {
            jsonrpc: "2.0".to_string(),
            id: Some(id),
            method: None,
            params: None,
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.to_string(),
                data: None,
            }),
        }
    }

    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|e| NoaError::Serialization(e.to_string()))
    }

    pub fn from_json(s: &str) -> Result<Self> {
        serde_json::from_str(s).map_err(|e| NoaError::Serialization(e.to_string()))
    }

    pub fn to_frame(&self) -> Result<Vec<u8>> {
        let json = self.to_json()?;
        let len = json.len() as u32;
        let mut frame = Vec::with_capacity(4 + json.len());
        frame.extend_from_slice(&len.to_be_bytes());
        frame.extend_from_slice(json.as_bytes());
        Ok(frame)
    }

    pub fn from_frame(data: &[u8]) -> Result<Self> {
        if data.len() < 4 {
            return Err(NoaError::Sync("frame too short".to_string()));
        }
        let len = u32::from_be_bytes([data[0], data[1], data[2], data[3]]) as usize;
        if data.len() < 4 + len {
            return Err(NoaError::Sync("frame incomplete".to_string()));
        }
        let json = std::str::from_utf8(&data[4..4 + len])
            .map_err(|e| NoaError::Serialization(e.to_string()))?;
        Self::from_json(json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_message() {
        let msg = JsonRpcMessage::request(
            1,
            "noa.handshake",
            serde_json::json!({"workspace_id": "test"}),
        );
        assert_eq!(msg.jsonrpc, "2.0");
        assert_eq!(msg.id, Some(1));
        assert_eq!(msg.method.as_deref(), Some("noa.handshake"));
    }

    #[test]
    fn test_response_message() {
        let msg = JsonRpcMessage::response(1, serde_json::json!({"ok": true}));
        assert_eq!(msg.id, Some(1));
        assert!(msg.result.is_some());
        assert!(msg.method.is_none());
    }

    #[test]
    fn test_error_response_message() {
        let msg = JsonRpcMessage::error_response(1, -32600, "invalid request");
        assert!(msg.error.is_some());
        assert_eq!(msg.error.unwrap().code, -32600);
    }

    #[test]
    fn test_json_roundtrip() {
        let msg = JsonRpcMessage::request(
            42,
            "noa.handshake",
            serde_json::json!({"workspace_id": "ws-1", "remote_name": "origin"}),
        );
        let json = msg.to_json().unwrap();
        let parsed = JsonRpcMessage::from_json(&json).unwrap();
        assert_eq!(parsed.id, msg.id);
        assert_eq!(parsed.method, msg.method);
    }

    #[test]
    fn test_frame_roundtrip() {
        let msg = JsonRpcMessage::request(1, "test.method", serde_json::json!({"key": "value"}));
        let frame = msg.to_frame().unwrap();
        let parsed = JsonRpcMessage::from_frame(&frame).unwrap();
        assert_eq!(parsed.id, msg.id);
        assert_eq!(parsed.method, msg.method);
    }

    #[test]
    fn test_frame_too_short() {
        let result = JsonRpcMessage::from_frame(&[0, 1, 2]);
        assert!(result.is_err());
    }

    #[test]
    fn test_frame_incomplete() {
        let msg = JsonRpcMessage::request(1, "test", serde_json::json!({}));
        let json = msg.to_json().unwrap();
        let len = (json.len() as u32 + 10).to_be_bytes();
        let mut frame = Vec::new();
        frame.extend_from_slice(&len);
        frame.extend_from_slice(json.as_bytes());
        let result = JsonRpcMessage::from_frame(&frame);
        assert!(result.is_err());
    }
}
