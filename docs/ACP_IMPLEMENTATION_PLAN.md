# Agent Client Protocol (ACP) Implementation Plan

## Overview

This document outlines the comprehensive plan for implementing full Agent Client Protocol (ACP) support in Flexorama. ACP is a JSON-RPC 2.0 protocol over stdio that enables code editors to communicate with AI coding agents, similar to how LSP (Language Server Protocol) works for language servers.

## Background

The Agent Client Protocol standardizes communication between:
- **Editors** (Zed, Neovim, VS Code, etc.) - interactive programs for viewing and editing source code
- **Coding Agents** (like Flexorama) - programs that use generative AI to autonomously modify code

### Key Features of ACP
- JSON-RPC 2.0 based protocol
- Transport over stdio (stdin/stdout)
- Bidirectional communication
- Capability negotiation
- Permission system for file operations
- Process isolation and language independence

## Current State

Flexorama currently supports:
- ✅ CLI mode (interactive, single message, non-interactive)
- ✅ Web UI mode
- ✅ MCP (Model Context Protocol) for tool integration
- ✅ Tool system with permissions
- ✅ Database for conversation persistence
- ✅ Subagent system
- ❌ ACP support (to be implemented)

## Implementation Phases

### Phase 1: Foundation & Setup

#### 1.1 Add Dependencies
**File**: `Cargo.toml`

```toml
[dependencies]
# Add ACP support
agent-client-protocol = "0.6"
agent-client-protocol-schema = "0.6"
```

**Tasks**:
- Add `agent-client-protocol` crate
- Add `agent-client-protocol-schema` for type definitions
- Verify tokio features support stdio handling
- Update dependencies and resolve any conflicts

#### 1.2 Create ACP Module Structure

**New Files**:
- `src/acp/mod.rs` - Main ACP module, exports public API
- `src/acp/server.rs` - ACP server implementation, stdio loop
- `src/acp/handler.rs` - Request/response handlers
- `src/acp/capabilities.rs` - Capability negotiation logic
- `src/acp/transport.rs` - Stdio transport layer
- `src/acp/types.rs` - ACP-specific types and conversions
- `src/acp/errors.rs` - ACP error types and handling

**Tasks**:
- Create module structure
- Set up basic module exports
- Add module to `src/lib.rs`
- Create placeholder implementations

#### 1.3 CLI Integration
**File**: `src/cli.rs`

Add new flags:
```rust
/// Enable ACP (Agent Client Protocol) server mode
#[arg(long)]
pub acp: bool,

/// Log ACP messages for debugging
#[arg(long)]
pub acp_debug: bool,
```

**File**: `src/main.rs`

Add ACP mode handling:
```rust
if cli.acp {
    run_acp_mode(&mut agent, cli.acp_debug).await?;
}
```

**Tasks**:
- Add `--acp` flag for ACP server mode
- Add `--acp-debug` flag for message logging
- Ensure ACP mode is mutually exclusive with web/interactive modes
- Update help text and documentation

### Phase 2: Core ACP Implementation

#### 2.1 Implement Agent Trait
**File**: `src/acp/handler.rs`

Implement the `Agent` trait from `agent-client-protocol`:

```rust
use agent_client_protocol::{Agent, InitializeParams, InitializeResult};

pub struct FlexoramaAgent {
    agent: Arc<Mutex<crate::agent::Agent>>,
    capabilities: Capabilities,
}

#[async_trait]
impl Agent for FlexoramaAgent {
    async fn initialize(&mut self, params: InitializeParams) -> Result<InitializeResult> {
        // Handle initialization, negotiate capabilities
    }

    async fn shutdown(&mut self) -> Result<()> {
        // Clean shutdown
    }
}
```

**Tasks**:
- Implement `initialize` method with capability negotiation
- Implement `shutdown` for clean exit
- Handle authentication if required
- Support workspace initialization
- Map ACP client capabilities to Flexorama features

#### 2.2 Stdio Transport
**File**: `src/acp/transport.rs`

Implement JSON-RPC 2.0 over stdio:

