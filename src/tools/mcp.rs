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
