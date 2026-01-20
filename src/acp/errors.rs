use thiserror::Error;

#[derive(Debug, Error)]
pub enum AcpError {
    #[error("Invalid JSON-RPC message: {0}")]
    InvalidMessage(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("Invalid file path: {0}")]
    InvalidPath(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Agent error: {0}")]
    Agent(#[from] anyhow::Error),

    #[error("Workspace not initialized")]
    WorkspaceNotInitialized,

    #[error("Capability not supported: {0}")]
    UnsupportedCapability(String),

    #[error("Invalid request: {0}")]
    InvalidRequest(String),

    #[error("Operation cancelled")]
    Cancelled,

    #[error("Timeout")]
    Timeout,

    #[error("Unknown error: {0}")]
    Unknown(String),
}

pub type AcpResult<T> = Result<T, AcpError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invalid_message_error() {
        let error = AcpError::InvalidMessage("test message".to_string());
        assert_eq!(error.to_string(), "Invalid JSON-RPC message: test message");
    }

    #[test]
    fn test_permission_denied_error() {
        let error = AcpError::PermissionDenied("access denied".to_string());
        assert_eq!(error.to_string(), "Permission denied: access denied");
    }

    #[test]
    fn test_file_not_found_error() {
        let error = AcpError::FileNotFound("/path/to/file.txt".to_string());
        assert_eq!(error.to_string(), "File not found: /path/to/file.txt");
    }

    #[test]
    fn test_invalid_path_error() {
        let error = AcpError::InvalidPath("/invalid/path".to_string());
        assert_eq!(error.to_string(), "Invalid file path: /invalid/path");
    }

    #[test]
    fn test_workspace_not_initialized_error() {
        let error = AcpError::WorkspaceNotInitialized;
        assert_eq!(error.to_string(), "Workspace not initialized");
    }

    #[test]
    fn test_unsupported_capability_error() {
        let error = AcpError::UnsupportedCapability("feature_x".to_string());
        assert_eq!(error.to_string(), "Capability not supported: feature_x");
    }

    #[test]
    fn test_invalid_request_error() {
        let error = AcpError::InvalidRequest("bad request".to_string());
        assert_eq!(error.to_string(), "Invalid request: bad request");
    }

    #[test]
    fn test_cancelled_error() {
        let error = AcpError::Cancelled;
        assert_eq!(error.to_string(), "Operation cancelled");
    }

    #[test]
    fn test_timeout_error() {
        let error = AcpError::Timeout;
        assert_eq!(error.to_string(), "Timeout");
    }

    #[test]
    fn test_unknown_error() {
        let error = AcpError::Unknown("something went wrong".to_string());
        assert_eq!(error.to_string(), "Unknown error: something went wrong");
    }

    #[test]
    fn test_io_error_conversion() {
        let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let acp_error: AcpError = io_error.into();
        assert!(matches!(acp_error, AcpError::Io(_)));
        assert!(acp_error.to_string().contains("file not found"));
    }

    #[test]
    fn test_json_error_conversion() {
        let json_str = "{invalid json}";
        let json_result: Result<serde_json::Value, _> = serde_json::from_str(json_str);
        let json_error = json_result.unwrap_err();
        let acp_error: AcpError = json_error.into();
        assert!(matches!(acp_error, AcpError::Json(_)));
    }

    #[test]
    fn test_anyhow_error_conversion() {
        let anyhow_error = anyhow::anyhow!("test error");
        let acp_error: AcpError = anyhow_error.into();
        assert!(matches!(acp_error, AcpError::Agent(_)));
        assert_eq!(acp_error.to_string(), "Agent error: test error");
    }

    #[test]
    fn test_acp_result_ok() {
        let result: AcpResult<i32> = Ok(42);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    #[test]
    fn test_acp_result_err() {
        let result: AcpResult<i32> = Err(AcpError::Cancelled);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), AcpError::Cancelled));
    }
}
