use colored::Color;
use serde_json::Value;
use std::collections::HashMap;
use std::time::Instant;

/// Metadata about a tool for display purposes
#[derive(Debug, Clone)]
pub struct ToolMetadata {
    pub name: String,
    pub description: String,
    pub icon: &'static str,
    pub color: Option<Color>,
    pub display_format: DisplayFormat,
    pub readonly: bool,
}

/// Different display formats for different tool types
#[derive(Debug, Clone)]
pub enum DisplayFormat {
    /// File operations with optional size display
    File { show_size: bool },
    /// Command execution with optional working directory display
    Command { show_working_dir: bool },
    /// Directory listing with optional item count display
    Directory { show_item_count: bool },
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
    Simple,
    Json,
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

    /// Get icon for a specific tool
    pub fn get_icon(&self, name: &str) -> &'static str {
        self.tools.get(name).map(|m| m.icon).unwrap_or("🔧")
    }

    /// Get all registered tools
    pub fn get_all_tools(&self) -> impl Iterator<Item = &ToolMetadata> {
        self.tools.values()
    }

    /// Check if a tool is registered
    pub fn has_tool(&self, name: &str) -> bool {
        self.tools.contains_key(name)
    }

    /// Check if a tool is readonly
    pub fn is_readonly(&self, name: &str) -> bool {
        self.tools.get(name).map(|m| m.readonly).unwrap_or(false)
    }

    /// Get default metadata for unknown tools
    pub fn get_default_metadata(name: &str) -> ToolMetadata {
        ToolMetadata {
            name: name.to_string(),
            description: format!("Tool: {}", name),
            icon: "🔧",
            color: None,
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
            description: "List contents of a directory".to_string(),
            icon: "📁",
            color: Some(colored::Color::Blue),
            display_format: DisplayFormat::Directory {
                show_item_count: true,
            },
            readonly: true,
        });

        registry.register_tool(ToolMetadata {
            name: "read_file".to_string(),
            description: "Read the contents of a file".to_string(),
            icon: "📖",
            color: Some(colored::Color::Cyan),
            display_format: DisplayFormat::File { show_size: false },
            readonly: true,
        });

        registry.register_tool(ToolMetadata {
            name: "write_file".to_string(),
            description: "Write content to a file (creates file if it doesn't exist)".to_string(),
            icon: "✏️",
            color: Some(colored::Color::Green),
            display_format: DisplayFormat::File { show_size: true },
            readonly: false,
        });

        registry.register_tool(ToolMetadata {
            name: "edit_file".to_string(),
            description: "Replace specific text in a file with new text".to_string(),
            icon: "🔄",
            color: Some(colored::Color::Yellow),
            display_format: DisplayFormat::File { show_size: true },
            readonly: false,
        });

        registry.register_tool(ToolMetadata {
            name: "delete_file".to_string(),
            description: "Delete a file or directory".to_string(),
            icon: "🗑️",
            color: Some(colored::Color::Red),
            display_format: DisplayFormat::File { show_size: false },
            readonly: false,
        });

        registry.register_tool(ToolMetadata {
            name: "create_directory".to_string(),
            description: "Create a directory (and parent directories if needed)".to_string(),
            icon: "📁",
            color: Some(colored::Color::Blue),
            display_format: DisplayFormat::Directory {
                show_item_count: false,
            },
            readonly: false,
        });

        registry.register_tool(ToolMetadata {
            name: "bash".to_string(),
            description: "Execute shell commands and return the output".to_string(),
            icon: "💻",
            color: Some(colored::Color::Magenta),
            display_format: DisplayFormat::Command {
                show_working_dir: true,
            },
            readonly: false,
        });

        registry.register_tool(ToolMetadata {
            name: "search_in_files".to_string(),
            description: "Search for a string in a file or directory (recursive)".to_string(),
            icon: "🔍",
            color: Some(colored::Color::Cyan),
            display_format: DisplayFormat::Generic,
            readonly: true,
        });

        registry.register_tool(ToolMetadata {
            name: "glob".to_string(),
            description: "Find files and directories using glob patterns (read-only)".to_string(),
            icon: "🔎",
            color: Some(colored::Color::Blue),
            display_format: DisplayFormat::Generic,
            readonly: true,
        });

        registry
    }
}

/// Trait for tools that can provide their own metadata
pub trait ToolMetadataProvider {
    fn get_metadata() -> ToolMetadata;
}
