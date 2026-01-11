use crate::tools::types::{ToolCall, ToolResult};
use anyhow::Result;
use serde_json::json;

/// List all todo items
pub async fn list_todos(
    call: &ToolCall,
    todos: &[super::create_todo::TodoItem],
) -> Result<ToolResult> {
    let tool_use_id = call.id.clone();

    if todos.is_empty() {
        let response = json!({
            "message": "No todo items found",
            "todos": []
        });

        return Ok(ToolResult {
            tool_use_id,
            content: response.to_string(),
            is_error: false,
        });
    }

    let response = json!({
        "message": format!("Found {} todo item(s)", todos.len()),
        "todos": todos
    });

    Ok(ToolResult {
        tool_use_id,
        content: response.to_string(),
        is_error: false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::create_todo::TodoItem;
    use crate::tools::types::ToolCall;
    use serde_json::json;

    fn sample_todo(id: &str, completed: bool, description: &str) -> TodoItem {
        TodoItem {
            id: id.to_string(),
            description: description.to_string(),
            completed,
        }
    }

    #[tokio::test]
    async fn list_todos_returns_empty_message() {
        let call = ToolCall {
            id: "tool-1".to_string(),
            name: "list_todos".to_string(),
            arguments: json!({}),
        };
        let todos = Vec::new();

        let result = list_todos(&call, &todos).await.unwrap();

        assert_eq!(result.tool_use_id, "tool-1");
        assert!(!result.is_error);

        let response: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(response["message"], "No todo items found");
        assert_eq!(response["todos"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn list_todos_returns_items() {
        let call = ToolCall {
            id: "tool-2".to_string(),
            name: "list_todos".to_string(),
            arguments: json!({}),
        };
        let todos = vec![
            sample_todo("todo-11111111", false, "First task"),
            sample_todo("todo-22222222", true, "Second task"),
        ];

        let result = list_todos(&call, &todos).await.unwrap();

        assert_eq!(result.tool_use_id, "tool-2");
        assert!(!result.is_error);

        let response: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(response["message"], "Found 2 todo item(s)");
        assert_eq!(response["todos"].as_array().unwrap().len(), 2);
        assert_eq!(response["todos"][0]["description"], "First task");
        assert_eq!(response["todos"][1]["completed"], true);
    }
}
