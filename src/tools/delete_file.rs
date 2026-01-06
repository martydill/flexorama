use crate::security::FileSecurityManager;
use crate::tools::types::{Tool, ToolCall, ToolResult};
use anyhow::Result;
use log::{debug, info};
use path_absolutize::*;
use serde_json::json;
use shellexpand;
use std::path::Path;
use tokio::fs;

pub async fn delete_file(
    call: &ToolCall,
    file_security_manager: &mut FileSecurityManager,
    yolo_mode: bool,
) -> Result<ToolResult> {
    let path = call
        .arguments
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;

    debug!("TOOL CALL: delete_file('{}')", path);

    let tool_use_id = call.id.clone();

    let expanded_path = shellexpand::tilde(path);
    let absolute_path = Path::new(&*expanded_path).absolutize()?;

    // Check file security permissions
    if yolo_mode {
        debug!(
            "YOLO MODE: Bypassing file security for 'delete_file' on '{}'",
            absolute_path.display()
        );
    } else {
        match file_security_manager
            .check_file_permission("delete_file", &absolute_path.to_string_lossy())
        {
            crate::security::FilePermissionResult::Allowed => {
                debug!(
                    "File operation 'delete_file' on '{}' is allowed by security policy",
                    absolute_path.display()
                );
            }
            crate::security::FilePermissionResult::Denied => {
                return Ok(ToolResult {
                    tool_use_id,
                    content: format!("🔒 Security: File delete operation on '{}' is not allowed by security policy.", absolute_path.display()),
                    is_error: true,
                });
            }
            crate::security::FilePermissionResult::RequiresPermission => {
                // Ask user for permission
                match file_security_manager
                    .ask_file_permission("delete_file", &absolute_path.to_string_lossy())
                    .await
                {
                    Ok(Some(_)) => {
                        // User granted permission
                        info!(
                            "User granted permission for file delete operation: {}",
                            absolute_path.display()
                        );
                    }
                    Ok(None) => {
                        return Ok(ToolResult {
                            tool_use_id,
                            content: format!(
                                "🔒 Security: Permission denied for file delete operation on '{}'",
                                absolute_path.display()
                            ),
                            is_error: true,
                        });
                    }
                    Err(e) => {
                        return Ok(ToolResult {
                            tool_use_id,
                            content: format!("🔒 Security: Error checking permission for file delete operation on '{}': {}", absolute_path.display(), e),
                            is_error: true,
                        });
                    }
                }
            }
        }
    }

    match fs::metadata(&absolute_path).await {
        Ok(metadata) => {
            if metadata.is_dir() {
                match fs::remove_dir_all(&absolute_path).await {
                    Ok(_) => Ok(ToolResult {
                        tool_use_id,
                        content: format!(
                            "Successfully deleted directory: {}",
                            absolute_path.display()
                        ),
                        is_error: false,
                    }),
                    Err(e) => Ok(ToolResult {
                        tool_use_id,
                        content: format!(
                            "Error deleting directory '{}': {}",
                            absolute_path.display(),
                            e
                        ),
                        is_error: true,
                    }),
                }
            } else {
                match fs::remove_file(&absolute_path).await {
                    Ok(_) => Ok(ToolResult {
                        tool_use_id,
                        content: format!("Successfully deleted file: {}", absolute_path.display()),
                        is_error: false,
                    }),
                    Err(e) => Ok(ToolResult {
                        tool_use_id,
                        content: format!(
                            "Error deleting file '{}': {}",
                            absolute_path.display(),
                            e
                        ),
                        is_error: true,
                    }),
                }
            }
        }
        Err(e) => Ok(ToolResult {
            tool_use_id,
            content: format!("Error accessing path '{}': {}", absolute_path.display(), e),
            is_error: true,
        }),
    }
}

pub fn delete_file_sync(
    call: ToolCall,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ToolResult>> + Send>> {
    Box::pin(async move {
        // For sync wrapper, we need to create a temporary file security manager
        // This should only be used during tool recreation, the actual execution
        // should be handled by the Agent with proper security managers
        let mut file_security_manager =
            crate::security::FileSecurityManager::new(crate::security::FileSecurity::default());
        delete_file(&call, &mut file_security_manager, false).await
    })
}

pub fn create_delete_file_tool(
    file_security_manager: std::sync::Arc<tokio::sync::RwLock<FileSecurityManager>>,
    yolo_mode: bool,
) -> Tool {
    Tool {
        name: "delete_file".to_string(),
        description: if yolo_mode {
            "Delete a file or directory (YOLO MODE - no security checks)".to_string()
        } else {
            "Delete a file or directory".to_string()
        },
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file or directory to delete"
                }
            },
            "required": ["path"]
        }),
        handler: Box::new(move |call: ToolCall| {
            let file_security_manager = file_security_manager.clone();
            let yolo_mode = yolo_mode;
            Box::pin(async move {
                // Create a wrapper function that handles the mutable reference
                async fn delete_file_wrapper(
                    call: ToolCall,
                    file_security_manager: std::sync::Arc<tokio::sync::RwLock<FileSecurityManager>>,
                    yolo_mode: bool,
                ) -> Result<ToolResult> {
                    let mut manager = file_security_manager.write().await;
                    delete_file(&call, &mut *manager, yolo_mode).await
                }

                delete_file_wrapper(call, file_security_manager, yolo_mode).await
            })
        }),
        metadata: None, // TODO: Add proper metadata
    }
}
