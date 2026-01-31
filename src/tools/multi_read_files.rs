use crate::tools::path::resolve_project_path;
use crate::tools::types::{Tool, ToolCall, ToolResult};
use anyhow::Result;
use log::debug;
use serde_json::json;
use tokio::fs;
use tokio::io::AsyncReadExt;

pub async fn multi_read_files(call: &ToolCall) -> Result<ToolResult> {
    let paths = extract_array_arg!(call, "paths");

    debug!("TOOL CALL: multi_read_files({:?})", paths);

    let tool_use_id = call.id.clone();

    // Extract the paths from the JSON array
    let paths_vec: Vec<String> = match paths.as_array() {
        Some(arr) => {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        }
        None => {
            return Ok(ToolResult {
                tool_use_id,
                content: "Invalid paths argument: expected an array of strings".to_string(),
                is_error: true,
            });
        }
    };

    if paths_vec.is_empty() {
        return Ok(ToolResult {
            tool_use_id,
            content: "No paths provided".to_string(),
            is_error: true,
        });
    }

    // Read all files and collect results
    let mut results = Vec::new();
    let mut errors = Vec::new();

    for path in &paths_vec {
        let absolute_path = match resolve_project_path(path) {
            Ok(path) => path,
            Err(e) => {
                errors.push(format!("Invalid path '{}': {}", path, e));
                continue;
            }
        };

        match fs::File::open(&absolute_path).await {
            Ok(mut file) => {
                let mut contents = Vec::new();
                match file.read_to_end(&mut contents).await {
                    Ok(_) => {
                        let content = String::from_utf8_lossy(&contents);
                        results.push(format!(
                            "=== File: {} ===\n\n{}",
                            absolute_path.display(),
                            content
                        ));
                    }
                    Err(e) => {
                        errors.push(format!(
                            "Error reading file '{}': {}",
                            absolute_path.display(),
                            e
                        ));
                    }
                }
            }
            Err(e) => {
                errors.push(format!(
                    "Error opening file '{}': {}",
                    absolute_path.display(),
                    e
                ));
            }
        }
    }

    // Build the response
    let mut output = String::new();

    if !results.is_empty() {
        output.push_str(&format!("Successfully read {} file(s):\n\n", results.len()));
        output.push_str(&results.join("\n\n"));
    }

    if !errors.is_empty() {
        if !output.is_empty() {
            output.push_str("\n\n");
        }
        output.push_str(&format!("Errors ({}):\n", errors.len()));
        output.push_str(&errors.join("\n"));
    }

    let is_error = results.is_empty() && !errors.is_empty();

    Ok(ToolResult {
        tool_use_id,
        content: output,
        is_error,
    })
}

pub fn multi_read_files_sync(
    call: ToolCall,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ToolResult>> + Send>> {
    Box::pin(async move { multi_read_files(&call).await })
}

pub fn create_multi_read_files_tool() -> Tool {
    Tool {
        name: "MultiRead".to_string(),
        description: "Read the contents of multiple files at once. This is more efficient than reading files one by one.".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "paths": {
                    "type": "array",
                    "items": {
                        "type": "string"
                    },
                    "description": "Array of file paths to read"
                }
            },
            "required": ["paths"]
        }),
        handler: Box::new(multi_read_files_sync),
        metadata: None,
    }
}
