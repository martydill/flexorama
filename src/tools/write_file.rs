use crate::security::FileSecurityManager;
use crate::tools::path::resolve_project_path;
use crate::tools::security_utils::check_file_security;
use crate::tools::types::{Tool, ToolCall, ToolResult};
use anyhow::Result;
use log::debug;
use serde_json::json;
use tokio::fs;

pub async fn write_file(
    call: &ToolCall,
    file_security_manager: &mut FileSecurityManager,
    yolo_mode: bool,
) -> Result<ToolResult> {
    let path = extract_string_arg!(call, "path");
    let content = extract_string_arg!(call, "content");

    debug!("TOOL CALL: write_file('{}', {} bytes)", path, content.len());

    let tool_use_id = call.id.clone();

    let absolute_path = match resolve_project_path(path) {
        Ok(path) => path,
        Err(error) => {
            return Ok(ToolResult {
                tool_use_id,
                content: format!("Invalid path for Write: {}", error),
                is_error: true,
            });
        }
    };

    // Check file security permissions
    if let Some(result) = check_file_security(
        "Write",
        &absolute_path,
        tool_use_id.clone(),
        file_security_manager,
        yolo_mode,
    )
    .await?
    {
        return Ok(result);
    }

    // Create parent directory if it doesn't exist
    if let Some(parent) = absolute_path.parent() {
        if let Err(e) = fs::create_dir_all(parent).await {
            return Ok(ToolResult {
                tool_use_id,
                content: format!("Error creating parent directory: {}", e),
                is_error: true,
            });
        }
    }

    match fs::write(&absolute_path, content).await {
        Ok(_) => Ok(ToolResult {
            tool_use_id,
            content: format!("Successfully wrote to file: {}", absolute_path.display()),
            is_error: false,
        }),
        Err(e) => Ok(ToolResult {
            tool_use_id,
            content: format!("Error writing to file '{}': {}", absolute_path.display(), e),
            is_error: true,
        }),
    }
}

pub fn write_file_sync(
    call: ToolCall,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ToolResult>> + Send>> {
    Box::pin(async move {
        // For sync wrapper, we need to create a temporary file security manager
        // This should only be used during tool recreation, the actual execution
        // should be handled by the Agent with proper security managers
        let mut file_security_manager =
            crate::security::FileSecurityManager::new(crate::security::FileSecurity::default());
        write_file(&call, &mut file_security_manager, false).await
    })
}

pub fn create_write_file_tool(
    file_security_manager: std::sync::Arc<tokio::sync::RwLock<FileSecurityManager>>,
    yolo_mode: bool,
) -> Tool {
    Tool {
        name: "Write".to_string(),
        description: if yolo_mode {
            "Write content to a file (YOLO MODE - no security checks)".to_string()
        } else {
            "Write content to a file (creates file if it doesn't exist)".to_string()
        },
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to write"
                },
                "content": {
                    "type": "string",
                    "description": "Content to write to the file"
                }
            },
            "required": ["path", "content"]
        }),
        handler: Box::new(move |call: ToolCall| {
            let file_security_manager = file_security_manager.clone();
            let yolo_mode = yolo_mode;
            Box::pin(async move {
                // Create a wrapper function that handles the mutable reference
                async fn write_file_wrapper(
                    call: ToolCall,
                    file_security_manager: std::sync::Arc<tokio::sync::RwLock<FileSecurityManager>>,
                    yolo_mode: bool,
                ) -> Result<ToolResult> {
                    let mut manager = file_security_manager.write().await;
                    write_file(&call, &mut *manager, yolo_mode).await
                }

                write_file_wrapper(call, file_security_manager, yolo_mode).await
            })
        }),
        metadata: None, // TODO: Add proper metadata
    }
}