```rust
pub struct StdioTransport {
    stdin: tokio::io::BufReader<tokio::io::Stdin>,
    stdout: tokio::io::Stdout,
}

impl StdioTransport {
    pub async fn read_message(&mut self) -> Result<JsonRpcMessage> {
        // Read newline-delimited JSON from stdin
    }

    pub async fn write_message(&mut self, msg: JsonRpcMessage) -> Result<()> {
        // Write JSON + newline to stdout
    }
}
```

**Tasks**:
- Implement newline-delimited JSON message reading
- Implement JSON-RPC 2.0 message writing
- Add error handling for malformed messages
- Implement message buffering
- Ensure stderr is used for logging (stdout for protocol)

#### 2.3 JSON-RPC Message Handling
**File**: `src/acp/server.rs`

Create main server loop:

```rust
pub async fn run_acp_server(
    agent: crate::agent::Agent,
    debug: bool,
) -> Result<()> {
    let transport = StdioTransport::new();
    let handler = FlexoramaAgent::new(agent);

    loop {
        let msg = transport.read_message().await?;
        let response = handler.handle_message(msg).await?;
        transport.write_message(response).await?;
    }
}
```

**Tasks**:
- Create JSON-RPC request parser
- Create JSON-RPC response builder
- Handle notification messages (no response expected)
- Implement error responses with proper error codes
- Support request/response correlation via ID

### Phase 3: File System Integration

#### 3.1 File System Access
**File**: `src/acp/filesystem.rs`

Map ACP file operations to Flexorama tools:

```rust
impl FileSystemOperations for FlexoramaAgent {
    async fn read_file(&self, path: &str) -> Result<String> {
        // Use existing read_file tool
    }

    async fn write_file(&self, path: &str, content: &str) -> Result<()> {
        // Use existing write_file tool with permissions
    }

    async fn list_directory(&self, path: &str) -> Result<Vec<FileEntry>> {
        // Use existing list_directory tool
    }
}
```

**Tasks**:
- Map `read_file` → ACP file read
- Map `write_file` → ACP file write (with permissions)
- Map `list_directory` → ACP directory listing
- Map `glob` → ACP file search
- Handle symbolic links properly
- Support binary files (base64 encoding)

#### 3.2 Permission System Integration
**File**: `src/acp/permissions.rs`

Integrate with existing security managers:

```rust
pub struct AcpPermissionManager {
    file_security: Arc<RwLock<FileSecurityManager>>,
    bash_security: Arc<RwLock<BashSecurityManager>>,
    client: Arc<dyn PermissionClient>,
}

impl AcpPermissionManager {
    pub async fn request_file_permission(&self, path: &str, operation: FileOp) -> Result<bool> {
        // Check existing permissions first
        // Request from client if needed
        // Cache grant/deny
    }
}
```

**Tasks**:
- Integrate with existing `FileSecurityManager`
- Integrate with existing `BashSecurityManager`
- Request permissions from editor/client when needed
- Cache permission grants per session
- Support permission revocation
- Handle yolo mode (bypass all permissions)

#### 3.3 Workspace Management
**File**: `src/acp/workspace.rs`

```rust
pub struct Workspace {
    root: PathBuf,
    settings: WorkspaceSettings,
}

impl Workspace {
    pub fn resolve_path(&self, relative: &str) -> PathBuf {
        // Resolve relative to workspace root
    }
}
```

**Tasks**:
- Support workspace root detection from editor
- Handle relative vs absolute paths
- Implement workspace-relative path resolution
- Support multiple workspace folders
- Validate paths are within workspace

### Phase 4: Agent Operations

#### 4.1 Prompt Handling
**File**: `src/acp/prompts.rs`

```rust
impl FlexoramaAgent {
    async fn handle_prompt(&mut self, prompt: PromptRequest) -> Result<PromptResponse> {
        // Convert ACP prompt to Flexorama message
        let message = prompt.content;

        // Process with existing agent
        let response = self.agent.lock().await
            .process_message(&message, cancellation_flag)
            .await?;

        // Convert to ACP response
        Ok(PromptResponse {
            content: response,
            metadata: self.collect_metadata(),
        })
    }
}
```

