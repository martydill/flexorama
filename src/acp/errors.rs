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
