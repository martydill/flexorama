# Flexorama Hooks Support

Flexorama provides **full Claude Code hooks compatibility** while using the `.flexorama` directory (instead of `.claude`). Hook definitions can live in either:

- `~/.flexorama` (global hooks)
- `<project>/.flexorama` (project hooks)

## 🎯 Claude Code Compatibility

Flexorama is **fully compatible** with Claude Code hooks:

✅ **All Claude Code event names supported** (UserPromptSubmit, PreToolUse, PostToolUse, Stop, etc.)
✅ **Claude Code settings.json format supported**
✅ **Claude Code response format supported** (decision, reason, continue, stopReason)
✅ **Claude Code payload structure supported** (session_id, timestamp, tool_name, etc.)
✅ **Legacy Flexorama event names still work** (pre_message, post_message, etc.)

**Note:** The only difference is that configuration files live in `.flexorama/` instead of `.claude/`.

## Configuration Formats

### 1. Claude Code Style: settings.json

Flexorama supports the official Claude Code `settings.json` format:

**Location:** `.flexorama/settings.json`

```json
{
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
      "hooks": [
        {
          "type": "command",
          "command": "check-tool",
          "args": ["--strict"]
        }
      ]
    }
  ]
}
```

### 2. Flexorama Style: hooks.json/yaml/toml

Flexorama also supports its own simplified configuration format:

**Locations:**
- `.flexorama/hooks.json`
- `.flexorama/hooks.yaml`
- `.flexorama/hooks.yml`
- `.flexorama/hooks.toml`
- `.flexorama/hooks` (auto-detected format)

**Format 1: With "hooks" wrapper**
```json
{
  "hooks": {
    "UserPromptSubmit": [
      "echo pre-message",
      {"command": "./scripts/check.sh", "args": ["--fast"], "continue_on_error": true}
    ],
    "PreToolUse": [
      "./scripts/check-tool.sh"
    ]
  }
}
```

**Format 2: Direct event mapping**
```yaml
UserPromptSubmit:
  - echo pre-message
  - command: ./scripts/check.sh
    args: [--fast]
    continue_on_error: true
    timeout_ms: 5000
    shell: bash

PreToolUse:
  - ./scripts/check-tool.sh
```

### 3. Hook Scripts Directory

Place executable hook scripts inside `.flexorama/hooks/`. Each filename (without extension) becomes the event name.

**Example:**
- `.flexorama/hooks/UserPromptSubmit` (executable script)
- `.flexorama/hooks/PreToolUse` (executable script)

**Requirements:**
- **Unix:** Files must be executable (`chmod +x script`)
- **Windows:** Files must have executable extension (.exe, .bat, .cmd, .ps1, .py, .rb, .js, .sh)

## Supported Hook Events

Flexorama supports **all** Claude Code hook events:

| Claude Code Event | Legacy Aliases | Description |
|-------------------|---------------|-------------|
| `UserPromptSubmit` | `pre_message`, `before_message`, `user_message`, `prompt_before` | Fires when user submits a prompt |
| `PreToolUse` | `pre_tool`, `before_tool`, `tool_call` | Fires before tool execution |
| `PostToolUse` | `post_tool`, `after_tool`, `tool_result` | Fires after tool completes |
| `Stop` | `post_message`, `after_message`, `response` | Fires when agent finishes responding |
| `SubagentStop` | `subagent_stop`, `after_subagent` | Fires when subagent completes |
| `SessionStart` | `session_start`, `on_start`, `init` | Fires at session initialization |
| `PreCompact` | `pre_compact`, `before_compact` | Fires before context compaction |
| `Notification` | `notification`, `on_notification` | Fires on notifications |
| `PermissionRequest` | `permission_request`, `on_permission` | Fires on permission requests |

**You can use either the Claude Code event names or the legacy Flexorama aliases** - they work identically.

## Hook Command Options

Both configuration formats support these options:

| Option | Type | Description |
|--------|------|-------------|
| `command` | string | Command to execute (required) |
| `args` | array | Command arguments (optional, defaults to []) |
| `env` | object | Environment variables (optional) |
| `working_dir` / `workingDirectory` | string | Working directory (optional) |
| `timeout_ms` / `timeoutMs` | number | Timeout in milliseconds (optional) |
| `shell` | string | Shell to use (optional, auto-detected) |
| `continue_on_error` / `continueOnError` | boolean | Continue if hook fails (optional, defaults to false) |

## Hook Execution Flow

### 1. Input (stdin)

Hooks receive a JSON payload on **stdin** with Claude Code compatible structure:

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

Hooks can return a JSON response to control execution. **Both Claude Code and Flexorama response formats are supported:**

#### Claude Code Format (Recommended)

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

#### Flexorama Format (Legacy)

**Abort:**
```json
{
  "action": "abort",
  "message": "Operation cancelled"
}
```

or

```json
{
  "abort": true,
  "message": "Operation cancelled"
}
```

**Modify user message (UserPromptSubmit only):**
```json
{
  "user_message": "Modified user prompt"
}
```

