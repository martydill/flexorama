use crate::tools::types::{Tool, ToolCall, ToolResult};
use anyhow::Result;
use glob::glob;
use log::debug;
use path_absolutize::*;
use serde_json::json;
use shellexpand;
use std::path::Path;

pub async fn glob_files(call: &ToolCall) -> Result<ToolResult> {
    let pattern = call
        .arguments
        .get("pattern")
        .and_then(|v| v.as_str())
        .unwrap_or("*");

    let base_path = call
        .arguments
        .get("base_path")
        .and_then(|v| v.as_str())
        .unwrap_or(".");

    debug!(
        "TOOL CALL: glob_files(pattern='{}', base_path='{}')",
        pattern, base_path
    );

    let tool_use_id = call.id.clone();

    // Expand and resolve base path
    let expanded_base_path = shellexpand::tilde(base_path);
    let absolute_base_path = Path::new(&*expanded_base_path).absolutize()?;

    // Combine base path with pattern
    let full_pattern = if pattern.contains('/') {
        // If pattern already contains path components, use it as-is
        shellexpand::tilde(pattern).to_string()
    } else {
        // Otherwise, join with base path
        absolute_base_path
            .join(pattern)
            .to_string_lossy()
            .to_string()
    };

    debug!("Using glob pattern: {}", full_pattern);

    match glob(&full_pattern) {
        Ok(entries) => {
            let mut result = String::new();
            result.push_str(&format!("Files matching pattern '{}':\n", pattern));

            let mut items = Vec::new();
            let mut count = 0;

            for entry in entries {
                match entry {
                    Ok(path) => {
                        if let Some(path_str) = path.to_str() {
                            // Check if it's a directory or file
                            if path.is_dir() {
                                items.push(format!("📁 {}/", path_str));
                            } else {
                                // Try to get file size
                                let size = if let Ok(metadata) = tokio::fs::metadata(&path).await {
                                    metadata.len()
                                } else {
                                    0
                                };
                                items.push(format!("📄 {} ({} bytes)", path_str, size));
                            }
                            count += 1;
                        }
                    }
                    Err(e) => {
                        debug!("Error processing glob entry: {}", e);
                    }
                }
            }

            if items.is_empty() {
                result.push_str("No files found matching the pattern.");
            } else {
                items.sort();
                result.push_str(&items.join("\n"));
                result.push_str(&format!("\n\nTotal: {} items found", count));
            }

            Ok(ToolResult {
                tool_use_id,
                content: result,
                is_error: false,
            })
        }
        Err(e) => Ok(ToolResult {
            tool_use_id,
            content: format!("Invalid glob pattern '{}': {}", pattern, e),
            is_error: true,
        }),
    }
}

pub fn glob_files_sync(
    call: ToolCall,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ToolResult>> + Send>> {
    Box::pin(async move { glob_files(&call).await })
}

pub fn create_glob_tool() -> Tool {
    Tool {
        name: "glob".to_string(),
        description: "Find files and directories using glob patterns (read-only)".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match files (e.g., '*.rs', '**/*.txt', 'src/**/*.json')"
                },
                "base_path": {
                    "type": "string",
                    "description": "Base directory to search from (default: current directory)"
                }
            }
        }),
        handler: Box::new(glob_files_sync),
        metadata: None,
    }
}
