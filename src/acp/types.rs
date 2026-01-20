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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_json_rpc_request_serialization() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "test_method".to_string(),
            params: Some(json!({"key": "value"})),
        };

        let serialized = serde_json::to_string(&request).unwrap();
        assert!(serialized.contains("\"jsonrpc\":\"2.0\""));
        assert!(serialized.contains("\"method\":\"test_method\""));
        assert!(serialized.contains("\"id\":1"));
    }

    #[test]
    fn test_json_rpc_request_deserialization() {
        let json_str = r#"{"jsonrpc":"2.0","id":1,"method":"test","params":{"a":"b"}}"#;
        let request: JsonRpcRequest = serde_json::from_str(json_str).unwrap();

        assert_eq!(request.jsonrpc, "2.0");
        assert_eq!(request.id, Some(json!(1)));
        assert_eq!(request.method, "test");
        assert_eq!(request.params, Some(json!({"a": "b"})));
    }

    #[test]
    fn test_json_rpc_response_success() {
        let response = JsonRpcResponse::success(Some(json!(1)), json!({"result": "ok"}));

        assert_eq!(response.jsonrpc, "2.0");
        assert_eq!(response.id, Some(json!(1)));
        assert_eq!(response.result, Some(json!({"result": "ok"})));
        assert!(response.error.is_none());
    }

    #[test]
    fn test_json_rpc_response_error() {
        let response = JsonRpcResponse::error(
            Some(json!(1)),
            error_codes::INVALID_REQUEST,
            "Invalid request".to_string(),
            None,
        );

        assert_eq!(response.jsonrpc, "2.0");
        assert_eq!(response.id, Some(json!(1)));
        assert!(response.result.is_none());
        assert!(response.error.is_some());

        let error = response.error.unwrap();
        assert_eq!(error.code, error_codes::INVALID_REQUEST);
        assert_eq!(error.message, "Invalid request");
    }

    #[test]
    fn test_json_rpc_notification() {
        let notification = JsonRpcNotification {
            jsonrpc: "2.0".to_string(),
            method: "notify".to_string(),
            params: Some(json!({"event": "test"})),
        };

        let serialized = serde_json::to_string(&notification).unwrap();
        assert!(serialized.contains("\"jsonrpc\":\"2.0\""));
        assert!(serialized.contains("\"method\":\"notify\""));
        assert!(!serialized.contains("\"id\""));
    }

    #[test]
    fn test_json_rpc_message_request_variant() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "test".to_string(),
            params: None,
        };

        let message = JsonRpcMessage::Request(request);
        let serialized = serde_json::to_string(&message).unwrap();
        let deserialized: JsonRpcMessage = serde_json::from_str(&serialized).unwrap();

        match deserialized {
            JsonRpcMessage::Request(req) => {
                assert_eq!(req.method, "test");
            }
            _ => panic!("Expected Request variant"),
        }
    }

    #[test]
    fn test_json_rpc_message_response_variant() {
        let response = JsonRpcResponse::success(Some(json!(1)), json!({"ok": true}));
        let message = JsonRpcMessage::Response(response);

        let serialized = serde_json::to_string(&message).unwrap();
        let deserialized: JsonRpcMessage = serde_json::from_str(&serialized).unwrap();

        match deserialized {
            JsonRpcMessage::Response(resp) => {
                assert_eq!(resp.result, Some(json!({"ok": true})));
            }
            _ => panic!("Expected Response variant"),
        }
    }

    #[test]
    fn test_acp_error_to_json_rpc_error_permission_denied() {
        let acp_error = crate::acp::errors::AcpError::PermissionDenied("Access denied".to_string());
        let json_error: JsonRpcError = acp_error.into();

        assert_eq!(json_error.code, error_codes::PERMISSION_DENIED);
        assert_eq!(json_error.message, "Access denied");
    }

    #[test]
    fn test_acp_error_to_json_rpc_error_file_not_found() {
        let acp_error = crate::acp::errors::AcpError::FileNotFound("file.txt".to_string());
        let json_error: JsonRpcError = acp_error.into();

        assert_eq!(json_error.code, error_codes::FILE_NOT_FOUND);
        assert_eq!(json_error.message, "file.txt");
    }

    #[test]
    fn test_acp_error_to_json_rpc_error_workspace_not_initialized() {
        let acp_error = crate::acp::errors::AcpError::WorkspaceNotInitialized;
        let json_error: JsonRpcError = acp_error.into();

        assert_eq!(json_error.code, error_codes::WORKSPACE_NOT_INITIALIZED);
        assert_eq!(json_error.message, "Workspace not initialized");
    }

    #[test]
    fn test_acp_error_to_json_rpc_error_cancelled() {
        let acp_error = crate::acp::errors::AcpError::Cancelled;
        let json_error: JsonRpcError = acp_error.into();

        assert_eq!(json_error.code, error_codes::CANCELLED);
        assert_eq!(json_error.message, "Operation cancelled");
    }

    #[test]
    fn test_error_codes_values() {
        assert_eq!(error_codes::PARSE_ERROR, -32700);
        assert_eq!(error_codes::INVALID_REQUEST, -32600);
        assert_eq!(error_codes::METHOD_NOT_FOUND, -32601);
        assert_eq!(error_codes::INVALID_PARAMS, -32602);
        assert_eq!(error_codes::INTERNAL_ERROR, -32603);
        assert_eq!(error_codes::PERMISSION_DENIED, -32001);
        assert_eq!(error_codes::FILE_NOT_FOUND, -32002);
        assert_eq!(error_codes::WORKSPACE_NOT_INITIALIZED, -32003);
        assert_eq!(error_codes::CAPABILITY_NOT_SUPPORTED, -32004);
        assert_eq!(error_codes::CANCELLED, -32005);
    }
}
