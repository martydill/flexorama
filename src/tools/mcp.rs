use crate::mcp::{McpManager, McpTool};
use crate::tools::types::{Tool, ToolCall, ToolResult};
use log;
use serde_json::{json, Value};
use std::sync::Arc;

pub fn create_mcp_tool(server_name: &str, mcp_tool: McpTool, mcp_manager: Arc<McpManager>) -> Tool {
    let tool_name = format!("mcp_{}_{}", server_name, mcp_tool.name);
    let description = mcp_tool
        .description
        .unwrap_or_else(|| format!("MCP tool from server: {}", server_name));
    let server_name_owned = server_name.to_string();
    let mcp_manager_clone = mcp_manager.clone();
    let tool_name_original = mcp_tool.name.clone();

    // Log the creation of this MCP tool wrapper
    log::info!("🔗 Creating MCP tool wrapper:");
    log::info!("   Internal name: {}", tool_name);
    log::info!("   Original tool name: {}", tool_name_original);
    log::info!("   Server: {}", server_name);
    log::info!("   Description: {}", description);

    // Debug the raw schema data
    log::debug!(
        "   Raw input_schema from MCP: {}",
        serde_json::to_string(&mcp_tool.input_schema)
            .unwrap_or_else(|_| "Invalid JSON".to_string())
    );
    log::debug!(
        "   input_schema.is_null(): {}",
        mcp_tool.input_schema.is_null()
    );
    log::debug!(
        "   input_schema type: {}",
        match &mcp_tool.input_schema {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Number(_) => "number",
            Value::String(_) => "string",
            Value::Array(_) => "array",
            Value::Object(_) => "object",
        }
    );

    // Ensure input_schema is a valid JSON object (not null)
    let input_schema = if mcp_tool.input_schema.is_null() {
        let default_schema = json!({
            "type": "object",
            "properties": {},
            "required": []
        });
        log::warn!(
            "   ⚠️  Using default schema (MCP tool '{}' had null schema)",
            tool_name_original
        );
        default_schema
    } else {
        log::info!("   ✅ Using schema from MCP tool '{}'", tool_name_original);
        log::debug!(
            "      Schema: {}",
            serde_json::to_string(&mcp_tool.input_schema)
                .unwrap_or_else(|_| "<Invalid JSON>".to_string())
        );
        mcp_tool.input_schema
    };

    Tool {
        name: tool_name.clone(),
        description: format!("{} (MCP: {})", description, server_name),
        input_schema,
        handler: Box::new(move |call: ToolCall| {
            let server_name = server_name_owned.clone();
            let mcp_manager = mcp_manager_clone.clone();
            let tool_name_original = tool_name_original.clone();

            Box::pin(async move {
                // Log when the MCP tool is actually called
                log::info!("🚀 Executing MCP tool:");
                log::info!("   Tool: {}", tool_name_original);
                log::info!("   Server: {}", server_name);
                log::info!("   Call ID: {}", call.id);

                // Log arguments if present
                if !call.arguments.is_null() {
                    log::info!(
                        "   Arguments: {}",
                        serde_json::to_string_pretty(&call.arguments)
                            .unwrap_or_else(|_| "<Invalid JSON>".to_string())
                    );
                } else {
                    log::info!("   Arguments: <No arguments>");
                }

                // Extract the actual tool name from the mcp_ prefix
                let actual_tool_name = call
                    .name
                    .strip_prefix(&format!("mcp_{}_", server_name))
                    .unwrap_or(&tool_name_original);

                log::debug!("   Resolved tool name: {}", actual_tool_name);

                match mcp_manager
                    .call_tool(&server_name, actual_tool_name, Some(call.arguments))
                    .await
                {
                    Ok(result) => {
                        log::info!("✅ MCP tool call successful");
                        log::debug!(
                            "   Result: {}",
                            serde_json::to_string_pretty(&result)
                                .unwrap_or_else(|_| "<Invalid JSON>".to_string())
                        );

                        Ok(ToolResult {
                            tool_use_id: call.id,
                            content: serde_json::to_string_pretty(&result)
                                .unwrap_or_else(|_| "Invalid JSON result".to_string()),
                            is_error: false,
                        })
                    }
                    Err(e) => {
                        log::error!("❌ MCP tool call failed: {}", e);
                        Ok(ToolResult {
                            tool_use_id: call.id,
                            content: format!("MCP tool call failed: {}", e),
                            is_error: true,
                        })
                    }
                }
            })
        }),
        metadata: None, // TODO: Add proper metadata for MCP tools
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mcp::{McpManager, McpTool};
    use serde_json::json;
    use std::sync::Arc;

    fn create_test_mcp_tool(name: &str, description: Option<String>, input_schema: Value) -> McpTool {
        McpTool {
            name: name.to_string(),
            description,
            input_schema,
        }
    }

    #[test]
    fn test_create_mcp_tool_with_valid_schema() {
        let mcp_manager = Arc::new(McpManager::new());
        let schema = json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path"
                }
            },
            "required": ["path"]
        });

        let mcp_tool = create_test_mcp_tool(
            "read_file",
            Some("Read a file".to_string()),
            schema.clone(),
        );

        let tool = create_mcp_tool("test_server", mcp_tool, mcp_manager);

        assert_eq!(tool.name, "mcp_test_server_read_file");
        assert!(tool.description.contains("Read a file"));
        assert!(tool.description.contains("(MCP: test_server)"));
        assert_eq!(tool.input_schema, schema);
    }

    #[test]
    fn test_create_mcp_tool_with_null_schema() {
        let mcp_manager = Arc::new(McpManager::new());
        let mcp_tool = create_test_mcp_tool(
            "no_schema_tool",
            Some("Tool with no schema".to_string()),
            Value::Null,
        );

        let tool = create_mcp_tool("test_server", mcp_tool, mcp_manager);

        assert_eq!(tool.name, "mcp_test_server_no_schema_tool");

        // Should have default schema when input_schema is null
        let expected_default_schema = json!({
            "type": "object",
            "properties": {},
            "required": []
        });
        assert_eq!(tool.input_schema, expected_default_schema);
    }

    #[test]
    fn test_create_mcp_tool_without_description() {
        let mcp_manager = Arc::new(McpManager::new());
        let schema = json!({
            "type": "object",
            "properties": {},
        });

        let mcp_tool = create_test_mcp_tool("unnamed_tool", None, schema);

        let tool = create_mcp_tool("my_server", mcp_tool, mcp_manager);

        assert_eq!(tool.name, "mcp_my_server_unnamed_tool");
        // Should use default description when None is provided
        assert!(tool.description.contains("MCP tool from server: my_server"));
        assert!(tool.description.contains("(MCP: my_server)"));
    }

    #[test]
    fn test_create_mcp_tool_name_formatting() {
        let mcp_manager = Arc::new(McpManager::new());
        let schema = json!({"type": "object"});

        let mcp_tool = create_test_mcp_tool("my_tool", None, schema);
        let tool = create_mcp_tool("server_name", mcp_tool, mcp_manager);

        // Verify the name format is mcp_{server}_{tool}
        assert_eq!(tool.name, "mcp_server_name_my_tool");
    }

    #[test]
    fn test_create_mcp_tool_with_complex_schema() {
        let mcp_manager = Arc::new(McpManager::new());
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "Name parameter"
                },
                "age": {
                    "type": "number",
                    "description": "Age parameter"
                },
                "tags": {
                    "type": "array",
                    "items": {
                        "type": "string"
                    }
                }
            },
            "required": ["name", "age"]
        });

        let mcp_tool = create_test_mcp_tool(
            "complex_tool",
            Some("A complex tool".to_string()),
            schema.clone(),
        );

        let tool = create_mcp_tool("advanced_server", mcp_tool, mcp_manager);

        assert_eq!(tool.name, "mcp_advanced_server_complex_tool");
        assert_eq!(tool.input_schema, schema);

        // Verify schema structure is preserved
        let properties = tool.input_schema["properties"].as_object().unwrap();
        assert!(properties.contains_key("name"));
        assert!(properties.contains_key("age"));
        assert!(properties.contains_key("tags"));

        let required = tool.input_schema["required"].as_array().unwrap();
        assert_eq!(required.len(), 2);
    }

    #[tokio::test]
    async fn test_mcp_tool_handler_with_server_not_connected() {
        let mcp_manager = Arc::new(McpManager::new());
        let schema = json!({"type": "object"});

        let mcp_tool = create_test_mcp_tool("test_tool", None, schema);
        let tool = create_mcp_tool("nonexistent_server", mcp_tool, mcp_manager);

        // Create a test tool call
        let call = crate::tools::types::ToolCall {
            id: "test-call-1".to_string(),
            name: "mcp_nonexistent_server_test_tool".to_string(),
            arguments: json!({}),
        };

        // Call the handler
        let result = (tool.handler)(call).await.unwrap();

        // Should return an error since server is not connected
        assert!(result.is_error);
        assert!(result.content.contains("not connected") || result.content.contains("failed"));
    }

    #[test]
    fn test_create_mcp_tool_with_empty_object_schema() {
        let mcp_manager = Arc::new(McpManager::new());
        let schema = json!({});

        let mcp_tool = create_test_mcp_tool("empty_schema_tool", None, schema.clone());
        let tool = create_mcp_tool("test_server", mcp_tool, mcp_manager);

        // Empty object schema should be preserved (not replaced with default)
        assert_eq!(tool.input_schema, schema);
        assert!(!tool.input_schema.is_null());
    }

    #[test]
    fn test_create_mcp_tool_description_concatenation() {
        let mcp_manager = Arc::new(McpManager::new());
        let schema = json!({"type": "object"});

        let mcp_tool = create_test_mcp_tool(
            "my_tool",
            Some("Custom description".to_string()),
            schema,
        );
        let tool = create_mcp_tool("my_server", mcp_tool, mcp_manager);

        // Verify description format: "{description} (MCP: {server})"
        assert_eq!(tool.description, "Custom description (MCP: my_server)");
    }

    #[test]
    fn test_create_mcp_tool_metadata_is_none() {
        let mcp_manager = Arc::new(McpManager::new());
        let schema = json!({"type": "object"});

        let mcp_tool = create_test_mcp_tool("test_tool", None, schema);
        let tool = create_mcp_tool("test_server", mcp_tool, mcp_manager);

        // Verify metadata is None (as indicated by the TODO comment)
        assert!(tool.metadata.is_none());
    }
}
