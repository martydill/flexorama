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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_capabilities_default() {
        let caps = ServerCapabilities::default();

        assert!(caps.file_system.is_some());
        let fs = caps.file_system.unwrap();
        assert!(fs.read);
        assert!(fs.write);
        assert!(fs.list);
        assert!(fs.search);
        assert!(fs.delete);
        assert!(fs.create_directory);

        assert!(caps.tools.is_some());
        let tools = caps.tools.unwrap();
        assert!(tools.dynamic);

        assert_eq!(caps.streaming, Some(true));
        assert_eq!(caps.multi_turn, Some(true));
        assert_eq!(caps.code_editing, Some(true));
        assert_eq!(caps.shell_execution, Some(true));
        assert_eq!(caps.progress, Some(true));
    }

    #[test]
    fn test_server_capabilities_yolo_mode() {
        let caps = ServerCapabilities::with_yolo_mode();
        let default_caps = ServerCapabilities::default();

        // Yolo mode should be same as default (all permissions)
        assert_eq!(
            serde_json::to_string(&caps).unwrap(),
            serde_json::to_string(&default_caps).unwrap()
        );
    }

    #[test]
    fn test_server_capabilities_plan_mode() {
        let caps = ServerCapabilities::with_plan_mode();

        assert!(caps.file_system.is_some());
        let fs = caps.file_system.unwrap();
        assert!(fs.read); // Can read
        assert!(!fs.write); // Cannot write
        assert!(fs.list); // Can list
        assert!(fs.search); // Can search
        assert!(!fs.delete); // Cannot delete
        assert!(!fs.create_directory); // Cannot create

        assert!(caps.tools.is_some());
        let tools = caps.tools.unwrap();
        assert!(!tools.dynamic); // No dynamic tools in plan mode

        assert_eq!(caps.streaming, Some(true));
        assert_eq!(caps.multi_turn, Some(true));
        assert_eq!(caps.code_editing, Some(false)); // No editing in plan mode
        assert_eq!(caps.shell_execution, Some(false)); // No shell in plan mode
        assert_eq!(caps.progress, Some(true));
    }

    #[test]
    fn test_file_system_capabilities_serialization() {
        let fs_caps = FileSystemCapabilities {
            read: true,
            write: false,
            list: true,
            search: false,
            delete: false,
            create_directory: true,
        };

        let serialized = serde_json::to_string(&fs_caps).unwrap();
        let deserialized: FileSystemCapabilities = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.read, fs_caps.read);
        assert_eq!(deserialized.write, fs_caps.write);
        assert_eq!(deserialized.list, fs_caps.list);
        assert_eq!(deserialized.search, fs_caps.search);
        assert_eq!(deserialized.delete, fs_caps.delete);
        assert_eq!(deserialized.create_directory, fs_caps.create_directory);
    }

    #[test]
    fn test_tool_capabilities_serialization() {
        let tool_caps = ToolCapabilities {
            available: vec!["tool1".to_string(), "tool2".to_string()],
            dynamic: true,
        };

        let serialized = serde_json::to_string(&tool_caps).unwrap();
        let deserialized: ToolCapabilities = serde_json::from_str(&serialized).unwrap();

        assert_eq!(deserialized.available, tool_caps.available);
        assert_eq!(deserialized.dynamic, tool_caps.dynamic);
    }

    #[test]
    fn test_client_capabilities_serialization() {
        let client_caps = ClientCapabilities {
            workspace: Some(WorkspaceCapabilities {
                workspace_folders: true,
            }),
            permissions: Some(true),
            progress: Some(true),
        };

        let serialized = serde_json::to_string(&client_caps).unwrap();
        let deserialized: ClientCapabilities = serde_json::from_str(&serialized).unwrap();

        assert!(deserialized.workspace.is_some());
        assert_eq!(deserialized.permissions, Some(true));
        assert_eq!(deserialized.progress, Some(true));
    }

    #[test]
    fn test_server_capabilities_negotiate() {
        let mut caps = ServerCapabilities::default();
        let client_caps = ClientCapabilities {
            workspace: Some(WorkspaceCapabilities {
                workspace_folders: true,
            }),
            permissions: Some(true),
            progress: Some(false),
        };

        caps.negotiate(&client_caps);

        // For now, negotiate doesn't change anything
        // This test ensures it doesn't panic
        assert!(caps.file_system.is_some());
    }

    #[test]
    fn test_workspace_capabilities() {
        let workspace_caps = WorkspaceCapabilities {
            workspace_folders: true,
        };

        assert!(workspace_caps.workspace_folders);
    }

    #[test]
    fn test_server_capabilities_optional_fields() {
        let caps = ServerCapabilities {
            file_system: None,
            tools: None,
            streaming: None,
            multi_turn: None,
            code_editing: None,
            shell_execution: None,
            progress: None,
        };

        let serialized = serde_json::to_string(&caps).unwrap();
        // All fields should be omitted when None
        assert_eq!(serialized, "{}");
    }

    #[test]
    fn test_client_capabilities_optional_fields() {
        let caps = ClientCapabilities {
            workspace: None,
            permissions: None,
            progress: None,
        };

        let serialized = serde_json::to_string(&caps).unwrap();
        assert_eq!(serialized, "{}");
    }
}
