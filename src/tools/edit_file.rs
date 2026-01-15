use crate::security::FileSecurityManager;
use crate::tools::path::resolve_project_path;
use crate::tools::security_utils::check_file_security;
use crate::tools::types::{Tool, ToolCall, ToolResult};
use anyhow::Result;
use log::debug;
use serde_json::json;
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
    let path = extract_string_arg!(call, "path");
    let old_text = extract_string_arg!(call, "old_text");
    let new_text = extract_string_arg!(call, "new_text");

    debug!(
        "TOOL CALL: edit_file('{}', {} -> {} bytes)",
        path,
        old_text.len(),
        new_text.len()
    );

    let tool_use_id = call.id.clone();

    let absolute_path = match resolve_project_path(path) {
        Ok(path) => path,
        Err(error) => {
            return Ok(ToolResult {
                tool_use_id,
                content: format!("Invalid path for edit_file: {}", error),
                is_error: true,
            });
        }
    };

    // Check file security permissions
    if let Some(result) = check_file_security(
        "edit_file",
        &absolute_path,
        tool_use_id.clone(),
        file_security_manager,
        yolo_mode,
    )
    .await?
    {
        return Ok(result);
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};
    use tempfile::TempDir;

    fn temp_file_path(prefix: &str) -> (TempDir, std::path::PathBuf) {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time went backwards")
            .as_nanos();
        let temp_dir =
            tempfile::tempdir_in(std::env::current_dir().expect("current dir")).expect("temp dir");
        let file_path =
            temp_dir
                .path()
                .join(format!("{}_{}_{}.txt", prefix, std::process::id(), nanos));
        (temp_dir, file_path)
    }

    #[test]
    fn detect_line_ending_prefers_crlf() {
        assert_eq!(detect_line_ending("one\r\ntwo\r\n"), "\r\n");
        assert_eq!(detect_line_ending("one\ntwo\n"), "\n");
    }

    #[test]
    fn normalize_line_endings_converts_mixed_input() {
        let input = "one\r\ntwo\nthree\r\n";
        let normalized = normalize_line_endings(input, "\r\n");
        assert_eq!(normalized, "one\r\ntwo\r\nthree\r\n");
    }

    #[tokio::test]
    async fn edit_file_replaces_with_windows_line_endings() {
        let (_temp_dir, path) = temp_file_path("edit_file_windows");
        let original = "first\r\nsecond\r\nthird\r\n";
        tokio::fs::write(&path, original)
            .await
            .expect("write temp file");

        let call = ToolCall {
            id: "test_edit".to_string(),
            name: "edit_file".to_string(),
            arguments: json!({
                "path": path.to_string_lossy(),
                "old_text": "second\nthird",
                "new_text": "alpha\nbeta"
            }),
        };

        let mut file_security_manager =
            FileSecurityManager::new(crate::security::FileSecurity::default());
        let result = edit_file(&call, &mut file_security_manager, true)
            .await
            .expect("edit file result");

        assert!(!result.is_error);
        assert!(result.content.contains("Successfully edited file"));

        let updated = tokio::fs::read_to_string(&path)
            .await
            .expect("read updated file");
        assert_eq!(updated, "first\r\nalpha\r\nbeta\r\n");
    }

    #[tokio::test]
    async fn edit_file_reports_text_not_found() {
        let (_temp_dir, path) = temp_file_path("edit_file_missing");
        tokio::fs::write(&path, "alpha\r\nbeta\r\n")
            .await
            .expect("write temp file");

        let call = ToolCall {
            id: "test_missing".to_string(),
            name: "edit_file".to_string(),
            arguments: json!({
                "path": path.to_string_lossy(),
                "old_text": "missing",
                "new_text": "gamma"
            }),
        };

        let mut file_security_manager =
            FileSecurityManager::new(crate::security::FileSecurity::default());
        let result = edit_file(&call, &mut file_security_manager, true)
            .await
            .expect("edit file result");

        assert!(result.is_error);
        assert!(result.content.contains("Text not found in file"));
    }
}
