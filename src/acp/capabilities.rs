use serde::{Deserialize, Serialize};

/// Server capabilities advertised to clients
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCapabilities {
    /// File system operations supported
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_system: Option<FileSystemCapabilities>,

    /// Tool execution supported
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<ToolCapabilities>,

    /// Streaming support
    #[serde(skip_serializing_if = "Option::is_none")]
    pub streaming: Option<bool>,

    /// Multi-turn conversation support
    #[serde(skip_serializing_if = "Option::is_none")]
    pub multi_turn: Option<bool>,

    /// Code editing support
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_editing: Option<bool>,

    /// Shell execution support
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shell_execution: Option<bool>,

    /// Progress notifications
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSystemCapabilities {
    /// Can read files
    pub read: bool,
    /// Can write files
    pub write: bool,
    /// Can list directories
    pub list: bool,
    /// Can search/glob files
    pub search: bool,
    /// Can delete files
    pub delete: bool,
    /// Can create directories
    pub create_directory: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCapabilities {
    /// List of available tools
    pub available: Vec<String>,
    /// Supports dynamic tool registration
    pub dynamic: bool,
}

/// Client capabilities sent during initialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCapabilities {
    /// Workspace capabilities
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workspace: Option<WorkspaceCapabilities>,

    /// Permission request support
    #[serde(skip_serializing_if = "Option::is_none")]
    pub permissions: Option<bool>,

    /// Progress report support
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceCapabilities {
    /// Workspace folders supported
    pub workspace_folders: bool,
}

impl Default for ServerCapabilities {
    fn default() -> Self {
        Self {
            file_system: Some(FileSystemCapabilities {
                read: true,
                write: true,
                list: true,
                search: true,
                delete: true,
                create_directory: true,
            }),
            tools: Some(ToolCapabilities {
                available: vec![], // Will be populated at runtime
                dynamic: true,
            }),
            streaming: Some(true),
            multi_turn: Some(true),
            code_editing: Some(true),
            shell_execution: Some(true),
            progress: Some(true),
        }
    }
}

impl ServerCapabilities {
    /// Create capabilities with yolo mode (all permissions granted)
    pub fn with_yolo_mode() -> Self {
        Self::default()
    }

    /// Create capabilities with plan mode (read-only)
    pub fn with_plan_mode() -> Self {
        Self {
            file_system: Some(FileSystemCapabilities {
                read: true,
                write: false,
                list: true,
                search: true,
                delete: false,
                create_directory: false,
            }),
            tools: Some(ToolCapabilities {
                available: vec![],
                dynamic: false,
            }),
            streaming: Some(true),
            multi_turn: Some(true),
            code_editing: Some(false),
            shell_execution: Some(false),
            progress: Some(true),
        }
    }

    /// Negotiate capabilities based on client capabilities
    pub fn negotiate(&mut self, _client_caps: &ClientCapabilities) {
        // For now, we advertise our full capabilities
        // In the future, we can restrict based on client caps
    }
}
