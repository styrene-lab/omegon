//! RPC protocol types (JSON-RPC 2.0 over stdin/stdout).

use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::ErrorCode;

/// Top-level RPC message — either a request or notification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcMessage {
    Request(RpcRequest),
    Notification(RpcNotification),
}

/// RPC request — expects a response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// RPC notification — no response expected.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

/// RPC response (either success or error).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl RpcResponse {
    /// Build a success response.
    pub fn success(id: Option<String>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Build an error response.
    pub fn error(id: Option<String>, code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(RpcError {
                code: code.to_string(),
                message: message.into(),
                data: None,
            }),
        }
    }
}

/// RPC error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rpc_request_roundtrip() {
        let req = RpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some("1".to_string()),
            method: "get_tools".to_string(),
            params: serde_json::json!({}),
        };

        let json = serde_json::to_string(&req).unwrap();
        let parsed: RpcRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.id, Some("1".to_string()));
        assert_eq!(parsed.method, "get_tools");
    }

    #[test]
    fn test_rpc_response_success() {
        let resp = RpcResponse::success(
            Some("1".to_string()),
            serde_json::json!({"status": "ok"}),
        );

        assert_eq!(resp.id, Some("1".to_string()));
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    #[test]
    fn test_rpc_response_error() {
        let resp = RpcResponse::error(
            Some("1".to_string()),
            ErrorCode::MethodNotFound,
            "method not found",
        );

        assert_eq!(resp.id, Some("1".to_string()));
        assert!(resp.result.is_none());
        assert!(resp.error.is_some());
    }
}