**Modify tool arguments (PreToolUse only):**
```json
{
  "tool_arguments": {
    "path": "different_file.txt",
    "content": "modified content"
  }
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
| `FLEXORAMA_HOOK_EVENT` | Same as CLAUDE_CODE_HOOK_EVENT |
| `FLEXORAMA_PROJECT_ROOT` | Same as CLAUDE_CODE_PROJECT_ROOT |
| `FLEXORAMA_HOOK_SOURCE` | Same as CLAUDE_CODE_HOOK_SOURCE |

Plus any custom environment variables specified in the hook configuration.

## Shell Detection

Flexorama automatically detects the best available shell:

**Windows:**
1. PowerShell Core (`pwsh`) - preferred
2. Windows PowerShell (`powershell`)
3. Command Prompt (`cmd`)

**Unix/Linux/macOS:**
1. Bash (`bash`) - preferred
2. POSIX shell (`sh`)

You can override this with the `shell` option in your hook configuration.

## Security and Safety Features

### 1. Hook Execution Timeout

- **Individual hooks:** Configurable per-hook timeout via `timeout_ms` / `timeoutMs`
- **Overall timeout:** All hooks for an event have a combined 30-second timeout

### 2. Executable Validation

Hook scripts in `.flexorama/hooks/` must be executable:
- **Unix:** Files must have execute permission
- **Windows:** Files must have valid executable extension

Non-executable files are skipped with a warning.

### 3. Error Handling

- Hooks fail by default (abort execution)
- Use `continue_on_error: true` to allow execution to continue despite hook failures

### 4. Duplicate Prevention

If you define the same hook under multiple event aliases (e.g., both "UserPromptSubmit" and "pre_message"), it will only execute once.

## Example Use Cases

### 1. Log all user prompts

**.flexorama/settings.json:**
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
  "hooks": {
    "PreToolUse": [
      {
        "command": "bash",
        "args": ["-c", "jq -r '.tool_input.command // empty' | grep -qE '(rm -rf|dd if=)' && echo '{\"decision\":\"block\",\"reason\":\"Dangerous command blocked\"}' || echo '{\"decision\":\"approve\"}'"]
      }
    ]
  }
}
```

### 3. Add context to prompts

**.flexorama/hooks/UserPromptSubmit** (executable script):
```bash
#!/bin/bash
# Read the payload
payload=$(cat)

# Extract the prompt
prompt=$(echo "$payload" | jq -r '.prompt')

# Add git branch info
branch=$(git branch --show-current 2>/dev/null || echo "unknown")

# Return modified prompt
echo "{\"user_message\": \"[Branch: $branch] $prompt\"}"
```

### 4. Enforce task completion

**.flexorama/settings.json:**
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

## Migration from Claude Code

If you're migrating from Claude Code:

1. **Copy your `.claude` directory to `.flexorama`:**
   ```bash
   cp -r .claude .flexorama
   ```

2. **Your hooks will work immediately!** Flexorama supports:
   - Claude Code event names
   - Claude Code settings.json format
   - Claude Code response format
   - Claude Code payload structure

3. **Optional:** Convert to Flexorama format for simpler syntax
   - Use `hooks.json` / `hooks.yaml` instead of `settings.json`
   - Use legacy event names if preferred

## Troubleshooting

### Hooks not executing

1. Check file permissions (Unix: `chmod +x script`)
2. Check file extensions (Windows)
3. Verify hook configuration syntax
4. Check Flexorama logs for errors

### Hook timing out

1. Increase `timeout_ms` for individual hooks
2. Optimize slow hooks
3. Check for blocking operations

### Hook failing

1. Check hook exit code (0 = success)
2. Review stderr output
3. Use `continue_on_error: true` if failure is acceptable
4. Test hook independently: `echo '{}' | your-hook-script`

## Best Practices

1. **Keep hooks fast** - They run synchronously and block execution
2. **Use timeouts** - Prevent hooks from hanging indefinitely
3. **Return valid JSON** - Invalid JSON is ignored
4. **Test independently** - Test hooks with sample JSON payloads
5. **Use continue_on_error wisely** - Don't mask important failures
6. **Log hook activity** - Help with debugging
7. **Prefer Claude Code event names** - Better compatibility and clarity

## Differences from Claude Code

| Feature | Claude Code | Flexorama |
|---------|-------------|-----------|
| Config directory | `.claude` | `.flexorama` |
| Event names | ✅ All supported | ✅ All supported + aliases |
| settings.json | ✅ Supported | ✅ Supported |
| Response format | ✅ Supported | ✅ Supported + legacy format |
| Payload structure | ✅ Supported | ✅ Supported + extensions |
| Hook scripts | ✅ Supported | ✅ Supported + validation |
| Multiple formats | JSON only | JSON, YAML, TOML |
| Overall timeout | ❓ | ✅ 30 seconds |

## Advanced Features

### Hook Composition

You can define multiple hooks for the same event - they execute in order:

```json
{
  "hooks": {
    "PreToolUse": [
      "./validate-input.sh",
      "./check-permissions.sh",
      "./log-attempt.sh"
    ]
  }
}
```

### Custom Environment Variables

Pass custom environment to hooks:

```json
{
  "command": "./my-hook.sh",
  "env": {
    "API_KEY": "secret",
    "ENVIRONMENT": "production"
  }
}
```

### Working Directory

Change working directory for hook execution:

```json
{
  "command": "./scripts/hook.sh",
  "working_dir": "./scripts"
}
```

## Summary

Flexorama provides **full Claude Code hooks compatibility** with these enhancements:

✅ Multiple configuration formats (JSON, YAML, TOML)
✅ Simplified syntax options
✅ Automatic shell detection
✅ Executable validation
✅ Overall timeout protection
✅ Duplicate hook prevention
✅ Both Claude Code and Flexorama response formats

**Just change `.claude` to `.flexorama` and your Claude Code hooks work perfectly!**
