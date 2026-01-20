use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 Request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

/// JSON-RPC 2.0 Response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// JSON-RPC 2.0 Error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// JSON-RPC 2.0 Notification (no id)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum JsonRpcMessage {
    Request(JsonRpcRequest),
    Response(JsonRpcResponse),
    Notification(JsonRpcNotification),
}

// JSON-RPC Error Codes
pub mod error_codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;

    // ACP-specific error codes
    pub const PERMISSION_DENIED: i32 = -32001;
    pub const FILE_NOT_FOUND: i32 = -32002;
    pub const WORKSPACE_NOT_INITIALIZED: i32 = -32003;
    pub const CAPABILITY_NOT_SUPPORTED: i32 = -32004;
    pub const CANCELLED: i32 = -32005;
}

impl JsonRpcResponse {
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Option<Value>, code: i32, message: String, data: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data,
            }),
        }
    }
}

impl From<crate::acp::errors::AcpError> for JsonRpcError {
    fn from(err: crate::acp::errors::AcpError) -> Self {
        use crate::acp::errors::AcpError;

        match err {
            AcpError::InvalidMessage(msg) => JsonRpcError {
                code: error_codes::PARSE_ERROR,
                message: msg,
                data: None,
            },
            AcpError::PermissionDenied(msg) => JsonRpcError {
                code: error_codes::PERMISSION_DENIED,
                message: msg,
                data: None,
            },
            AcpError::FileNotFound(msg) => JsonRpcError {
                code: error_codes::FILE_NOT_FOUND,
                message: msg,
                data: None,
            },
            AcpError::WorkspaceNotInitialized => JsonRpcError {
                code: error_codes::WORKSPACE_NOT_INITIALIZED,
                message: "Workspace not initialized".to_string(),
                data: None,
            },
            AcpError::UnsupportedCapability(msg) => JsonRpcError {
                code: error_codes::CAPABILITY_NOT_SUPPORTED,
                message: msg,
                data: None,
            },
            AcpError::InvalidRequest(msg) => JsonRpcError {
                code: error_codes::INVALID_REQUEST,
                message: msg,
                data: None,
            },
            AcpError::Cancelled => JsonRpcError {
                code: error_codes::CANCELLED,
                message: "Operation cancelled".to_string(),
                data: None,
            },
            _ => JsonRpcError {
                code: error_codes::INTERNAL_ERROR,
                message: err.to_string(),
                data: None,
            },
        }
    }
}