**Tasks**:
- Receive prompts from editor via ACP
- Convert ACP prompts to Flexorama conversation format
- Stream responses back to editor (if supported)
- Support multi-turn conversations
- Include context files automatically
- Handle prompt metadata (file context, selection, etc.)

#### 4.2 Tool Execution via ACP
**File**: `src/acp/tools.rs`

```rust
impl FlexoramaAgent {
    async fn handle_tool_call(&mut self, tool: ToolCallRequest) -> Result<ToolCallResponse> {
        // Map ACP tool call to Flexorama tool
        let tool_call = ToolCall {
            id: tool.id,
            name: tool.name,
            arguments: tool.arguments,
        };

        // Execute with existing tool system
        let result = self.agent.lock().await
            .execute_tool(&tool_call)
            .await?;

        Ok(ToolCallResponse::from(result))
    }
}
```

**Tasks**:
- Expose Flexorama tools through ACP
- Map tool calls to ACP operations
- Support tool use confirmations/permissions
- Handle tool execution progress/status
- Stream tool output if possible
- Support MCP tools via ACP

#### 4.3 Context Management
**File**: `src/acp/context.rs`

```rust
pub struct ContextManager {
    files: Vec<ContextFile>,
    workspace: Workspace,
}

impl ContextManager {
    pub async fn add_file_context(&mut self, path: &str) -> Result<()> {
        // Read file and add to context
    }

    pub async fn get_context_for_prompt(&self) -> Vec<Message> {
        // Convert context files to messages
    }
}
```

**Tasks**:
- Send context files to LLM
- Support context from multiple files
- Handle large context efficiently (truncation/summarization)
- Support incremental context updates
- Track context usage/tokens
- Support different context types (files, selection, diagnostics)

### Phase 5: Advanced Features

#### 5.1 Capability Negotiation
**File**: `src/acp/capabilities.rs`

```rust
pub struct Capabilities {
    pub file_system: FileSystemCapabilities,
    pub tools: ToolCapabilities,
    pub streaming: bool,
    pub multi_turn: bool,
    pub code_editing: bool,
    pub shell_execution: bool,
}

impl Capabilities {
    pub fn negotiate(client_caps: ClientCapabilities) -> Self {
        // Negotiate based on what client supports
    }
}
```

**Tasks**:
- Advertise file system access capability
- Advertise tool execution capability
- Advertise streaming responses capability
- Advertise multi-turn conversations
- Advertise code editing capability
- Advertise shell command execution
- Support capability extensions

#### 5.2 Progress & Status Updates
**File**: `src/acp/progress.rs`

```rust
impl FlexoramaAgent {
    async fn send_progress(&self, token: &str, progress: Progress) {
        // Send progress notification to client
    }

    async fn send_status(&self, status: Status) {
        // Send status update
    }
}
```

**Tasks**:
- Send progress notifications to editor
- Report tool execution status
- Stream LLM response chunks
- Handle cancellation requests from client
- Support progress tokens
- Report estimated completion

#### 5.3 Edit Operations
**File**: `src/acp/edits.rs`

```rust
pub struct EditOperation {
    pub file: String,
    pub range: Range,
    pub new_text: String,
}

impl FlexoramaAgent {
    async fn apply_edit(&mut self, edit: EditOperation) -> Result<()> {
        // Validate edit
        // Request permission if needed
        // Apply edit using edit_file tool
        // Send confirmation
    }
}
```

**Tasks**:
- Support code edit requests from editor
- Validate edits before applying
- Send edit confirmations to client
- Support edit rollback/undo
- Batch multiple edits efficiently
- Support different edit formats (replace, insert, delete)

#### 5.4 Diagnostics & Logging
**File**: `src/acp/diagnostics.rs`

```rust
pub struct DiagnosticManager {
    debug_mode: bool,
}

impl DiagnosticManager {
    pub fn log_message(&self, msg: &JsonRpcMessage) {
        if self.debug_mode {
            eprintln!("[ACP] {}", serde_json::to_string(msg).unwrap());
        }
    }
}
```

