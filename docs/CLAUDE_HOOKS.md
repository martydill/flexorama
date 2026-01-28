# Flexorama Hooks Support

Flexorama provides **Claude Code hooks compatibility**. Hook definitions live in:

- `~/.flexorama/hooks.json` (global hooks)
- `<project>/.flexorama/hooks.json` (project hooks)

## Configuration Format

Flexorama uses the official Claude Code `hooks.json` format with a top-level `"hooks"` wrapper:

```json
{
  "hooks": {
    "UserPromptSubmit": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "python .flexorama/hooks/user_prompt_submit.py",
            "args": ["--log-only"],
            "continueOnError": true,
            "timeoutMs": 5000
          }
        ]
      }
    ],
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "jq -r '.tool_input.command' >> ~/.flexorama/bash-log.txt"
          }
        ]
      }
    ]
  }
}
```

### Using Matchers

The `matcher` field allows you to filter hooks by tool name. This is useful for `PreToolUse` and `PostToolUse` events:

```json
{
  "hooks": {
    "PreToolUse": [
      {
        "matcher": "Bash",
        "hooks": [
          {
            "type": "command",
            "command": "python validate_bash.py"
          }
        ]
      },
      {
        "matcher": "Write",
        "hooks": [
          {
            "type": "command",
            "command": "python validate_write.py"
          }
        ]
      }
    ]
  }
}
```

Without a `matcher`, hooks run for all tools.

## Supported Hook Events

| Event | Description |
|-------|-------------|
| `UserPromptSubmit` | Fires when user submits a prompt |
| `PreToolUse` | Fires before tool execution |
| `PostToolUse` | Fires after tool completes |
| `Stop` | Fires when agent finishes responding |
| `SubagentStop` | Fires when subagent completes |
| `SessionStart` | Fires at session initialization |
| `PermissionRequest` | Fires on permission requests |

## Hook Entry Options

| Option | Type | Description |
|--------|------|-------------|
| `matcher` | string | Tool name to match (e.g., "Bash", "Read", "Write") - optional |
| `hooks` | array | Array of hook command objects (required) |

## Hook Command Options

| Option | Type | Description |
|--------|------|-------------|
| `type` | string | Must be "command" |
| `command` | string | Command to execute (required) |
| `args` | array | Command arguments (optional, defaults to []) |
| `env` | object | Environment variables (optional) |
| `workingDirectory` | string | Working directory (optional) |
| `timeoutMs` | number | Timeout in milliseconds (optional) |
| `continueOnError` | boolean | Continue if hook fails (optional, defaults to false) |

## Hook Execution Flow

### 1. Input (stdin)

Hooks receive a JSON payload on **stdin**:

**UserPromptSubmit:**
```json
{
  "session_id": "conv-123",
  "prompt": "User's message",
  "timestamp": "2026-01-26T12:34:56.789Z",
  "event": "UserPromptSubmit",
  "project_root": "/path/to/project",
  "cwd": "/path/to/cwd",
  "model": "claude-sonnet-4",
  "cleaned_message": "User's message (processed)",
  "context_files": ["file1.txt", "file2.txt"]
}
```

**PreToolUse:**
```json
{
  "session_id": "conv-123",
  "tool_name": "write_file",
  "tool_input": {"path": "file.txt", "content": "..."},
  "timestamp": "2026-01-26T12:34:56.789Z",
  "event": "PreToolUse",
  "project_root": "/path/to/project",
  "cwd": "/path/to/cwd",
  "model": "claude-sonnet-4",
  "tool_use_id": "toolu_123"
}
```

**PostToolUse:**
```json
{
  "session_id": "conv-123",
  "tool_name": "write_file",
  "tool_input": {"path": "file.txt", "content": "..."},
  "tool_output": "File written successfully",
  "timestamp": "2026-01-26T12:34:56.789Z",
  "event": "PostToolUse",
  "project_root": "/path/to/project",
  "cwd": "/path/to/cwd",
  "model": "claude-sonnet-4",
  "tool_use_id": "toolu_123",
  "is_error": false
}
```

**Stop:**
```json
{
  "session_id": "conv-123",
  "timestamp": "2026-01-26T12:34:56.789Z",
  "event": "Stop",
  "project_root": "/path/to/project",
  "cwd": "/path/to/cwd",
  "model": "claude-sonnet-4",
  "response": "Agent's final response text"
}
```

### 2. Output (stdout)

Hooks can return a JSON response to control execution:

**Block/Abort:**
```json
{
  "decision": "block",
  "reason": "Tool not allowed in this context"
}
```

