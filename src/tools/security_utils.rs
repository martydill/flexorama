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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::{FileSecurity, FileSecurityManager, PermissionHandler};
    use std::sync::Arc;

    fn manager(ask_for_permission: bool, enabled: bool) -> FileSecurityManager {
        FileSecurityManager::new(FileSecurity {
            ask_for_permission,
            enabled,
            allow_all_session: false,
        })
    }

    fn handler(selection: Option<usize>) -> PermissionHandler {
        Arc::new(move |_prompt| {
            let selection = selection;
            Box::pin(async move { selection })
        })
    }

    fn path_in_temp(file_name: &str) -> std::path::PathBuf {
        let current_dir = std::env::current_dir().expect("current dir");
        let temp = tempfile::tempdir_in(current_dir).expect("temp dir");
        temp.keep().join(file_name)
    }

    #[tokio::test]
    async fn yolo_mode_bypasses_security_checks() {
        let mut manager = manager(true, true);

        let result = check_file_security(
            "Write",
            &path_in_temp("yolo.txt"),
            "call-1".to_string(),
            &mut manager,
            true,
        )
        .await
        .unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn disabled_security_allows_operation() {
        let mut manager = manager(true, false);

        let result = check_file_security(
            "Write",
            &path_in_temp("disabled.txt"),
            "call-2".to_string(),
            &mut manager,
            false,
        )
        .await
        .unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn enabled_security_without_prompting_allows_operation() {
        let mut manager = manager(false, true);

        let result = check_file_security(
            "Write",
            &path_in_temp("allowed.txt"),
            "call-3".to_string(),
            &mut manager,
            false,
        )
        .await
        .unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn granted_permission_allows_operation() {
        let mut manager = manager(true, true);
        manager.set_permission_handler(Some(handler(Some(0))));

        let result = check_file_security(
            "Write",
            &path_in_temp("granted.txt"),
            "call-4".to_string(),
            &mut manager,
            false,
        )
        .await
        .unwrap();

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn denied_permission_returns_error_result() {
        let mut manager = manager(true, true);
        manager.set_permission_handler(Some(handler(Some(2))));

        let path = path_in_temp("denied.txt");
        let result = check_file_security(
            "Write",
            &path,
            "call-5".to_string(),
            &mut manager,
            false,
        )
        .await
        .unwrap()
        .expect("denied operations return an error result");

        assert!(result.is_error);
        assert_eq!(result.tool_use_id, "call-5");
        assert!(result.content.contains("Permission denied"));
        assert!(result.content.contains(&path.to_string_lossy().to_string()));
    }

    #[tokio::test]
    async fn dismissed_permission_prompt_denies_operation() {
        let mut manager = manager(true, true);
        manager.set_permission_handler(Some(handler(None)));

        let result = check_file_security(
            "delete_file",
            &path_in_temp("cancelled.txt"),
            "call-6".to_string(),
            &mut manager,
            false,
        )
        .await
        .unwrap()
        .expect("cancelled prompts deny the operation");

        assert!(result.is_error);
        assert!(result.content.contains("Permission denied"));
    }

    #[test]
    fn operation_display_names_use_friendly_verbs() {
        assert_eq!(get_operation_display_name("Write"), "write");
        assert_eq!(get_operation_display_name("delete_file"), "delete");
        assert_eq!(get_operation_display_name("Edit"), "edit");
        assert_eq!(get_operation_display_name("create_directory"), "create");
        assert_eq!(get_operation_display_name("read_file"), "read_file");
    }
}
