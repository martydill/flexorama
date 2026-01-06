use crate::tools::types::{Tool, ToolCall, ToolResult};
use anyhow::Result;
use log::debug;
use path_absolutize::*;
use serde_json::json;
use shellexpand;
use std::path::Path;
use tokio::task;

pub async fn search_in_files(call: &ToolCall) -> Result<ToolResult> {
    let path = call
        .arguments
        .get("path")
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
        .unwrap_or(".");

    let query = call
        .arguments
        .get("query")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'query' argument"))?;

    debug!("TOOL CALL: search_in_files('{}', '{}')", path, query);

    let tool_use_id = call.id.clone();
    let expanded_path = shellexpand::tilde(path);
    let search_root = Path::new(&*expanded_path).absolutize()?.to_path_buf();
    let absolute_path_display = search_root.display().to_string();
    let query_owned = query.to_string();
    let max_results = 200usize;

    let search_result = task::spawn_blocking(move || -> Result<(Vec<String>, bool)> {
        fn walk_path(
            path: &Path,
            needle: &str,
            matches: &mut Vec<String>,
            max_results: usize,
            truncated: &mut bool,
        ) -> std::io::Result<()> {
            if *truncated {
                return Ok(());
            }

            let metadata = match std::fs::metadata(path) {
                Ok(m) => m,
                Err(_) => return Ok(()), // Skip paths we can't stat (e.g., special files on Windows)
            };
            if metadata.file_type().is_symlink() {
                return Ok(());
            }
            if metadata.is_dir() {
                if path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .map(|n| n.eq_ignore_ascii_case(".git"))
                    .unwrap_or(false)
                {
                    return Ok(());
                }
                if let Ok(iter) = std::fs::read_dir(path) {
                    for entry in iter {
                        let entry = entry?;
                        walk_path(&entry.path(), needle, matches, max_results, truncated)?;
                        if *truncated {
                            break;
                        }
                    }
                }
            } else if metadata.is_file() {
                match std::fs::read_to_string(path) {
                    Ok(contents) => {
                        for (idx, line) in contents.lines().enumerate() {
                            if line.contains(needle) {
                                matches.push(format!(
                                    "{}:{}: {}",
                                    path.display(),
                                    idx + 1,
                                    line.trim_end()
                                ));
                                if matches.len() >= max_results {
                                    *truncated = true;
                                    break;
                                }
                            }
                        }
                    }
                    Err(_) => {
                        // Skip unreadable or binary files
                    }
                }
            }
            Ok(())
        }

        let mut matches = Vec::new();
        let mut truncated = false;
        walk_path(
            &search_root,
            &query_owned,
            &mut matches,
            max_results,
            &mut truncated,
        )?;
        Ok((matches, truncated))
    })
    .await;

    match search_result {
        Ok(Ok((matches, truncated))) => {
            if matches.is_empty() {
                Ok(ToolResult {
                    tool_use_id,
                    content: format!("No matches for '{}' under {}", query, absolute_path_display),
                    is_error: false,
                })
            } else {
                let mut content = format!(
                    "Found {} matches for '{}' under {}:\n",
                    matches.len(),
                    query,
                    absolute_path_display
                );
                content.push_str(&matches.join("\n"));
                if truncated {
                    content.push_str(&format!("\n...truncated after {} matches", matches.len()));
                }

                Ok(ToolResult {
                    tool_use_id,
                    content,
                    is_error: false,
                })
            }
        }
        Ok(Err(e)) => Ok(ToolResult {
            tool_use_id,
            content: format!("Error searching '{}': {}", absolute_path_display, e),
            is_error: true,
        }),
        Err(e) => Ok(ToolResult {
            tool_use_id,
            content: format!("Search task failed: {}", e),
            is_error: true,
        }),
    }
}

pub fn search_in_files_sync(
    call: ToolCall,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<ToolResult>> + Send>> {
    Box::pin(async move { search_in_files(&call).await })
}

pub fn create_search_in_files_tool() -> Tool {
    Tool {
        name: "search_in_files".to_string(),
        description: "Search for a string in a file or directory (recursive)".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to a file or directory to search (default: current directory)"
                },
                "query": {
                    "type": "string",
                    "description": "String to search for"
                }
            },
            "required": ["query"]
        }),
        handler: Box::new(search_in_files_sync),
        metadata: None, // TODO: Add proper metadata
    }
}
