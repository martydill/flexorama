use crate::tools::types::{Tool, ToolCall, ToolResult};
use anyhow::Result;
use log::debug;
use path_absolutize::*;
use serde_json::json;
use shellexpand;
use std::path::Path;
use tokio::fs;

pub async fn list_directory(call: &ToolCall) -> Result<ToolResult> {
    let path = call
        .arguments
        .get("path")
        .and_then(|v| v.as_str())
        .unwrap_or(".");

    debug!("TOOL CALL: list_directory('{}')", path);

    let tool_use_id = call.id.clone();

    let expanded_path = shellexpand::tilde(path);
    let absolute_path = Path::new(&*expanded_path).absolutize()?;

    match fs::read_dir(&absolute_path).await {
        Ok(mut entries) => {
            let mut result = String::new();
            result.push_str(&format!("Contents of '{}':\n", absolute_path.display()));

            let mut items = Vec::new();
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");

                if path.is_dir() {
                    items.push(format!("📁 {}/", name));
                } else {
                    let size = if let Ok(metadata) = fs::metadata(&path).await {
                        metadata.len()
                    } else {
                        0
                    };
                    items.push(format!("📄 {} ({} bytes)", name, size));
                }
            }

            items.sort();
            result.push_str(&items.join("\n"));

            Ok(ToolResult {
                tool_use_id,
                content: result,
                is_error: false,
            })
        }
        Err(e) => Ok(ToolResult {
            tool_use_id,
            content: format!(
                "Error reading directory '{}': {}",
                absolute_path.display(),
                e
            ),
            is_error: true,
        }),
    }
}

pub fn list_directory_sync(
    call: ToolCall,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ToolResult>> + Send>> {
    Box::pin(async move { list_directory(&call).await })
}

pub fn create_list_directory_tool() -> Tool {
    Tool {
        name: "list_directory".to_string(),
        description: "List contents of a directory".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the directory to list (default: current directory)"
                }
            }
        }),
        handler: Box::new(list_directory_sync),
        metadata: None, // TODO: Add metadata for list_directory tool
    }
}
