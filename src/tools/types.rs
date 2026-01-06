use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type AsyncToolHandler = Box<
    dyn Fn(
            ToolCall,
        )
            -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ToolResult>> + Send>>
        + Send
        + Sync,
>;

pub struct Tool {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub handler: AsyncToolHandler,
    pub metadata: Option<crate::tools::registry::ToolMetadata>,
}

impl Clone for Tool {
    fn clone(&self) -> Self {
        Self {
            name: self.name.clone(),
            description: self.description.clone(),
            input_schema: self.input_schema.clone(),
            // Note: We can't clone the function pointer directly, so we recreate it
            handler: self.recreate_handler(),
            metadata: self.metadata.clone(),
        }
    }
}

impl Tool {
    fn recreate_handler(&self) -> AsyncToolHandler {
        match self.name.as_str() {
            "list_directory" => Box::new(crate::tools::list_directory::list_directory_sync),
            "read_file" => Box::new(crate::tools::read_file::read_file_sync),
            "search_in_files" => Box::new(crate::tools::search_in_files::search_in_files_sync),
            "glob" => Box::new(crate::tools::glob::glob_files_sync),
            "write_file" => Box::new(crate::tools::write_file::write_file_sync),
            "edit_file" => Box::new(crate::tools::edit_file::edit_file_sync),
            "delete_file" => Box::new(crate::tools::delete_file::delete_file_sync),
            "create_directory" => Box::new(crate::tools::create_directory::create_directory_sync),
            "bash" => {
                // For bash tool, we need to create a handler that will be updated later
                // with the security manager. This is a limitation of the current architecture.
                Box::new(|_call| {
                    Box::pin(async move {
                        // This should be handled by the Agent's tool execution logic
                        // The Agent has special handling for bash commands with security
                        Err(anyhow::anyhow!("Bash tool should be handled by Agent with security manager. Tool recreation failed."))
                    })
                })
            }
            _ if self.name.starts_with("mcp_") => {
                // This is an MCP tool - we need to handle this differently
                // The issue is that we can't recreate MCP handlers without the MCP manager
                // So we'll create a placeholder that indicates the issue
                Box::new(|call| {
                    Box::pin(async move {
                        Err(anyhow::anyhow!("MCP tool '{}' cannot be recreated without proper MCP manager context. This suggests there's an issue with how MCP tools are being cloned or moved.", call.name))
                    })
                })
            }
            _ => panic!("Unknown tool: {}", self.name),
        }
    }
}

impl std::fmt::Debug for Tool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Tool")
            .field("name", &self.name)
            .field("description", &self.description)
            .field("input_schema", &self.input_schema)
            .field("handler", &"<async_handler>")
            .field("metadata", &self.metadata)
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub content: String,
    pub is_error: bool,
}