**Tasks**:
- Log ACP messages for debugging (when `--acp-debug` enabled)
- Send diagnostic notifications to client
- Use stderr for all logging (stdout reserved for protocol)
- Support log level configuration
- Report errors clearly
- Include helpful troubleshooting info

### Phase 6: Integration & Testing

#### 6.1 Integration with Existing Components

**Agent Integration** (`src/agent.rs`):
```rust
impl Agent {
    pub fn for_acp(config: Config) -> Self {
        // Create agent suitable for ACP mode
        // May need different defaults
    }
}
```

**Tasks**:
- Integrate with existing `Agent` struct (may need refactoring)
- Reuse `ConversationManager` for multi-turn conversations
- Reuse `ToolRegistry` and tool system
- Integrate with `DatabaseManager` for persistence
- Support MCP tools in ACP mode
- Handle subagents if needed

#### 6.2 State Management
**File**: `src/acp/state.rs`

```rust
pub struct AcpSessionState {
    conversation_id: String,
    workspace: Workspace,
    permissions: PermissionCache,
    capabilities: Capabilities,
}
```

**Tasks**:
- Manage ACP session state
- Handle multiple concurrent requests (if protocol supports)
- Support conversation persistence across restarts
- Clean up resources on shutdown
- Support session recovery
- Track active operations

#### 6.3 Testing Strategy

**Unit Tests**:
- `src/acp/transport.rs` - JSON-RPC parsing tests
- `src/acp/handler.rs` - Agent trait implementation tests
- `src/acp/capabilities.rs` - Capability negotiation tests
- `src/acp/permissions.rs` - Permission logic tests

**Integration Tests** (`tests/acp_integration.rs`):
```rust
#[tokio::test]
async fn test_acp_initialization() {
    // Test full initialization flow
}

#[tokio::test]
async fn test_acp_prompt_response() {
    // Test prompt handling end-to-end
}
```

**Test Files**:
- `tests/acp_integration.rs` - End-to-end integration tests
- `tests/mock_editor.rs` - Mock ACP client for testing
- `tests/acp_fixtures.rs` - Test fixtures and helpers

**Tasks**:
- Create unit tests for JSON-RPC parsing
- Create unit tests for Agent trait implementation
- Create integration tests with mock editor client
- Create end-to-end tests with real ACP client (if available)
- Test permission flows thoroughly
- Test error handling and edge cases
- Test cancellation
- Test large files and contexts
- Test concurrent operations

### Phase 7: Documentation & Examples

#### 7.1 Documentation

**Update Files**:
- `README.md` - Add ACP section
- `AGENTS.md` - Document ACP mode
- `docs/ACP_USAGE.md` - Detailed ACP usage guide
- `docs/ACP_TROUBLESHOOTING.md` - Common issues and solutions

**Tasks**:
- Update README with ACP overview
- Document ACP-specific flags and options
- Add troubleshooting guide for common issues
- Document supported capabilities in detail
- Add architecture diagram showing ACP flow
- Document security considerations
- Add FAQ section

#### 7.2 Examples

**New Files**:
- `examples/acp_client.rs` - Minimal ACP client example
- `examples/acp_workflows.md` - Example workflows
- `examples/editor_configs/` - Editor integration examples
  - `examples/editor_configs/zed.json` - Zed configuration
  - `examples/editor_configs/neovim.lua` - Neovim configuration

**Tasks**:
- Create minimal ACP client example in Rust
- Add example workflows (common use cases)
- Document integration with Zed editor
- Document integration with Neovim
- Document integration with VS Code (if supported)
- Provide sample configuration files
- Show advanced usage patterns

#### 7.3 Configuration

**Update**: `src/config.rs`

Add ACP-specific options:
```rust
#[derive(Debug, Deserialize, Serialize)]
pub struct AcpConfig {
    pub debug: bool,
    pub max_message_size: usize,
    pub timeout_seconds: u64,
    pub allowed_capabilities: Vec<String>,
}
```

