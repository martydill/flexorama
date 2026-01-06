use crate::security::BashSecurityManager;
use crate::tools::types::{Tool, ToolCall, ToolResult};
use anyhow::Result;
use log::{debug, info};
use serde_json::json;
use std::process::Command;
use tokio::task;

// Convert Unix command separators to Windows PowerShell equivalents
// This function properly handles quoted strings, escape sequences, and complex command structures
fn convert_unix_separators_to_windows(command: &str) -> String {
    let mut result = String::new();
    let chars = command.chars().collect::<Vec<_>>();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        let c = chars[i];

        match c {
            // Handle quoted strings - preserve them exactly as they are
            '\'' => {
                result.push(c);
                i += 1;
                // Find the closing single quote
                while i < len {
                    result.push(chars[i]);
                    if chars[i] == '\'' && (i == 0 || chars[i - 1] != '\\') {
                        break;
                    }
                    i += 1;
                }
                i += 1;
            }
            '"' => {
                result.push(c);
                i += 1;
                // Find the closing double quote
                while i < len {
                    result.push(chars[i]);
                    if chars[i] == '"' && (i == 0 || chars[i - 1] != '\\') {
                        break;
                    }
                    i += 1;
                }
                i += 1;
            }
            // Handle && (logical AND) - replace with ; for PowerShell
            '&' if i + 1 < len && chars[i + 1] == '&' => {
                result.push(';');
                i += 2;
            }
            // Handle || (logical OR) - replace with ; for PowerShell (PowerShell handles this differently)
            '|' if i + 1 < len && chars[i + 1] == '|' => {
                result.push(';');
                i += 2;
            }
            // Handle ; (command separator) - keep as ; for PowerShell
            ';' => {
                result.push(';');
                i += 1;
            }
            // Handle && at end of string
            '&' if i == len - 1 => {
                result.push(';');
                i += 1;
            }
            // Handle all other characters
            _ => {
                result.push(c);
                i += 1;
            }
        }
    }

    result.trim().to_string()
}

pub async fn bash(
    call: &ToolCall,
    security_manager: &mut BashSecurityManager,
    yolo_mode: bool,
) -> Result<ToolResult> {
    let command = call
        .arguments
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'command' argument"))?
        .to_string();

    debug!("TOOL CALL: bash('{}')", command);

    let tool_use_id = call.id.clone();
    let mut permissions_updated = false;

    // Check security permissions
    if yolo_mode {
        debug!("YOLO MODE: Bypassing security for command '{}'", command);
    } else {
        match security_manager.check_command_permission(&command) {
            crate::security::PermissionResult::Allowed => {
                debug!("Command '{}' is allowed by security policy", command);
            }
            crate::security::PermissionResult::Denied => {
                return Ok(ToolResult {
                    tool_use_id,
                    content: format!("🔒 Security: Command '{}' is not allowed by security policy. Use /permissions to manage allowed commands.", command),
                    is_error: true,
                });
            }
            crate::security::PermissionResult::RequiresPermission => {
                // Ask user for permission
                match security_manager.ask_permission(&command).await {
                    Ok(Some(true)) => {
                        // User granted permission and wants to add to allowlist
                        info!(
                            "User granted permission for command: {} (added to allowlist)",
                            command
                        );
                        permissions_updated = true;
                    }
                    Ok(Some(false)) => {
                        // User granted permission for this time only
                        info!("User granted one-time permission for command: {}", command);
                    }
                    Ok(None) => {
                        return Ok(ToolResult {
                            tool_use_id,
                            content: format!(
                                "🔒 Security: Permission denied for command '{}'",
                                command
                            ),
                            is_error: true,
                        });
                    }
                    Err(e) => {
                        return Ok(ToolResult {
                            tool_use_id,
                            content: format!(
                                "🔒 Security: Error checking permission for command '{}': {}",
                                command, e
                            ),
                            is_error: true,
                        });
                    }
                }
            }
        }
    }

    // Convert command separators for Windows compatibility
    let processed_command = if cfg!(target_os = "windows") {
        // Convert Unix-style separators to PowerShell-compatible syntax
        convert_unix_separators_to_windows(&command)
    } else {
        command.clone()
    };

    // Execute the command using tokio::task to spawn blocking operation
    let command_clone = processed_command.clone();
    match task::spawn_blocking(move || {
        #[cfg(target_os = "windows")]
        {
            // Use PowerShell for better command handling on Windows
            // For multiple commands, we need to use -Command with proper PowerShell syntax
            let mut cmd = Command::new("powershell");

            // If the command contains semicolons, it's likely multiple commands
            if command_clone.contains(';') {
                cmd.args(["-Command", &command_clone]);
            } else {
                // For single commands, we can use -Command as well for consistency
                cmd.args(["-Command", &command_clone]);
            }

            cmd.output()
        }
        #[cfg(not(target_os = "windows"))]
        {
            Command::new("bash").args(["-c", &command_clone]).output()
        }
    })
    .await
    {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            let content = if !stderr.is_empty() {
                format!(
                    "Exit code: {}\nStdout:\n{}\nStderr:\n{}",
                    output.status.code().unwrap_or(-1),
                    stdout,
                    stderr
                )
            } else {
                format!(
                    "Exit code: {}\nOutput:\n{}",
                    output.status.code().unwrap_or(-1),
                    stdout
                )
            };

            let mut final_content = content;

            // Add a note if permissions were updated
            if permissions_updated {
                final_content.push_str("\n\n💾 Note: This command has been added to your allowlist and saved to config.");
            }

            Ok(ToolResult {
                tool_use_id,
                content: final_content,
                is_error: !output.status.success(),
            })
        }
        Ok(Err(e)) => Ok(ToolResult {
            tool_use_id,
            content: format!("Error executing command '{}': {}", command, e),
            is_error: true,
        }),
        Err(e) => Ok(ToolResult {
            tool_use_id,
            content: format!("Task join error: {}", e),
            is_error: true,
        }),
    }
}

pub fn bash_sync(
    _call: ToolCall,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ToolResult>> + Send>> {
    Box::pin(async move {
        // For sync wrapper, we need to create a temporary bash security manager
        // This should only be used during tool recreation, the actual execution
        // should be handled by the Agent with proper security managers
        let mut bash_security_manager =
            crate::security::BashSecurityManager::new(crate::security::BashSecurity::default());
        bash(&_call, &mut bash_security_manager, false).await
    })
}

pub fn create_bash_tool(
    security_manager: std::sync::Arc<tokio::sync::RwLock<BashSecurityManager>>,
    yolo_mode: bool,
) -> Tool {
    Tool {
        name: "bash".to_string(),
        description: if yolo_mode {
            "Execute shell commands and return the output (YOLO MODE - no security checks)"
                .to_string()
        } else {
            "Execute shell commands and return the output (with security)".to_string()
        },
        input_schema: json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to execute"
                }
            },
            "required": ["command"]
        }),
        handler: Box::new(move |call: ToolCall| {
            let security_manager = security_manager.clone();
            let yolo_mode = yolo_mode;
            Box::pin(async move {
                // Create a wrapper function that handles the mutable reference
                async fn bash_wrapper(
                    call: ToolCall,
                    security_manager: std::sync::Arc<tokio::sync::RwLock<BashSecurityManager>>,
                    yolo_mode: bool,
                ) -> Result<ToolResult> {
                    let mut manager = security_manager.write().await;
                    bash(&call, &mut *manager, yolo_mode).await
                }

                bash_wrapper(call, security_manager, yolo_mode).await
            })
        }),
        metadata: None, // TODO: Add metadata for bash tool
    }
}
