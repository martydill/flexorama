# Agent Client Protocol (ACP) Usage Guide

## Overview

Flexorama implements the Agent Client Protocol (ACP), a JSON-RPC 2.0 based protocol that enables code editors to communicate with AI coding agents over stdio. This allows editors like Zed, Neovim, and others to integrate Flexorama as a coding assistant.

## Quick Start

### Starting ACP Server

```bash
# Start in ACP mode
flexorama --acp

# With debug logging
flexorama --acp --acp-debug

# With specific model
flexorama --acp --model claude-opus-4

# With yolo mode (no permissions)
flexorama --acp --yolo
```

### ACP Server Modes

- **Normal Mode**: Respects file and bash security settings
- **Yolo Mode** (`--yolo`): Bypasses all permission checks
- **Plan Mode** (`--plan-mode`): Read-only operations only
- **Debug Mode** (`--acp-debug`): Logs all JSON-RPC messages to stderr

## Supported Methods

### Lifecycle Methods

#### `initialize`
Initialize the ACP server and negotiate capabilities.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "initialize",
  "params": {
    "workspaceRoot": "/path/to/workspace",
    "capabilities": {}
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "capabilities": {
      "fileSystem": {
        "read": true,
        "write": true,
        "list": true,
        "search": true,
        "delete": true,
        "create_directory": true
      },
      "tools": {
        "available": [],
        "dynamic": true
      },
      "streaming": true,
      "multi_turn": true,
      "code_editing": true,
      "shell_execution": true,
      "progress": true
    },
    "serverInfo": {
      "name": "Flexorama",
      "version": "0.1.0"
    }
  }
}
```

#### `initialized`
Notification that client has received initialization response.

**Notification:**
```json
{
  "jsonrpc": "2.0",
  "method": "initialized",
  "params": null
}
```

#### `shutdown`
Request server shutdown.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "shutdown"
}
```

#### `exit`
Notification to exit the server process.

**Notification:**
```json
{
  "jsonrpc": "2.0",
  "method": "exit"
}
```

### Agent Methods

#### `agent/prompt`
Send a prompt to the AI agent and receive a response.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "method": "agent/prompt",
  "params": {
    "prompt": "Write a function to calculate fibonacci numbers"
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 3,
  "result": {
    "response": "Here's a fibonacci function...",
    "usage": {
      "inputTokens": 50,
      "outputTokens": 200,
      "totalTokens": 250
    }
  }
}
```

#### `agent/cancel`
Cancel the currently running operation.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 4,
  "method": "agent/cancel"
}
```

### File System Methods

#### `fs/readFile`
Read the contents of a file.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 5,
  "method": "fs/readFile",
  "params": {
    "path": "src/main.rs"
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 5,
  "result": {
    "content": "fn main() {\n    println!(\"Hello, world!\");\n}\n"
  }
}
```

#### `fs/writeFile`
Write content to a file.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 6,
  "method": "fs/writeFile",
  "params": {
    "path": "src/lib.rs",
    "content": "pub fn hello() {\n    println!(\"Hello from lib\");\n}\n"
  }
}
```

#### `fs/listDirectory`
List contents of a directory.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 7,
  "method": "fs/listDirectory",
  "params": {
    "path": "src"
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 7,
  "result": {
    "entries": [
      {
        "name": "main.rs",
        "isDirectory": false,
        "path": "/workspace/src/main.rs"
      },
      {
        "name": "lib.rs",
        "isDirectory": false,
        "path": "/workspace/src/lib.rs"
      }
    ]
  }
}
```

#### `fs/glob`
Search for files matching a glob pattern.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 8,
  "method": "fs/glob",
  "params": {
    "pattern": "**/*.rs"
  }
}
```

#### `fs/delete`
Delete a file or directory.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 9,
  "method": "fs/delete",
  "params": {
    "path": "temp.txt"
  }
}
```

#### `fs/createDirectory`
Create a directory (and parent directories if needed).

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 10,
  "method": "fs/createDirectory",
  "params": {
    "path": "src/utils"
  }
}
```

### Context Methods

#### `context/addFile`
Add a file to the conversation context.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 11,
  "method": "context/addFile",
  "params": {
    "path": "src/main.rs"
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 11,
  "result": {
    "success": true,
    "path": "/workspace/src/main.rs"
  }
}
```

#### `context/clear`
Clear the conversation history (keeping AGENTS.md if present).

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 12,
  "method": "context/clear"
}
```

### Edit Methods

