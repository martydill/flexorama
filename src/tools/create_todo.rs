use crate::tools::types::{ToolCall, ToolResult};
use anyhow::Result;
use serde_json::json;

/// Create a new todo item
pub async fn create_todo(call: &ToolCall, todos: &mut Vec<TodoItem>) -> Result<ToolResult> {
    let description = call
        .arguments
        .get("description")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'description' argument"))?;

    let tool_use_id = call.id.clone();

    if let Some(existing) = todos.iter().find(|todo| todo.description == description) {
        let response = json!({
            "message": "Todo already exists",
            "todo": existing
        });
        return Ok(ToolResult {
            tool_use_id,
            content: response.to_string(),
            is_error: false,
        });
    }

    // Create new todo item
    let todo = TodoItem {
        id: generate_id(),
        description: description.to_string(),
        completed: false,
    };

    todos.push(todo.clone());

    let response = json!({
        "message": "Todo created successfully",
        "todo": todo
    });

    Ok(ToolResult {
        tool_use_id,
        content: response.to_string(),
        is_error: false,
    })
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub description: String,
    pub completed: bool,
}

fn generate_id() -> String {
    let short = uuid::Uuid::new_v4()
        .to_string()
        .chars()
        .take(8)
        .collect::<String>();
    format!("todo-{}", short)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::types::ToolCall;
    use serde_json::json;

    #[tokio::test]
    async fn create_todo_adds_item_and_returns_response() {
        let call = ToolCall {
            id: "tool-1".to_string(),
            name: "create_todo".to_string(),
            arguments: json!({ "description": "Write tests" }),
        };
        let mut todos = Vec::new();

        let result = create_todo(&call, &mut todos).await.unwrap();

        assert_eq!(result.tool_use_id, "tool-1");
        assert!(!result.is_error);
        assert_eq!(todos.len(), 1);
        assert_eq!(todos[0].description, "Write tests");
        assert!(!todos[0].completed);
        assert!(todos[0].id.starts_with("todo-"));
        assert_eq!(todos[0].id.len(), "todo-".len() + 8);
        assert!(todos[0].id.chars().all(|ch| ch.is_ascii()));

        let response: serde_json::Value = serde_json::from_str(&result.content).unwrap();
        assert_eq!(response["message"], "Todo created successfully");
        assert_eq!(response["todo"]["description"], "Write tests");
        assert_eq!(response["todo"]["completed"], false);
    }

    #[tokio::test]
    async fn create_todo_requires_description() {
        let call = ToolCall {
            id: "tool-2".to_string(),
            name: "create_todo".to_string(),
            arguments: json!({}),
        };
        let mut todos = Vec::new();

        let err = create_todo(&call, &mut todos).await.unwrap_err();
        assert!(err.to_string().contains("Missing 'description' argument"));
        assert!(todos.is_empty());
    }

    #[tokio::test]
    async fn create_todo_dedupes_by_description() {
        let call = ToolCall {
            id: "tool-3".to_string(),
            name: "create_todo".to_string(),
            arguments: json!({ "description": "Ship it" }),
        };
        let mut todos = Vec::new();

        let first = create_todo(&call, &mut todos).await.unwrap();
        let second = create_todo(&call, &mut todos).await.unwrap();

        assert_eq!(todos.len(), 1);
        let first_response: serde_json::Value = serde_json::from_str(&first.content).unwrap();
        let second_response: serde_json::Value = serde_json::from_str(&second.content).unwrap();
        assert_eq!(first_response["todo"]["id"], second_response["todo"]["id"]);
        assert_eq!(second_response["message"], "Todo already exists");
    }
}
