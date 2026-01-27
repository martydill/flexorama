use crate::security::{FilePermissionResult, FileSecurityManager};
use crate::tools::types::ToolResult;
use log::{debug, info};
use std::path::Path;

/// Check file security permissions and handle user authorization if needed.
/// This function encapsulates the common security check pattern used across all file operations.
///
/// # Arguments
/// * `operation_name` - The name of the file operation (e.g., "write_file", "delete_file")
/// * `path` - The absolute path to the file being operated on
/// * `tool_use_id` - The tool use ID for error reporting
/// * `file_security_manager` - The security manager to check permissions
/// * `yolo_mode` - If true, bypasses all security checks
///
/// # Returns
/// * `Ok(None)` - Permission granted, operation should proceed
/// * `Ok(Some(ToolResult))` - Permission denied or error, operation should return this result
/// * `Err(e)` - Unexpected error occurred
pub async fn check_file_security(
    operation_name: &str,
    path: &Path,
    tool_use_id: String,
    file_security_manager: &mut FileSecurityManager,
    yolo_mode: bool,
) -> anyhow::Result<Option<ToolResult>> {
    // Check file security permissions
    if yolo_mode {
        debug!(
            "YOLO MODE: Bypassing file security for '{}' on '{}'",
            operation_name,
            path.display()
        );
        return Ok(None);
    }

    match file_security_manager.check_file_permission(operation_name, &path.to_string_lossy()) {
        FilePermissionResult::Allowed => {
            debug!(
                "File operation '{}' on '{}' is allowed by security policy",
                operation_name,
                path.display()
            );
            Ok(None)
        }
        FilePermissionResult::Denied => Ok(Some(ToolResult {
            tool_use_id,
            content: format!(
                "🔒 Security: File {} operation on '{}' is not allowed by security policy.",
                get_operation_display_name(operation_name),
                path.display()
            ),
            is_error: true,
        })),
        FilePermissionResult::RequiresPermission => {
            // Ask user for permission
            match file_security_manager
                .ask_file_permission(operation_name, &path.to_string_lossy())
                .await
            {
                Ok(Some(_)) => {
                    // User granted permission
                    info!(
                        "User granted permission for file {} operation: {}",
                        get_operation_display_name(operation_name),
                        path.display()
                    );
                    Ok(None)
                }
                Ok(None) => Ok(Some(ToolResult {
                    tool_use_id,
                    content: format!(
                        "🔒 Security: Permission denied for file {} operation on '{}'",
                        get_operation_display_name(operation_name),
                        path.display()
                    ),
                    is_error: true,
                })),
                Err(e) => Ok(Some(ToolResult {
                    tool_use_id,
                    content: format!(
                        "🔒 Security: Error checking permission for file {} operation on '{}': {}",
                        get_operation_display_name(operation_name),
                        path.display(),
                        e
                    ),
                    is_error: true,
                })),
            }
        }
    }
}

/// Convert operation name to a display-friendly format
fn get_operation_display_name(operation_name: &str) -> &str {
    match operation_name {
        "Write" => "write",
        "delete_file" => "delete",
        "Edit" => "edit",
        "create_directory" => "create",
        _ => operation_name,
    }
}