#### `edit/applyEdit`
Apply a string replacement edit to a file.

**Request:**
```json
{
  "jsonrpc": "2.0",
  "id": 13,
  "method": "edit/applyEdit",
  "params": {
    "path": "src/main.rs",
    "oldString": "println!(\"Hello, world!\");",
    "newString": "println!(\"Hello, Flexorama!\");"
  }
}
```

**Response:**
```json
{
  "jsonrpc": "2.0",
  "id": 13,
  "result": {
    "success": true,
    "message": "File edited successfully"
  }
}
```

## Capabilities

Flexorama advertises the following capabilities:

- **File System Operations**: Full read/write/delete/list/search support
- **Tools**: Dynamic tool registration and execution
- **Streaming**: Streaming response support (planned)
- **Multi-turn**: Multi-turn conversation support
- **Code Editing**: Specialized code editing operations
- **Shell Execution**: Bash command execution (with permissions)
- **Progress**: Progress notifications (planned)

## Security

### Permission System

By default, Flexorama uses a permission system for file and bash operations:

- **File Operations**: Checked against allowlist/denylist in config
- **Bash Commands**: Checked against allowlist/denylist in config
- **Workspace Boundaries**: All file paths validated to be within workspace

### Yolo Mode

Use `--yolo` flag to bypass all permission checks (use with caution):

```bash
flexorama --acp --yolo
```

### Plan Mode

Use `--plan-mode` for read-only operations:

```bash
flexorama --acp --plan-mode
```

## Error Handling

ACP uses standard JSON-RPC 2.0 error codes:

| Code | Meaning |
|------|---------|
| -32700 | Parse error |
| -32600 | Invalid request |
| -32601 | Method not found |
| -32602 | Invalid params |
| -32603 | Internal error |
| -32001 | Permission denied |
| -32002 | File not found |
| -32003 | Workspace not initialized |
| -32004 | Capability not supported |
| -32005 | Cancelled |

**Error Response Example:**
```json
{
  "jsonrpc": "2.0",
  "id": 14,
  "error": {
    "code": -32001,
    "message": "Permission denied: Cannot write to /etc/passwd"
  }
}
```

## Debugging

Enable debug mode to see all JSON-RPC messages:

```bash
flexorama --acp --acp-debug
```

Debug messages are written to stderr in this format:

```
[ACP RX] {"jsonrpc":"2.0","id":1,"method":"initialize",...}
[ACP TX] {"jsonrpc":"2.0","id":1,"result":{...}}
```

## Example Integration

### Minimal Python Client

```python
import json
import subprocess
import sys

# Start Flexorama in ACP mode
proc = subprocess.Popen(
    ['flexorama', '--acp'],
    stdin=subprocess.PIPE,
    stdout=subprocess.PIPE,
    stderr=subprocess.PIPE,
    text=True,
    bufsize=1
)

def send_request(method, params, id=1):
    request = {
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params
    }
    proc.stdin.write(json.dumps(request) + '\n')
    proc.stdin.flush()

    response = json.loads(proc.stdout.readline())
    return response

# Initialize
init_response = send_request("initialize", {
    "workspaceRoot": "/path/to/workspace",
    "capabilities": {}
}, id=1)

print("Server capabilities:", init_response['result']['capabilities'])

# Send a prompt
prompt_response = send_request("agent/prompt", {
    "prompt": "Write a hello world function in Rust"
}, id=2)

print("Response:", prompt_response['result']['response'])

# Shutdown
send_request("shutdown", None, id=3)
proc.communicate(input="exit\n")
```

## Editor Integration

### Zed

(Configuration example would go here once tested)

### Neovim

(Configuration example would go here once tested)

## Limitations

Current limitations of the ACP implementation:

1. **No Real-time Progress**: Progress notifications are not yet fully implemented
2. **No Streaming**: Response streaming is marked as supported but not fully implemented
3. **Single Request at a Time**: Concurrent requests not currently supported
4. **Limited Edit Operations**: Only string replacement edits, no range-based edits

## Future Enhancements

Planned improvements:

- Full streaming support for long responses
- Progress notifications during tool execution
- Range-based edit operations
- Multiple concurrent request handling
- WebSocket transport in addition to stdio
- Enhanced diagnostics and logging

## See Also

- [Agent Client Protocol Specification](https://agentclientprotocol.com/)
- [Flexorama Documentation](../AGENTS.md)
- [ACP Implementation Plan](./ACP_IMPLEMENTATION_PLAN.md)
