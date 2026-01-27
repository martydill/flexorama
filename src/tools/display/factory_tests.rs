#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::registry::{ToolRegistry, OutputMode};
    use serde_json::json;

    #[test]
    fn test_tool_registry() {
        let registry = ToolRegistry::with_builtin_tools();

        // Test that built-in tools are registered
        assert!(registry.has_tool("Read"));
        assert!(registry.has_tool("Write"));
        assert!(registry.has_tool("Bash"));

        // Test getting metadata
        let metadata = registry.get_metadata("Read").unwrap();
        assert_eq!(metadata.name, "Read");
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
            "Read",
            &json!({"path": "/test/file.txt"}),
            &registry,
        );

        // Test creating display with specific mode
        let json_display = DisplayFactory::create_display_with_mode(
            "Read",
            &json!({"path": "/test/file.txt"}),
            &registry,
            OutputMode::Json,
        );

        // Just ensure they don't panic
        drop(display);
        drop(json_display);
    }
}

