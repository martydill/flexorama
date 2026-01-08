# MCP (Model Context Protocol) Support

This document describes the MCP (Model Context Protocol) support added to the Flexorama tool.

## Overview

MCP allows the Flexorama to connect to external servers that provide additional tools and capabilities. This enables the agent to interact with external services, databases, APIs, and more through a standardized protocol.

## Features

### Connection Types

1. **Stdio Servers**: Connect to MCP servers via standard input/output
2. **WebSocket Servers**: Connect to MCP servers via WebSocket connections
3. **Auto-discovery**: Automatically load tools from connected servers

### Server Management

- Add/remove MCP servers
- Connect/disconnect from servers
- List available servers and their status
- View available tools from connected servers

### Tool Integration

- MCP tools are automatically integrated into the agent's tool ecosystem
- Tools are prefixed with `mcp_<server>_<toolname>` for identification
- Real-time tool refresh when servers connect/disconnect

## Usage

### Interactive Commands

```bash
# List all MCP servers
/mcp list

# Add a stdio server
/mcp add filesystem stdio npx -y @modelcontextprotocol/server-filesystem /path/to/directory

# Add a WebSocket server
/mcp add websocket ws://localhost:8080

# Connect to a server
/mcp connect filesystem

# Disconnect from a server
/mcp disconnect filesystem

# List available tools
/mcp tools

# Remove a server
/mcp remove filesystem

# Connect to all enabled servers
/mcp connect-all

# Disconnect from all servers
/mcp disconnect-all
```

### Configuration

MCP servers are configured in `~/.config/flexorama/mcp.toml`:

```toml
[servers.filesystem]
name = "filesystem"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/path/to/directory"]
enabled = true

[servers.websocket]
name = "websocket"
url = "ws://localhost:8080"
enabled = true

[servers.disabled_server]
name = "disabled_server"
command = "some-command"
enabled = false
```

## MCP Server Examples

### Filesystem Server

```bash
# Add filesystem server
/mcp add fs stdio npx -y @modelcontextprotocol/server-filesystem /home/user/documents

# Use it
"List the files in /home/user/documents using the filesystem tools"
```

### Database Server

```bash
# Add database server
/mcp add db stdio python db_server.py --connection-string postgresql://user:pass@localhost/db

# Use it
"Query the users table and show me the first 10 records"
```

### Web API Server

```bash
# Add API server
/mcp add api ws://api-server.example.com/mcp

# Use it
"Fetch the latest weather data from the weather API"
```

## Implementation Details

### Architecture

1. **McpManager**: Manages server connections and tool discovery
2. **McpConnection**: Handles individual server connections
3. **Tool Integration**: MCP tools are wrapped as standard tools
4. **Configuration**: TOML-based server configuration

### Protocol Support

- **JSON-RPC 2.0**: Base protocol for communication
- **Stdio Transport**: Communication via standard input/output
- **WebSocket Transport**: Communication via WebSocket connections
- **Tool Discovery**: Automatic tool listing and schema retrieval

### Error Handling

- Connection failures are logged but don't crash the agent
- Tool execution errors are properly handled and reported
- Server disconnection gracefully handles in-flight requests

## Security Considerations

1. **Server Validation**: Only connect to trusted MCP servers
2. **Command Execution**: Stdio servers execute commands - ensure they're safe
3. **Network Connections**: WebSocket servers connect to remote endpoints
4. **Tool Permissions**: MCP tools have access to external resources

## Troubleshooting

### Common Issues

1. **Server Won't Connect**: Check command/URL and server availability
2. **Tools Not Loading**: Verify server implements MCP protocol correctly
3. **Permission Errors**: Ensure server has necessary permissions

### Debug Logging

Enable debug logging to see MCP communication:

```bash
RUST_LOG=debug flexorama
```

### Server Status

Use `/mcp list` to see server connection status and available tools.

## Future Enhancements

1. **Authentication**: Support for authenticated MCP servers
2. **Load Balancing**: Multiple server instances for high availability
3. **Caching**: Tool result caching for performance
4. **Monitoring**: Server health monitoring and metrics
5. **Security**: Sandboxing and permission management