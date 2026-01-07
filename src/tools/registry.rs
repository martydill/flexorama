use serde_json::Value;
use std::collections::HashMap;
use std::time::Instant;

/// Metadata about a tool for display purposes
#[derive(Debug, Clone)]
pub struct ToolMetadata {
    pub name: String,
    pub icon: &'static str,
    pub display_format: DisplayFormat,
    pub readonly: bool,
}

/// Different display formats for different tool types
#[derive(Debug, Clone)]
pub enum DisplayFormat {
    /// File operations with optional size display
    File { show_size: bool },
    /// Command execution with optional working directory display
    Command,
    /// Directory listing with optional item count display
    Directory,
    /// Generic tool with no special formatting
    Generic,
}

/// Context information for tool display
#[derive(Debug, Clone)]
pub struct DisplayContext {
    pub tool_name: String,
    pub arguments: Value,
    pub start_time: Instant,
    pub metadata: ToolMetadata,
    pub output_mode: OutputMode,
}

/// Different output modes for tool display
#[derive(Debug, Clone, PartialEq)]
pub enum OutputMode {
    Pretty,
}

/// Registry for managing tool metadata
#[derive(Debug, Default)]
pub struct ToolRegistry {
    tools: HashMap<String, ToolMetadata>,
}

impl ToolRegistry {
    /// Create a new empty tool registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a tool with its metadata
    pub fn register_tool(&mut self, metadata: ToolMetadata) {
        self.tools.insert(metadata.name.clone(), metadata);
    }

    /// Get metadata for a specific tool
    pub fn get_metadata(&self, name: &str) -> Option<&ToolMetadata> {
        self.tools.get(name)
    }

    /// Get all registered tools
    pub fn get_all_tools(&self) -> impl Iterator<Item = &ToolMetadata> {
        self.tools.values()
    }

    /// Check if a tool is readonly
    pub fn is_readonly(&self, name: &str) -> bool {
        self.tools.get(name).map(|m| m.readonly).unwrap_or(false)
    }

    /// Get default metadata for unknown tools
    pub fn get_default_metadata(name: &str) -> ToolMetadata {
        ToolMetadata {
            name: name.to_string(),
            icon: "🔧",
            display_format: DisplayFormat::Generic,
            readonly: false,
        }
    }

    /// Initialize the registry with built-in tool metadata
    pub fn with_builtin_tools() -> Self {
        let mut registry = Self::new();

        // Register built-in tools
        registry.register_tool(ToolMetadata {
            name: "list_directory".to_string(),
            icon: "📁",
            display_format: DisplayFormat::Directory,
            readonly: true,
        });

        registry.register_tool(ToolMetadata {
            name: "read_file".to_string(),
            icon: "📖",
            display_format: DisplayFormat::File { show_size: false },
            readonly: true,
        });

        registry.register_tool(ToolMetadata {
            name: "write_file".to_string(),
            icon: "✏️",
            display_format: DisplayFormat::File { show_size: true },
            readonly: false,
        });

        registry.register_tool(ToolMetadata {
            name: "edit_file".to_string(),
            icon: "🔄",
            display_format: DisplayFormat::File { show_size: true },
            readonly: false,
        });

        registry.register_tool(ToolMetadata {
            name: "delete_file".to_string(),
            icon: "🗑️",
            display_format: DisplayFormat::File { show_size: false },
            readonly: false,
        });

        registry.register_tool(ToolMetadata {
            name: "create_directory".to_string(),
            icon: "📁",
            display_format: DisplayFormat::Directory,
            readonly: false,
        });

        registry.register_tool(ToolMetadata {
            name: "bash".to_string(),
            icon: "💻",
            display_format: DisplayFormat::Command,
            readonly: false,
        });

        registry.register_tool(ToolMetadata {
            name: "search_in_files".to_string(),
            icon: "🔍",
            display_format: DisplayFormat::Generic,
            readonly: true,
        });

        registry.register_tool(ToolMetadata {
            name: "glob".to_string(),
            icon: "🔎",
            display_format: DisplayFormat::Generic,
            readonly: true,
        });

        registry.register_tool(ToolMetadata {
            name: "use_skill".to_string(),
            icon: "🎯",
            display_format: DisplayFormat::Generic,
            readonly: true,
        });

        registry
    }
}