**Tasks**:
- Add ACP section to config file
- Support editor-specific customization
- Document all configuration options
- Provide example configurations
- Document security implications of each option
- Support workspace-specific config

### Phase 8: Polish & Deployment

#### 8.1 Error Handling

**File**: `src/acp/errors.rs`

```rust
#[derive(Debug, thiserror::Error)]
pub enum AcpError {
    #[error("Invalid JSON-RPC message: {0}")]
    InvalidMessage(String),

    #[error("Permission denied: {0}")]
    PermissionDenied(String),

    #[error("File not found: {0}")]
    FileNotFound(String),

    // ... more error types
}
```

**Tasks**:
- Implement graceful error recovery
- Provide clear, actionable error messages
- Include detailed error logs for debugging
- Map internal errors to ACP error codes
- Support error metadata (file, line, context)
- Never expose sensitive information in errors

#### 8.2 Performance Optimization

**Tasks**:
- Optimize JSON parsing (use simd-json if beneficial)
- Minimize latency for small requests
- Handle large file operations efficiently (streaming)
- Profile and optimize hot paths
- Use zero-copy where possible
- Implement connection pooling if needed
- Cache frequently accessed data
- Benchmark against other ACP implementations

#### 8.3 Security Hardening

**Tasks**:
- Validate all inputs thoroughly
- Prevent path traversal attacks
- Sanitize file paths
- Enforce workspace boundaries
- Secure permission system (defense in depth)
- Implement rate limiting if needed
- Audit logging for security events
- Follow principle of least privilege
- Regular security reviews

## Implementation Timeline

### Week 1-2: Foundation
- Phase 1: Foundation & Setup (1.1, 1.2, 1.3)
- Basic module structure
- CLI integration

### Week 3-4: Core Protocol
- Phase 2: Core ACP Implementation (2.1, 2.2, 2.3)
- Agent trait implementation
- Stdio transport
- JSON-RPC handling

### Week 5-6: File System & Permissions
- Phase 3: File System Integration (3.1, 3.2, 3.3)
- File operations
- Permission system
- Workspace management

### Week 7-8: Agent Operations
- Phase 4: Agent Operations (4.1, 4.2, 4.3)
- Prompt handling
- Tool execution
- Context management

### Week 9-10: Advanced Features
- Phase 5: Advanced Features (5.1, 5.2, 5.3, 5.4)
- Capabilities
- Progress tracking
- Edit operations
- Diagnostics

### Week 11-12: Testing & Integration
- Phase 6: Integration & Testing (6.1, 6.2, 6.3)
- Component integration
- State management
- Comprehensive testing

### Week 13: Documentation
- Phase 7: Documentation & Examples (7.1, 7.2, 7.3)
- Write documentation
- Create examples
- Configuration

### Week 14: Polish
- Phase 8: Polish & Deployment (8.1, 8.2, 8.3)
- Error handling
- Performance optimization
- Security hardening

## Architecture Diagram

```
┌─────────────────────────────────────────────────┐
│                    Editor                       │
│              (Zed, Neovim, etc.)               │
└─────────────────┬───────────────────────────────┘
                  │ JSON-RPC 2.0
                  │ over stdio
                  │
┌─────────────────▼───────────────────────────────┐
│           Flexorama (ACP Mode)                  │
│                                                 │
│  ┌─────────────────────────────────────────┐   │
│  │      ACP Server (stdio transport)       │   │
│  └──────────────┬──────────────────────────┘   │
│                 │                               │
│  ┌──────────────▼──────────────────────────┐   │
│  │        FlexoramaAgent Handler           │   │
│  │  (implements agent-client-protocol)     │   │
│  └──────────────┬──────────────────────────┘   │
│                 │                               │
│  ┌──────────────▼──────────────────────────┐   │
│  │         Flexorama Agent                 │   │
│  │    (existing agent logic)               │   │
│  └──────────────┬──────────────────────────┘   │
│                 │                               │
│  ┌──────────────▼──────────────────────────┐   │
│  │          Tool System                    │   │
│  │  - File operations (with permissions)   │   │
│  │  - Bash execution                       │   │
│  │  - MCP tools                            │   │
│  │  - Custom tools                         │   │
│  └─────────────────────────────────────────┘   │
│                                                 │
└─────────────────────────────────────────────────┘
```