**Continue but force retry:**
```json
{
  "continue": false,
  "stopReason": "Need more information"
}
```

**Approve/Allow:**
```json
{
  "decision": "approve"
}
```

**Note:** Empty stdout or non-JSON output is ignored.

## Environment Variables

Flexorama sets these environment variables for all hooks:

| Variable | Description |
|----------|-------------|
| `CLAUDE_CODE_HOOK_EVENT` | Event name (e.g., "UserPromptSubmit") |
| `CLAUDE_CODE_PROJECT_ROOT` | Project root directory |
| `CLAUDE_CODE_HOOK_SOURCE` | "home" or "project" |

## Shell Detection

Flexorama automatically detects the best available shell:

**Windows:**
1. PowerShell Core (`pwsh`) - preferred
2. Windows PowerShell (`powershell`)
3. Command Prompt (`cmd`)

**Unix/Linux/macOS:**
1. Bash (`bash`) - preferred
2. POSIX shell (`sh`)

## Security and Safety Features

### 1. Hook Execution Timeout

- **Individual hooks:** Configurable per-hook timeout via `timeoutMs`
- **Overall timeout:** All hooks for an event have a combined 30-second timeout

### 2. Error Handling

- Hooks fail by default (abort execution)
- Use `continueOnError: true` to allow execution to continue despite hook failures

## Example Use Cases

### 1. Log all user prompts

**.flexorama/hooks.json:**
```json
{
  "UserPromptSubmit": [
    {
      "hooks": [
        {
          "type": "command",
          "command": "python",
          "args": [".flexorama/hooks/log_prompt.py"]
        }
      ]
    }
  ]
}
```

**.flexorama/hooks/log_prompt.py:**
```python
#!/usr/bin/env python3
import json
import sys
from datetime import datetime

payload = json.load(sys.stdin)
prompt = payload.get("prompt", "")

with open(".flexorama/prompt_log.txt", "a") as f:
    f.write(f"{datetime.now()}: {prompt}\n")

# Allow execution to continue
print(json.dumps({"decision": "approve"}))
```

### 2. Block dangerous tool calls

**.flexorama/hooks.json:**
```json
{
  "PreToolUse": [
    {
      "hooks": [
        {
          "type": "command",
          "command": "python",
          "args": [".flexorama/hooks/check_tool.py"]
        }
      ]
    }
  ]
}
```

**.flexorama/hooks/check_tool.py:**
```python
#!/usr/bin/env python3
import json
import sys

payload = json.load(sys.stdin)
tool_name = payload.get("tool_name", "")
tool_input = payload.get("tool_input", {})

# Block dangerous bash commands
if tool_name == "bash":
    command = tool_input.get("command", "")
    dangerous = ["rm -rf", "dd if=", "mkfs", "> /dev/"]
    if any(d in command for d in dangerous):
        print(json.dumps({
            "decision": "block",
            "reason": f"Dangerous command blocked: {command}"
        }))
        sys.exit(0)

print(json.dumps({"decision": "approve"}))
```

### 3. Enforce task completion

**.flexorama/hooks.json:**
```json
{
  "Stop": [
    {
      "hooks": [
        {
          "type": "command",
          "command": "python",
          "args": [".flexorama/hooks/check_completion.py"]
        }
      ]
    }
  ]
}
```

**.flexorama/hooks/check_completion.py:**
```python
#!/usr/bin/env python3
import json
import sys

payload = json.load(sys.stdin)
response = payload.get("response", "")

# Check if response indicates incomplete work
if "TODO" in response or "not implemented" in response.lower():
    print(json.dumps({
        "continue": False,
        "stopReason": "Please complete all TODOs before finishing"
    }))
else:
    print(json.dumps({"decision": "approve"}))
```

## Troubleshooting

### Hooks not executing

1. Verify hooks.json syntax is valid JSON
2. Check that the command exists and is executable
3. Check Flexorama logs for errors

### Hook timing out

1. Increase `timeoutMs` for individual hooks
2. Optimize slow hooks
3. Check for blocking operations

### Hook failing

1. Check hook exit code (0 = success)
2. Review stderr output
3. Use `continueOnError: true` if failure is acceptable
4. Test hook independently: `echo '{}' | your-hook-script`

## Best Practices

1. **Keep hooks fast** - They run synchronously and block execution
2. **Use timeouts** - Prevent hooks from hanging indefinitely
3. **Return valid JSON** - Invalid JSON is ignored
4. **Test independently** - Test hooks with sample JSON payloads
5. **Use continueOnError wisely** - Don't mask important failures
6. **Log hook activity** - Help with debugging
