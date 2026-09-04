use crate::tools::types::{Tool, ToolResult};
use crate::tools::{
    bash::bash_sync, create_directory::create_directory_sync, delete_file::delete_file_sync,
    edit_file::edit_file_sync, glob::create_glob_tool, list_directory::create_list_directory_tool,
    multi_read_files::create_multi_read_files_tool, read_file::create_read_file_tool,
    search_in_files::create_search_in_files_tool, write_file::write_file_sync,
};
use serde_json::json;

pub fn get_builtin_tools() -> Vec<Tool> {
    vec![
        create_list_directory_tool(),
        create_read_file_tool(),
        create_multi_read_files_tool(),
        create_search_in_files_tool(),
        create_glob_tool(),
        // Todo management tools
        Tool {
            name: "create_todo".to_string(),
            description: "Create a new todo item. Adds a task to the internal todo list."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "description": {
                        "type": "string",
                        "description": "Description of the todo item"
                    }
                },
                "required": ["description"]
            }),
            handler: Box::new(|_call| {
                Box::pin(async move {
                    Ok(ToolResult {
                        tool_use_id: String::new(),
                        content: "create_todo is handled internally by the Agent".to_string(),
                        is_error: false,
                    })
                })
            }),
            metadata: None,
        },
        Tool {
            name: "complete_todo".to_string(),
            description:
                "Mark a todo item as completed. Use the todo ID to identify which item to complete."
                    .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {
                        "type": "string",
                        "description": "ID of the todo item to mark as completed"
                    }
                },
                "required": ["id"]
            }),
            handler: Box::new(|_call| {
                Box::pin(async move {
                    Ok(ToolResult {
                        tool_use_id: String::new(),
                        content: "complete_todo is handled internally by the Agent".to_string(),
                        is_error: false,
                    })
                })
            }),
            metadata: None,
        },
        Tool {
            name: "list_todos".to_string(),
            description: "List all todo items in the internal todo list, showing their status."
                .to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": []
            }),
            handler: Box::new(|_call| {
                Box::pin(async move {
                    Ok(ToolResult {
                        tool_use_id: String::new(),
                        content: "list_todos is handled internally by the Agent".to_string(),
                        is_error: false,
                    })
                })
            }),
            metadata: None,
        },
        Tool {
            name: "Write".to_string(),
            description: "Write content to a file (creates file if it doesn't exist)".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to write"
                    },
                    "content": {
                        "type": "string",
                        "description": "Content to write to the file"
                    }
                },
                "required": ["path", "content"]
            }),
            handler: Box::new(write_file_sync),
            metadata: None, // TODO: Add proper metadata
        },
        Tool {
            name: "Edit".to_string(),
            description: "Replace specific text in a file with new text".to_string(),
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
            handler: Box::new(edit_file_sync),
            metadata: None, // TODO: Add proper metadata
        },
        Tool {
            name: "delete_file".to_string(),
            description: "Delete a file or directory".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file or directory to delete"
                    }
                },
                "required": ["path"]
            }),
            handler: Box::new(delete_file_sync),
            metadata: None, // TODO: Add proper metadata
        },
        Tool {
            name: "create_directory".to_string(),
            description: "Create a directory (and parent directories if needed)".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the directory to create"
                    }
                },
                "required": ["path"]
            }),
            handler: Box::new(create_directory_sync),
            metadata: None, // TODO: Add proper metadata
        },
        // Note: The Bash tool is handled specially by the Agent with security
        // We include a placeholder here that will be properly handled by the Agent
        Tool {
            name: "Bash".to_string(),
            description: "Execute shell commands and return the output (with security)".to_string(),
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
            handler: Box::new(bash_sync),
            metadata: None, // TODO: Add proper metadata
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_tool_list_is_not_empty() {
        assert!(!get_builtin_tools().is_empty());
    }

    #[test]
    fn builtin_tool_names_are_unique() {
        let tools = get_builtin_tools();
        let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), tools.len(), "duplicate builtin tool names");
    }

    #[test]
    fn builtin_tools_expose_expected_surface() {
        let tools = get_builtin_tools();

        for tool in &tools {
            assert!(!tool.description.is_empty(), "{} has empty description", tool.name);
            assert_eq!(
                tool.input_schema.get("type").and_then(|v| v.as_str()),
                Some("object"),
                "{} schema is not an object",
                tool.name
            );
        }

        for expected in [
            "list_directory",
            "Read",
            "MultiRead",
            "search_in_files",
            "glob",
            "create_todo",
            "complete_todo",
            "list_todos",
            "Write",
            "Edit",
            "delete_file",
            "create_directory",
            "Bash",
        ] {
            assert!(
                tools.iter().any(|t| t.name == expected),
                "missing builtin tool '{}'",
                expected
            );
        }
    }

    #[test]
    fn builtin_tool_schemas_declare_required_fields() {
        let tools = get_builtin_tools();

        for tool in &tools {
            let properties = tool
                .input_schema
                .get("properties")
                .and_then(|v| v.as_object())
                .expect("properties object");

            let required = tool
                .input_schema
                .get("required")
                .and_then(|v| v.as_array())
                .cloned()
                .unwrap_or_default();

            for field in required {
                assert!(
                    properties.contains_key(field.as_str().unwrap()),
                    "{} requires '{}' but does not define it",
                    tool.name,
                    field
                );
            }
        }
    }
}
