#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::registry::{ToolRegistry, OutputMode};
    use serde_json::json;

    #[test]
    fn test_tool_registry() {
        let registry = ToolRegistry::with_builtin_tools();
        
        // Test that built-in tools are registered
        assert!(registry.has_tool("read_file"));
        assert!(registry.has_tool("write_file"));
        assert!(registry.has_tool("bash"));
        
        // Test getting metadata
        let metadata = registry.get_metadata("read_file").unwrap();
        assert_eq!(metadata.name, "read_file");
        assert_eq!(metadata.icon, "📖");
        
        // Test default metadata for unknown tool
        let default_metadata = ToolRegistry::get_default_metadata("unknown_tool");
        assert_eq!(default_metadata.name, "unknown_tool");
        assert_eq!(default_metadata.icon, "🔧");
    }

    #[test]
    fn test_display_factory() {
        let registry = ToolRegistry::with_builtin_tools();
        
        // Test creating displays
        let display = DisplayFactory::create_display(
            "read_file",
            &json!({"path": "/test/file.txt"}),
            &registry,
        );
        
        // Test creating display with specific mode
        let json_display = DisplayFactory::create_display_with_mode(
            "read_file",
            &json!({"path": "/test/file.txt"}),
            &registry,
            OutputMode::Json,
        );
        
        // Just ensure they don't panic
        drop(display);
        drop(json_display);
    }
}

