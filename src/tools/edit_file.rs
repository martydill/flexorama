use crate::security::FileSecurityManager;
use crate::tools::types::{Tool, ToolCall, ToolResult};
use anyhow::Result;
use log::{debug, info};
use path_absolutize::*;
use serde_json::json;
use shellexpand;
use std::path::Path;
use tokio::fs;

// Detect the line ending type used in the content
fn detect_line_ending(content: &str) -> &str {
    if content.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}

// Convert text to use the specified line ending type
fn normalize_line_endings(text: &str, line_ending: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', line_ending)
}

pub async fn edit_file(
    call: &ToolCall,
    file_security_manager: &mut FileSecurityManager,
    yolo_mode: bool,
) -> Result<ToolResult> {
    let path = call
        .arguments
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;

    let old_text = call
        .arguments
        .get("old_text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'old_text' argument"))?;

    let new_text = call
        .arguments
        .get("new_text")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'new_text' argument"))?;

    debug!(
        "TOOL CALL: edit_file('{}', {} -> {} bytes)",
        path,
        old_text.len(),
        new_text.len()
    );

    let tool_use_id = call.id.clone();

    let expanded_path = shellexpand::tilde(path);
    let absolute_path = Path::new(&*expanded_path).absolutize()?;

    // Check file security permissions
    if yolo_mode {
        debug!(
            "YOLO MODE: Bypassing file security for 'edit_file' on '{}'",
            absolute_path.display()
        );
    } else {
        match file_security_manager
            .check_file_permission("edit_file", &absolute_path.to_string_lossy())
        {
            crate::security::FilePermissionResult::Allowed => {
                debug!(
                    "File operation 'edit_file' on '{}' is allowed by security policy",
                    absolute_path.display()
                );
            }
            crate::security::FilePermissionResult::Denied => {
                return Ok(ToolResult {
                    tool_use_id,
                    content: format!("🔒 Security: File edit operation on '{}' is not allowed by security policy.", absolute_path.display()),
                    is_error: true,
                });
            }
            crate::security::FilePermissionResult::RequiresPermission => {
                // Ask user for permission
                match file_security_manager
                    .ask_file_permission("edit_file", &absolute_path.to_string_lossy())
                    .await
                {
                    Ok(Some(_)) => {
                        // User granted permission
                        info!(
                            "User granted permission for file edit operation: {}",
                            absolute_path.display()
                        );
                    }
                    Ok(None) => {
                        return Ok(ToolResult {
                            tool_use_id,
                            content: format!(
                                "🔒 Security: Permission denied for file edit operation on '{}'",
                                absolute_path.display()
                            ),
                            is_error: true,
                        });
                    }
                    Err(e) => {
                        return Ok(ToolResult {
                            tool_use_id,
                            content: format!("🔒 Security: Error checking permission for file edit operation on '{}': {}", absolute_path.display(), e),
                            is_error: true,
                        });
                    }
                }
            }
        }
    }

    // Read existing file
    match fs::read_to_string(&absolute_path).await {
        Ok(mut content) => {
            // Detect the line ending type used in the file
            let file_line_ending = detect_line_ending(&content);

            // Normalize old_text to use the file's line endings for matching
            let normalized_old_text = normalize_line_endings(old_text, file_line_ending);

            if !content.contains(&normalized_old_text) {
                return Ok(ToolResult {
                    tool_use_id,
                    content: format!(
                        "Text not found in file '{}': {}",
                        absolute_path.display(),
                        normalized_old_text
                    ),
                    is_error: true,
                });
            }

            // Normalize new_text to use the file's line endings
            let normalized_new_text = normalize_line_endings(new_text, file_line_ending);

            content = content.replace(&normalized_old_text, &normalized_new_text);

            match fs::write(&absolute_path, content).await {
                Ok(_) => Ok(ToolResult {
                    tool_use_id,
                    content: format!("Successfully edited file: {}", absolute_path.display()),
                    is_error: false,
                }),
                Err(e) => Ok(ToolResult {
                    tool_use_id,
                    content: format!("Error writing to file '{}': {}", absolute_path.display(), e),
                    is_error: true,
                }),
            }
        }
        Err(e) => Ok(ToolResult {
            tool_use_id,
            content: format!("Error reading file '{}': {}", absolute_path.display(), e),
            is_error: true,
        }),
    }
}

pub fn edit_file_sync(
    call: ToolCall,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ToolResult>> + Send>> {
    Box::pin(async move {
        // For sync wrapper, we need to create a temporary file security manager
        // This should only be used during tool recreation, the actual execution
        // should be handled by the Agent with proper security managers
        let mut file_security_manager =
            crate::security::FileSecurityManager::new(crate::security::FileSecurity::default());
        edit_file(&call, &mut file_security_manager, false).await
    })
}

pub fn create_edit_file_tool(
    file_security_manager: std::sync::Arc<tokio::sync::RwLock<FileSecurityManager>>,
    yolo_mode: bool,
) -> Tool {
    Tool {
        name: "edit_file".to_string(),
        description: if yolo_mode {
            "Edit a file (YOLO MODE - no security checks)".to_string()
        } else {
            "Replace specific text in a file with new text".to_string()
        },
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to edit"
                },
                "old_text": {
                    "type": "string",
                    "description": "Text to replace"
                },
                "new_text": {
                    "type": "string",
                    "description": "New text to replace with"
                }
            },
            "required": ["path", "old_text", "new_text"]
        }),
        handler: Box::new(move |call: ToolCall| {
            let file_security_manager = file_security_manager.clone();
            let yolo_mode = yolo_mode;
            Box::pin(async move {
                // Create a wrapper function that handles the mutable reference
                async fn edit_file_wrapper(
                    call: ToolCall,
                    file_security_manager: std::sync::Arc<tokio::sync::RwLock<FileSecurityManager>>,
                    yolo_mode: bool,
                ) -> Result<ToolResult> {
                    let mut manager = file_security_manager.write().await;
                    edit_file(&call, &mut *manager, yolo_mode).await
                }

                edit_file_wrapper(call, file_security_manager, yolo_mode).await
            })
        }),
        metadata: None, // TODO: Add proper metadata
    }
}


