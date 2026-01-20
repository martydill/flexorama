# Flexoramas Documentation

This file contains documentation about the available Flexoramas in this project.

### Agent Capabilities

The Flexorama supports the following features:

1. **Interactive Mode**: Chat with the AI in a terminal interface
2. **Single Message Mode**: Send a single message and get a response
3. **Non-interactive Mode**: Read from stdin for scripting
4. **ACP Mode**: Agent Client Protocol server for editor integration (NEW!)
5. **Tool Support**: Execute various tools for file operations, code analysis, bash commands, etc.
6. **Context Management**: Maintains conversation history
7. **@file Syntax**: Auto-include files using @path-to-file syntax
8. **Progress Spinner**: Visual feedback while waiting for LLM responses
9. **System Prompts**: Set custom system prompts to control AI behavior and personality
10. **Streaming Support**: Real-time response streaming for immediate feedback
11. **Conversation Cancellation**: Press ESC to cancel ongoing AI conversations

### Available Tools

- **read_file**: Read the contents of a file
- **write_file**: Write content to a file (creates if doesn't exist)
- **edit_file**: Replace specific text in a file with new text
- **list_directory**: List contents of a directory
- **glob**: Find files and directories using glob patterns (read-only)
- **create_directory**: Create a directory (and parent directories if needed)
- **delete_file**: Delete a file or directory
- **bash**: Execute shell commands and return the output (with security)
- **create_todo**: Create a new todo item in the internal todo list
- **complete_todo**: Mark a todo item as completed using its ID
- **list_todos**: List all todo items with their status

### Usage Examples

```bash
# Interactive mode
flexorama

# Single message
flexorama -m "Hello, how are you?"

# Read from stdin
echo "Help me understand this code" | flexorama --non-interactive

# With API key via command line
flexorama -k "your-api-key" -m "Your message here"

# With API key from environment variable (RECOMMENDED)
export ANTHROPIC_AUTH_TOKEN="your-api-key"
flexorama -m "Your message here"

# With context files
flexorama -f config.toml -f Cargo.toml "Explain this project"

# Using @file syntax (NEW!)
flexorama "What does @Cargo.toml contain?"
flexorama "Compare @src/main.rs and @src/lib.rs"
flexorama "@file1.txt @file2.txt"

# With system prompts (NEW!)
flexorama -s "You are a Rust expert" "Help me with this code"
flexorama -s "Act as a code reviewer" -f main.rs "Review this code"
flexorama -s "You are a helpful assistant" "Explain this concept"

# With streaming support (NEW!)
flexorama --stream -m "Tell me a story"
flexorama --stream --non-interactive < input.txt
flexorama --stream  # Interactive mode with streaming

# Interactive mode examples
flexorama
> !dir                    # List directory contents
> !git status             # Check git status
> !cargo build            # Build the project
> /help                   # Show available commands
> /permissions allow "git *"  # Allow git commands
> ESC                     # Cancel current AI conversation
```

### Web App

- Launch with `flexorama --web [--web-port 3000]` (or `cargo run -- --web ...`) to start the local UI at `http://127.0.0.1:<port>`; when `--web` is supplied, `-m/--message` and `--non-interactive` flags are ignored.
- Chats tab lists conversations, supports message sending with streaming on/off, lets you switch the active agent from the dropdown, and includes a context modal showing files, the system prompt, and recent messages.
- Plans tab lists, creates, edits, and deletes plans (title, user request, markdown) that stay tied to a conversation.
- MCP Servers tab manages command- or WebSocket-based servers with args, env pairs, enabled flag, and connect/disconnect actions, and shows status per server.
- Agents tab creates/updates/deletes agents (system prompt, model, temperature, max tokens, allow/deny lists). New agents default to read-only tools (`search_in_files`, `glob`); activate via the header dropdown to switch subagents and conversations.
- UI controls include a light/dark toggle, stream toggle, and tab persistence via localStorage; the web app reads/writes the same SQLite data the CLI uses.

### ACP Mode (Agent Client Protocol)

Flexorama now supports the Agent Client Protocol (ACP), enabling integration with code editors like Zed, Neovim, and others. ACP uses JSON-RPC 2.0 over stdio for communication.

#### Starting ACP Server

```bash
# Start in ACP mode
flexorama --acp

# With debug logging
flexorama --acp --acp-debug

# With specific model
flexorama --acp --model claude-opus-4

# With yolo mode (no permissions)
flexorama --acp --yolo

# With plan mode (read-only)
flexorama --acp --plan-mode
```

#### ACP Capabilities

- **File System Operations**: Read, write, list, search, delete files and directories
- **Agent Prompts**: Send prompts and receive AI-generated responses
- **Context Management**: Add files to conversation context
- **Code Editing**: Apply string replacement edits
- **Tool Execution**: All Flexorama tools available via ACP
- **Workspace Management**: Path resolution relative to workspace root
- **Permission System**: File and bash operations respect security settings

#### ACP Methods

- `initialize` - Initialize server and negotiate capabilities
- `shutdown` / `exit` - Graceful server shutdown
- `agent/prompt` - Send prompts to AI
- `agent/cancel` - Cancel ongoing operations
- `fs/readFile`, `fs/writeFile`, `fs/listDirectory`, `fs/glob`, `fs/delete`, `fs/createDirectory` - File operations
- `context/addFile`, `context/clear` - Context management
- `edit/applyEdit` - Apply code edits

For detailed ACP documentation, see [docs/ACP_USAGE.md](docs/ACP_USAGE.md).

### System Prompts

System prompts allow you to control the AI's behavior, personality, and response style. They are set at the beginning of the conversation and influence all subsequent responses.

#### System Prompt Examples

```bash
# Set the AI to act as a specific expert
flexorama -s "You are a senior Rust developer with 10 years of experience" "Review this code"

# Set a specific response style
flexorama -s "Respond in a concise, technical manner" "Explain distributed systems"

# Set a specific context or role
flexorama -s "You are a code reviewer. Focus on security, performance, and maintainability" -f app.rs "Review this file"

# Multiple instructions
flexorama -s "You are a helpful coding assistant. Always provide code examples and explain your reasoning" "How do I implement a binary tree in Rust?"
```

#### When to Use System Prompts

- **Code Review**: Set the AI to act as a senior developer reviewing code
- **Learning**: Set the AI to act as a teacher explaining concepts
- **Specific Domains**: Set the AI as an expert in a particular field
- **Response Style**: Control how detailed, technical, or casual the responses should be
- **Context Setting**: Provide background information that should influence all responses

### Shell Command Execution

The agent can execute shell commands directly using two different methods:

#### 1. AI-Executed Commands
The agent can automatically execute shell commands when you ask it to:

```bash
# List files in current directory
flexorama "List the files in the current directory"

# Check git status
flexorama "Check the git status"

# Run tests
flexorama "Run tests and show me the results"

# Execute multiple commands
flexorama "Check the current branch and run the build process"
```

#### 2. Direct Shell Commands (!)
In interactive mode, you can use `!` commands to execute shell commands directly:

```bash
# Start interactive mode
flexorama

# Then use shell commands directly
> !dir
> !ls -la
> !git status
> !cargo build
> !cargo test
> !pwd
> !ps aux
```

#### Security for Shell Commands
**Important distinction** between AI-executed and direct shell commands:

- **AI-Executed Commands**: Subject to security permissions, allowlist/denylist checks
- **Direct Shell Commands (!)**: Execute immediately without permission checks for full user control

Use `/permissions` to manage security settings for AI-executed commands only. Direct `!` commands provide unrestricted shell access.

#### Platform Support

The shell command tool automatically detects the operating system and uses the appropriate shell:
- **Windows**: Uses `cmd.exe /C` for command execution
- **Unix/Linux/macOS**: Uses `bash -c` for command execution

### Configuration

The agent can be configured via:

1. **Environment Variables**:
   - `ANTHROPIC_AUTH_TOKEN`: Your API key (required)
   - `ANTHROPIC_BASE_URL`: Custom base URL (default: https://api.anthropic.com/v1)

2. **Command Line**:
   - `-k` or `--api-key`: Set API key via command line

3. **Config File**: Located at `~/.config/flexorama/config.toml` (API keys are excluded for security)

⚠️ **Security Note**: API keys are **never** stored in config files for security reasons. Always use environment variables or command line flags.

Example config file (API key excluded):
```toml
base_url = "https://api.anthropic.com/v1"
default_model = "glm-4.6"
max_tokens = 4096
temperature = 0.7
```

#### API Key Security Best Practices
- **Use environment variables** for API keys (recommended)
- **Use command line flag `-k`** for temporary API keys
- **Never commit API keys** to version control
- **API keys are automatically excluded** from config files
- **Use `.env` files** for local development (add to .gitignore)

### Context Files

The agent supports multiple ways to include files as context:

1. **Command Line Flag**: Use `-f` or `--file` to specify files
2. **@file Syntax**: Use `@path-to-file` directly in messages
3. **Auto-inclusion**: AGENTS.md is automatically included if it exists

#### @file Syntax Examples
```bash
# Single file
flexorama "What does @config.toml contain?"

# Multiple files
flexorama "Compare @file1.rs and @file2.rs"

# File with question
flexorama "Explain the Rust code in @src/main.rs"

# Only file references
flexorama "@file1.txt @file2.txt"
```

### Progress Spinner

The agent now includes a visual progress spinner that appears while waiting for LLM responses. The spinner provides immediate feedback that the system is processing your request:

- **Spinner Characters**: Rotating Unicode characters (⠋ ⠙ ⠹ ⠸ ⠼ ⠴ ⠦ ⠧ ⠇ ⠏)
- **Message**: Shows "Thinking..." while processing
- **Color**: Green spinner with clear visibility
- **Behavior**: Automatically clears when the response is ready

The spinner appears in all modes:
- Interactive mode (during conversation)
- Single message mode
- Non-interactive mode (stdin)

### Streaming Support

The agent now supports streaming responses for real-time feedback as the AI generates its response:

- **Real-time Output**: See responses as they're being generated
- **Reduced Perceived Latency**: No waiting for complete response
- **Visual Feedback**: Immediate indication that the system is working
- **Backward Compatible**: Existing functionality unchanged
- **Optional**: Can be enabled via `--stream` flag

#### Streaming Examples
```bash
# Enable streaming for single message
flexorama --stream -m "Tell me a story"

# Enable streaming for stdin
echo "Explain quantum computing" | flexorama --stream --non-interactive

# Enable streaming in interactive mode
flexorama --stream

# Compare streaming vs non-streaming
flexorama -m "What's the weather like?"  # Shows spinner, then formatted response
flexorama --stream -m "What's the weather like?"  # Shows real-time response
```

#### When to Use Streaming
- **Long Responses**: Better experience for detailed explanations
- **Interactive Sessions**: More natural conversation flow
- **Real-time Needs**: When you need immediate feedback
- **Scripting**: Better for pipelines where you want immediate output

#### When to Use Non-Streaming
- **Short Responses**: Spinner provides better UX for quick responses
- **Formatted Output**: Non-streaming mode applies syntax highlighting
- **Debugging**: Easier to capture complete response for troubleshooting

### Conversation Cancellation

The agent supports cancelling ongoing AI conversations using the ESC key:

- **ESC Key**: Press ESC during AI processing to immediately cancel the current conversation
- **Visual Feedback**: Shows "🛑 Cancelling AI conversation..." when cancellation is triggered
- **Clean Exit**: Gracefully stops API calls and tool execution
- **Continue Working**: Returns to the prompt without losing conversation context

#### Cancellation Behavior
- Works in all modes (interactive, single message, non-interactive)
- Cancels both streaming and non-streaming responses
- Stops tool execution that may be in progress
- Preserves conversation history for continued interaction
- Shows clear user feedback when cancellation occurs

#### Cancellation Examples
```bash
# Start a long-running conversation
flexorama "Write a detailed analysis of quantum computing"

# Press ESC during processing to cancel
# Output: 🛑 Cancelling AI conversation...

# Continue with a new request
flexorama "What's the weather like today?"
```

### Slash Commands

In interactive mode, you can use these commands:

- `/help` - Show help information
- `/stats` - Show token usage statistics
- `/usage` - Show token usage statistics (alias for /stats)
- `/context` - Show current conversation context (including system prompt)
- `/clear` - Clear all conversation context (keeps AGENTS.md if it exists)
- `/reset-stats` - Reset token usage statistics
- `/exit` or `/quit` - Exit the program

### Cancellation Commands

- **ESC** - Cancel current AI conversation during processing
- **Ctrl+C** - Exit the program immediately

### Shell Commands (!)

In interactive mode, you can use `!` commands to execute shell commands directly:

- `!<command>` - Execute a shell command and display the output
- Examples: `!dir`, `!ls -la`, `!git status`, `!cargo test`
- **Note**: Shell commands with `!` bypass all security permissions for unrestricted access

#### Shell Command Examples
```bash
# List directory contents
!dir

# List files with details (Unix)
!ls -la

# Check git status
!git status

# Build project
!cargo build

# Run tests
!cargo test

# Show current directory
!pwd

# List processes
!ps aux

# Any command executes without permission checks
!sudo apt update
!rm -rf /tmp/*
!chmod +x script.sh
```

#### Security for Shell Commands
Direct shell commands (`!`) bypass all security restrictions:
- **No permission checks**: Commands execute immediately
- **No allowlist/denylist**: All commands are allowed
- **No interactive prompts**: No confirmation dialogs
- **User responsibility**: You have full control and responsibility

⚠️ **Warning**: `!` commands provide unrestricted shell access. Use with caution and only execute commands you trust.

### Error Handling

The agent includes comprehensive error handling for:
- API authentication failures
- Network connectivity issues
- File operation errors
- Tool execution failures
- Bash command execution failures
- Invalid file references in @file syntax
- Streaming connection failures (graceful fallback to non-streaming)
- Conversation cancellation handling

All errors are displayed with clear, actionable messages to help troubleshoot issues. When a conversation is cancelled via ESC, the system provides clear feedback and returns to the prompt gracefully.

### Configuration for Streaming

Streaming can be enabled via:
1. **Command Line Flag**: Use `--stream` flag
2. **Default Behavior**: Non-streaming remains the default for backward compatibility
3. **Mode Support**: Available in all modes (single message, non-interactive, interactive)

#### Streaming Configuration Examples
```bash
# Per-request streaming
flexorama --stream -m "Your message"

# Interactive mode with streaming
flexorama --stream

# Non-interactive with streaming
cat input.txt | flexorama --stream --non-interactive

# Combine with other options
flexorama --stream -s "You are an expert" -f context.txt "Analyze this"
```

#### Rules
 - Any time you create a doc, it must go in the docs folder. Any time you need to read a doc, look in the docs folder.

### Todo Management

The agent includes built-in todo management tools that allow the LLM to track and manage tasks internally. This is useful for:

- **Task Planning**: Break down complex tasks into manageable todo items
- **Progress Tracking**: Keep track of what has been completed
- **Work Organization**: Maintain a list of action items during conversations

#### Todo Tools

1. **create_todo**: Create a new todo item
   - Parameter: `description` (string) - The task description
   - Returns: Todo ID and confirmation message

2. **complete_todo**: Mark a todo as completed
   - Parameter: `id` (string) - The todo item ID
   - Returns: Updated todo status

3. **list_todos**: Show all todo items
   - No parameters required
   - Returns: List of all todos with their completion status

#### Todo Examples

```bash
# Create todos for a project
flexorama "Create todos for: 1) Design database schema, 2) Implement API endpoints, 3) Write tests, 4) Deploy to production"

# List all todos
flexorama "Show me all the todos"

# Complete a specific todo
flexorama "Mark todo 'todo-1234567890' as completed"

# Work through a task list
flexorama "Create a todo list for refactoring the authentication module, then work through each item"
```

#### Todo Storage

- **In-Memory**: Todos are stored in memory during the agent session
- **Per-Session**: Each new agent session starts with an empty todo list
- **JSON Format**: Todo items are returned in JSON format with id, description, and completed status

#### Todo Item Structure

```json
{
  "id": "todo-1767888578",
  "description": "Review pull requests",
  "completed": false
}
```
