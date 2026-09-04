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
    Simple,
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
            name: "Read".to_string(),
            icon: "📖",
            display_format: DisplayFormat::File { show_size: false },
            readonly: true,
        });

        registry.register_tool(ToolMetadata {
            name: "Write".to_string(),
            icon: "✏️",
            display_format: DisplayFormat::File { show_size: true },
            readonly: false,
        });

        registry.register_tool(ToolMetadata {
            name: "Edit".to_string(),
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
            name: "Bash".to_string(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_registry_starts_empty() {
        let registry = ToolRegistry::new();

        assert_eq!(registry.get_all_tools().count(), 0);
        assert!(registry.get_metadata("Read").is_none());
        assert!(!registry.is_readonly("Read"));
    }

    #[test]
    fn register_and_retrieve_tool_metadata() {
        let mut registry = ToolRegistry::new();
        registry.register_tool(ToolMetadata {
            name: "custom".to_string(),
            icon: "🧪",
            display_format: DisplayFormat::File { show_size: true },
            readonly: true,
        });

        let metadata = registry.get_metadata("custom").expect("registered tool");
        assert_eq!(metadata.icon, "🧪");
        assert!(metadata.readonly);
        assert!(matches!(metadata.display_format, DisplayFormat::File { show_size: true }));

        assert!(registry.is_readonly("custom"));
        assert_eq!(registry.get_all_tools().count(), 1);
    }

    #[test]
    fn registering_same_name_replaces_entry() {
        let mut registry = ToolRegistry::new();
        registry.register_tool(ToolMetadata {
            name: "Read".to_string(),
            icon: "📖",
            display_format: DisplayFormat::File { show_size: false },
            readonly: true,
        });
        registry.register_tool(ToolMetadata {
            name: "Read".to_string(),
            icon: "📘",
            display_format: DisplayFormat::Generic,
            readonly: false,
        });

        assert_eq!(registry.get_all_tools().count(), 1);
        assert_eq!(registry.get_metadata("Read").unwrap().icon, "📘");
        assert!(!registry.is_readonly("Read"));
    }

    #[test]
    fn unknown_tools_default_to_generic_non_readonly_metadata() {
        let default = ToolRegistry::get_default_metadata("mystery_tool");

        assert_eq!(default.name, "mystery_tool");
        assert_eq!(default.icon, "🔧");
        assert!(matches!(default.display_format, DisplayFormat::Generic));
        assert!(!default.readonly);
    }

    #[test]
    fn builtin_registry_contains_expected_tools() {
        let registry = ToolRegistry::with_builtin_tools();
        let names: Vec<&str> = registry.get_all_tools().map(|t| t.name.as_str()).collect();

        for expected in [
            "list_directory",
            "Read",
            "Write",
            "Edit",
            "delete_file",
            "create_directory",
            "Bash",
            "search_in_files",
            "glob",
            "use_skill",
        ] {
            assert!(
                names.contains(&expected),
                "builtin registry missing '{}'",
                expected
            );
        }
        assert_eq!(names.len(), 10);
    }

    #[test]
    fn builtin_readonly_flags_match_tool_semantics() {
        let registry = ToolRegistry::with_builtin_tools();

        for readonly_tool in ["Read", "list_directory", "search_in_files", "glob", "use_skill"] {
            assert!(
                registry.is_readonly(readonly_tool),
                "'{}' should be readonly",
                readonly_tool
            );
        }

        for mutating_tool in ["Write", "Edit", "delete_file", "create_directory", "Bash"] {
            assert!(
                !registry.is_readonly(mutating_tool),
                "'{}' should not be readonly",
                mutating_tool
            );
        }
    }
}