## Key Design Decisions

### 1. Reuse Existing Agent Logic
- **Decision**: Wrap existing `Agent` struct rather than reimplement
- **Rationale**: Maintain consistency, reduce code duplication, leverage existing features
- **Impact**: Lower implementation effort, easier maintenance

### 2. Stdio-Only Transport Initially
- **Decision**: Start with stdio transport, add TCP later if needed
- **Rationale**: ACP spec primarily uses stdio, simpler to implement
- **Impact**: Editors must support stdio-based agents

### 3. Permission Integration
- **Decision**: Reuse existing `FileSecurityManager` and `BashSecurityManager`
- **Rationale**: Consistent security model across all modes
- **Impact**: ACP permissions work like CLI permissions

### 4. Separate Module
- **Decision**: Implement ACP in its own module (`src/acp/`)
- **Rationale**: Clear separation of concerns, optional feature
- **Impact**: Easy to maintain and test independently

### 5. Database Support
- **Decision**: Use existing database for conversation persistence
- **Rationale**: Enables conversation history across sessions
- **Impact**: Users can resume conversations after editor restart

## Success Criteria

✅ **Functional Requirements**:
1. Flexorama can start in ACP mode via `--acp` flag
2. Editors can initialize and negotiate capabilities
3. Basic prompts work end-to-end with responses
4. File read/write operations work with permission system
5. Multi-turn conversations are supported
6. Tool execution works through ACP
7. Integration with at least one editor (Zed or Neovim)
8. Error handling is robust and informative

✅ **Non-Functional Requirements**:
1. Low latency (<100ms for small requests)
2. Stable operation (no crashes)
3. Clear error messages
4. Complete documentation
5. Security: no path traversal vulnerabilities
6. Good test coverage (>80%)

## Risks & Mitigation

| Risk | Impact | Mitigation |
|------|--------|-----------|
| ACP spec changes | High | Pin crate version, monitor changes |
| Stdio conflicts with tool output | High | Use stderr for logging, careful output handling |
| Permission UX in headless mode | Medium | Provide clear prompts, support config-based permissions |
| Performance issues with large files | Medium | Implement streaming, chunking |
| Editor compatibility issues | Medium | Test with multiple editors, document limitations |
| Security vulnerabilities | High | Thorough testing, security review, principle of least privilege |

## Future Enhancements

- **TCP Transport**: Support network-based ACP connections
- **Multiple Sessions**: Support multiple concurrent ACP sessions
- **Advanced Streaming**: Stream tool output in real-time
- **Code Intelligence**: Integrate with LSP for better code understanding
- **Workspace Symbols**: Support workspace-wide symbol search
- **Diagnostics Integration**: Send compiler errors/warnings to editor
- **Test Integration**: Run tests from editor, report results
- **Debugger Integration**: Support debugging through ACP

## References

- [Agent Client Protocol Specification](https://agentclientprotocol.com/)
- [agent-client-protocol Rust Crate](https://crates.io/crates/agent-client-protocol)
- [agent_client_protocol Rust Docs](https://docs.rs/agent-client-protocol/latest/agent_client_protocol/)
- [ACP GitHub Repository](https://github.com/agentclientprotocol/agent-client-protocol)
- [JetBrains ACP Documentation](https://www.jetbrains.com/help/ai-assistant/acp.html)
- [Intro to ACP by Block/Goose](https://block.github.io/goose/blog/2025/10/24/intro-to-agent-client-protocol-acp/)
- [JSON-RPC 2.0 Specification](https://www.jsonrpc.org/specification)
- [Language Server Protocol](https://microsoft.github.io/language-server-protocol/) (for inspiration)

---

**Document Version**: 1.0
**Last Updated**: 2026-01-20
**Status**: Draft - Ready for Implementation
