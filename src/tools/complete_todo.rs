use crate::tools::types::{ToolCall, ToolResult};
use anyhow::Result;
use serde_json::json;

/// Complete a todo item by marking it as done
pub async fn complete_todo(
    call: &ToolCall,
    todos: &mut Vec<super::create_todo::TodoItem>,
) -> Result<ToolResult> {
    let id = call
        .arguments
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'id' argument"))?;

    let tool_use_id = call.id.clone();

    // Find and mark the todo as completed
    let todo = todos
        .iter_mut()
        .find(|t| t.id == id)
        .ok_or_else(|| anyhow::anyhow!("Todo item with id '{}' not found", id))?;

    if todo.completed {
        let response = json!({
            "message": "Todo item is already completed",
            "todo": todo
        });

        return Ok(ToolResult {
            tool_use_id,
            content: response.to_string(),
            is_error: false,
        });
    }

    todo.completed = true;

    let response = json!({
        "message": "Todo marked as completed",
        "todo": todo
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

    fn sample_todo(id: &str, completed: bool) -> TodoItem {
        TodoItem {
            id: id.to_string(),
            description: "Test todo".to_string(),
            completed,
        }
    }

    #[tokio::test]
    async fn complete_todo_marks_item_complete() {
        let call = ToolCall {
            id: "tool-1".to_string(),
            name: "complete_todo".to_string(),
            arguments: json!({ "id": "todo-abc12345" }),
        };
        let mut todos = vec![sample_todo("todo-abc12345", false)];

        let result = complete_todo(&call, &mut todos).await.unwrap();

        assert_eq!(result.tool_use_id, "tool-1");
        assert!(!result.is_error);
        assert!(todos[0].completed);

        let response: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(response["message"], "Todo marked as completed");
        assert_eq!(response["todo"]["completed"], true);
    }

    #[tokio::test]
    async fn complete_todo_is_idempotent() {
        let call = ToolCall {
            id: "tool-2".to_string(),
            name: "complete_todo".to_string(),
            arguments: json!({ "id": "todo-abc12345" }),
        };
        let mut todos = vec![sample_todo("todo-abc12345", true)];

        let result = complete_todo(&call, &mut todos).await.unwrap();

        assert_eq!(result.tool_use_id, "tool-2");
        assert!(!result.is_error);
        assert!(todos[0].completed);

        let response: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(response["message"], "Todo item is already completed");
    }

    #[tokio::test]
    async fn complete_todo_requires_id() {
        let call = ToolCall {
            id: "tool-3".to_string(),
            name: "complete_todo".to_string(),
            arguments: json!({}),
        };
        let mut todos = vec![sample_todo("todo-abc12345", false)];

        let err = complete_todo(&call, &mut todos).await.unwrap_err();
        assert!(err.to_string().contains("Missing 'id' argument"));
    }

    #[tokio::test]
    async fn complete_todo_errors_when_missing() {
        let call = ToolCall {
            id: "tool-4".to_string(),
            name: "complete_todo".to_string(),
            arguments: json!({ "id": "todo-missing" }),
        };
        let mut todos = vec![sample_todo("todo-abc12345", false)];

        let err = complete_todo(&call, &mut todos).await.unwrap_err();
        assert!(err.to_string().contains("not found"));
        assert!(!todos[0].completed);
    }
}
