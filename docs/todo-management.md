# Todo Management System

## Overview

The Todo Management System is a built-in feature that allows the LLM to create, track, and complete tasks internally during conversations. This helps the AI maintain context about work that needs to be done and track progress.

## Available Tools

### 1. create_todo

Creates a new todo item in the internal todo list.

**Parameters:**
- `description` (string, required): Description of the todo item

**Returns:**
- JSON object with the created todo item including:
  - `id`: Unique identifier for the todo (e.g., "todo-1767888578")
  - `description`: The task description
  - `completed`: Boolean status (false for new todos)

**Example:**
```json
{
  "message": "Todo created successfully",
  "todo": {
    "id": "todo-1767888578",
    "description": "Review pull requests",
    "completed": false
  }
}
```

### 2. complete_todo

Marks a todo item as completed.

**Parameters:**
- `id` (string, required): ID of the todo item to mark as completed

**Returns:**
- JSON object with the updated todo item

**Example:**
```json
{
  "message": "Todo marked as completed",
  "todo": {
    "id": "todo-1767888578",
    "description": "Review pull requests",
    "completed": true
  }
}
```

### 3. list_todos

Lists all todo items in the internal todo list.

**Parameters:**
- None

**Returns:**
- JSON object with all todo items and their statuses

**Example:**
```json
{
  "message": "Found 3 todo item(s)",
  "todos": [
    {
      "id": "todo-1767888578",
      "description": "Review pull requests",
      "completed": false
    },
    {
      "id": "todo-1767888579",
      "description": "Write documentation",
      "completed": true
    },
    {
      "id": "todo-1767888580",
      "description": "Test the application",
      "completed": false
    }
  ]
}
```

## Usage Examples

### Creating Multiple Todos

```
User: Create todos for: 1) Review pull requests, 2) Write documentation, 3) Test the application

AI: I'll create these todo items for you.

[Creates three todos using create_todo tool]

Created 3 todo items:
- todo-1767888578: Review pull requests
- todo-1767888579: Write documentation
- todo-1767888580: Test the application
```

### Listing and Completing Todos

```
User: Show me all todos and mark the second one as complete

AI: Here are all your todos:

[Uses list_todos tool]

1. Review pull requests (Incomplete)
2. Write documentation (Incomplete)
3. Test the application (Incomplete)

Now I'll mark the second todo as complete:

[Uses complete_todo tool with id "todo-1767888579"]

✓ Write documentation marked as completed
```

### Working Through a Task List

```
User: Create a todo list for refactoring the authentication module, then work through each item

AI: I'll create a comprehensive todo list for this refactoring task.

[Creates todos for:
- Analyze current authentication code
- Design new authentication flow
- Implement user authentication
- Implement session management
- Add unit tests
- Update documentation]

Now I'll start working through these items systematically...
```

## Implementation Details

### Storage
- **In-Memory**: Todos are stored in memory during the agent session
- **Session-Based**: Each new agent session starts with an empty todo list
- **Thread-Safe**: Uses `Arc<AsyncMutex<Vec<TodoItem>>>` for concurrent access

### ID Generation
Todo IDs are generated using Unix timestamps:
```rust
format!("todo-{}", timestamp)
```

This ensures unique IDs within a session.

### Data Structure

```rust
pub struct TodoItem {
    pub id: String,
    pub description: String,
    pub completed: bool,
}
```

## Best Practices

1. **Be Specific**: Use clear, actionable descriptions for todo items
   - Good: "Implement user login endpoint with JWT authentication"
   - Bad: "Do auth stuff"

2. **Break Down Tasks**: Split large tasks into smaller, manageable todos
   - Instead of: "Build the entire application"
   - Use: "Design database schema", "Implement API endpoints", "Write tests"

3. **Use Consistently**: Regularly list todos to track progress and update status

4. **Reference IDs**: When completing todos, use the exact ID returned by create_todo

## Limitations

1. **No Persistence**: Todos are not saved between sessions
2. **No Editing**: Todo descriptions cannot be modified after creation
3. **No Deletion**: Completed todos remain in the list
4. **No Prioritization**: All todos have equal priority
5. **No Due Dates**: Todos do not have deadlines or time tracking

## Future Enhancements

Potential improvements for the todo system:
- Persistent storage (database or file-based)
- Edit and delete operations
- Priority levels
- Tags and categories
- Due dates and reminders
- Subtasks and dependencies
- Progress percentage
- Todo templates

## Integration with Agent

The todo system is integrated into the Agent struct:

```rust
pub struct Agent {
    // ... other fields
    todos: Arc<AsyncMutex<Vec<crate::tools::create_todo::TodoItem>>>,
}
```

Tool execution is handled in `execute_tool_internal`:
```rust
else if call.name == "create_todo" {
    let mut todos = self.todos.lock().await;
    crate::tools::create_todo::create_todo(call, &mut todos).await
}
```

This design allows the LLM to manage tasks autonomously while maintaining thread safety and proper error handling.
