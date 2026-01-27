use crate::tools::path::resolve_project_path;
use crate::tools::types::{Tool, ToolCall, ToolResult};
use anyhow::Result;
use log::debug;
use serde_json::json;
use tokio::fs;
use tokio::io::AsyncReadExt;

pub async fn read_file(call: &ToolCall) -> Result<ToolResult> {
    let path = extract_string_arg!(call, "path");

    debug!("TOOL CALL: read_file('{}')", path);

    let tool_use_id = call.id.clone();

    let absolute_path = match resolve_project_path(path) {
        Ok(path) => path,
        Err(e) => {
            return Ok(ToolResult {
                tool_use_id,
                content: format!("Invalid path '{}': {}", path, e),
                is_error: true,
            });
        }
    };

    match fs::File::open(&absolute_path).await {
        Ok(mut file) => {
            let mut contents = Vec::new();
            match file.read_to_end(&mut contents).await {
                Ok(_) => {
                    let content = String::from_utf8_lossy(&contents);
                    Ok(ToolResult {
                        tool_use_id,
                        content: format!("File: {}\n\n{}", absolute_path.display(), content),
                        is_error: false,
                    })
                }
                Err(e) => Ok(ToolResult {
                    tool_use_id,
                    content: format!("Error reading file '{}': {}", absolute_path.display(), e),
                    is_error: true,
                }),
            }
        }
        Err(e) => Ok(ToolResult {
            tool_use_id,
            content: format!("Error opening file '{}': {}", absolute_path.display(), e),
            is_error: true,
        }),
    }
}

pub fn read_file_sync(
    call: ToolCall,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ToolResult>> + Send>> {
    Box::pin(async move { read_file(&call).await })
}

pub fn create_read_file_tool() -> Tool {
    Tool {
        name: "Read".to_string(),
        description: "Read the contents of a file".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the file to read"
                }
            },
            "required": ["path"]
        }),
        handler: Box::new(read_file_sync),
        metadata: None,
    }
}
