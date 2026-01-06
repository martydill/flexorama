use crate::tools::registry::{DisplayFormat, ToolMetadata, ToolMetadataProvider};
use crate::tools::types::{Tool, ToolCall, ToolResult};
use anyhow::Result;
use log::debug;
use path_absolutize::*;
use serde_json::json;
use shellexpand;
use std::path::Path;
use tokio::fs;
use tokio::io::AsyncReadExt;
// Dummy struct for metadata implementation
pub struct ReadFileTool;

pub async fn read_file(call: &ToolCall) -> Result<ToolResult> {
    let path = call
        .arguments
        .get("path")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'path' argument"))?;

    debug!("TOOL CALL: read_file('{}')", path);

    let tool_use_id = call.id.clone();

    let expanded_path = shellexpand::tilde(path);
    let absolute_path = Path::new(&*expanded_path).absolutize()?;

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
        name: "read_file".to_string(),
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
