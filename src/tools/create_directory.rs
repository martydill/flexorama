use crate::security::FileSecurityManager;
use crate::tools::types::{Tool, ToolCall, ToolResult};
use anyhow::Result;
use log::{debug, info};
use path_absolutize::*;
use serde_json::json;
use shellexpand;
use std::path::Path;
use tokio::fs;

pub async fn create_directory(
    call: &ToolCall,
    file_security_manager: &mut FileSecurityManager,
    yolo_mode: bool,
) -> Result<ToolResult> {
    let path = call
        .arguments
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;

    debug!("TOOL CALL: create_directory('{}')", path);

    let tool_use_id = call.id.clone();

    let expanded_path = shellexpand::tilde(path);
    let absolute_path = Path::new(&*expanded_path).absolutize()?;

    // Check file security permissions
    if yolo_mode {
        debug!(
            "YOLO MODE: Bypassing file security for 'create_directory' on '{}'",
            absolute_path.display()
        );
    } else {
        match file_security_manager
            .check_file_permission("create_directory", &absolute_path.to_string_lossy())
        {
            crate::security::FilePermissionResult::Allowed => {
                debug!(
                    "File operation 'create_directory' on '{}' is allowed by security policy",
                    absolute_path.display()
                );
            }
            crate::security::FilePermissionResult::Denied => {
                return Ok(ToolResult {
                    tool_use_id,
                    content: format!("🔒 Security: Directory create operation on '{}' is not allowed by security policy.", absolute_path.display()),
                    is_error: true,
                });
            }
            crate::security::FilePermissionResult::RequiresPermission => {
                // Ask user for permission
                match file_security_manager
                    .ask_file_permission("create_directory", &absolute_path.to_string_lossy())
                    .await
                {
                    Ok(Some(_)) => {
                        // User granted permission
                        info!(
                            "User granted permission for directory create operation: {}",
                            absolute_path.display()
                        );
                    }
                    Ok(None) => {
                        return Ok(ToolResult {
                            tool_use_id,
                            content: format!("🔒 Security: Permission denied for directory create operation on '{}'", absolute_path.display()),
                            is_error: true,
                        });
                    }
                    Err(e) => {
                        return Ok(ToolResult {
                            tool_use_id,
                            content: format!("🔒 Security: Error checking permission for directory create operation on '{}': {}", absolute_path.display(), e),
                            is_error: true,
                        });
                    }
                }
            }
        }
    }

    match fs::create_dir_all(&absolute_path).await {
        Ok(_) => Ok(ToolResult {
            tool_use_id,
            content: format!(
                "Successfully created directory: {}",
                absolute_path.display()
            ),
            is_error: false,
        }),
        Err(e) => Ok(ToolResult {
            tool_use_id,
            content: format!(
                "Error creating directory '{}': {}",
                absolute_path.display(),
                e
            ),
            is_error: true,
        }),
    }
}

pub fn create_directory_sync(
    call: ToolCall,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ToolResult>> + Send>> {
    Box::pin(async move {
        // For sync wrapper, we need to create a temporary file security manager
        // This should only be used during tool recreation, the actual execution
        // should be handled by the Agent with proper security managers
        let mut file_security_manager =
            crate::security::FileSecurityManager::new(crate::security::FileSecurity::default());
        create_directory(&call, &mut file_security_manager, false).await
    })
}

pub fn create_create_directory_tool(
    file_security_manager: std::sync::Arc<tokio::sync::RwLock<FileSecurityManager>>,
    yolo_mode: bool,
) -> Tool {
    Tool {
        name: "create_directory".to_string(),
        description: if yolo_mode {
            "Create a directory (YOLO MODE - no security checks)".to_string()
        } else {
            "Create a directory (and parent directories if needed)".to_string()
        },
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the directory to create"
                }
            },
            "required": ["path"]
        }),
        handler: Box::new(move |call: ToolCall| {
            let file_security_manager = file_security_manager.clone();
            let yolo_mode = yolo_mode;
            Box::pin(async move {
                // Create a wrapper function that handles the mutable reference
                async fn create_directory_wrapper(
                    call: ToolCall,
                    file_security_manager: std::sync::Arc<tokio::sync::RwLock<FileSecurityManager>>,
                    yolo_mode: bool,
                ) -> Result<ToolResult> {
                    let mut manager = file_security_manager.write().await;
                    create_directory(&call, &mut *manager, yolo_mode).await
                }

                create_directory_wrapper(call, file_security_manager, yolo_mode).await
            })
        }),
        metadata: None, // TODO: Add proper metadata
    }
}
