# MCP (Model Context Protocol) Support

This document describes the MCP (Model Context Protocol) support added to the Flexorama tool.

## Overview

MCP allows the Flexorama to connect to external servers that provide additional tools and capabilities. This enables the agent to interact with external services, databases, APIs, and more through a standardized protocol.

## Features

### Connection Types

1. **Stdio Servers**: Connect to MCP servers via standard input/output
2. **WebSocket Servers**: Connect to MCP servers via WebSocket connections
3. **HTTP Servers**: Connect to MCP servers over HTTP (POST and optional SSE)
4. **Auto-discovery**: Automatically load tools from connected servers

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

# Add an HTTP server
/mcp add linear http https://mcp.linear.app/mcp

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

[servers.linear]
name = "linear"
url = "https://mcp.linear.app/mcp"
enabled = true

[servers.disabled_server]
name = "disabled_server"
command = "some-command"
enabled = false
```

### OAuth Authentication

Flexorama supports two OAuth flows for authenticating with MCP servers:

#### Authorization Code Flow with PKCE (Default)

For MCP servers that require user authorization (like Linear, GitHub, etc.), use the
`authorization_code` grant type. This is the default and recommended flow for user-facing OAuth.

When you connect to a server requiring OAuth:
1. Flexorama starts a local callback server
2. Opens your browser to the authorization URL with PKCE parameters
3. After you authorize, the callback receives the code
4. Flexorama exchanges the code for an access token

```toml
[servers.linear]
name = "linear"
url = "https://mcp.linear.app/mcp"
enabled = true

[servers.linear.auth]
type = "oauth"
grant_type = "authorization_code"  # Default, can be omitted
authorization_url = "https://linear.app/oauth/authorize"
token_url = "https://api.linear.app/oauth/token"  # Optional, derived from server URL if omitted
client_id = "your-client-id"
client_secret = "your-client-secret"  # Optional for public clients using PKCE
scope = "read write"
```

#### Client Credentials Flow

For machine-to-machine authentication without user interaction, use the `client_credentials`
grant type. This requires a client secret.

`client_auth` controls how the client credentials are sent:
- `basic`: HTTP Basic auth header
- `body`: form fields in the token request body (default)

```toml
[servers.secure_api]
name = "secure_api"
url = "wss://api.example.com/mcp"
enabled = true

[servers.secure_api.auth]
type = "oauth"
grant_type = "client_credentials"
token_url = "https://auth.example.com/oauth/token"
client_id = "your-client-id"
client_secret = "your-client-secret"  # Required for client_credentials
scope = "mcp.read mcp.write"
audience = "https://api.example.com"
client_auth = "basic"
```

#### OAuth Configuration Options

| Option | Description | Required |
|--------|-------------|----------|
| `type` | Must be `"oauth"` | Yes |
| `grant_type` | `"authorization_code"` (default) or `"client_credentials"` | No |
| `authorization_url` | URL for user authorization | Yes for authorization_code |
| `token_url` | URL for token exchange | No (derived from server URL) |
| `client_id` | OAuth client ID | Yes |
| `client_secret` | OAuth client secret | Yes for client_credentials, optional for authorization_code with PKCE |
| `scope` | OAuth scopes to request | No |
| `audience` | OAuth audience parameter | No |
| `client_auth` | `"basic"` or `"body"` | No (defaults to body) |
| `extra_params` | Additional parameters to include | No |

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
- **HTTP Transport**: JSON-RPC over HTTP POST, with optional SSE responses
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
5. **OAuth Credentials**: Store client secrets securely and avoid sharing config files

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

1. **Load Balancing**: Multiple server instances for high availability
2. **Caching**: Tool result caching for performance
3. **Monitoring**: Server health monitoring and metrics
4. **Security**: Sandboxing and permission management
5. **Token Refresh**: Automatic refresh of OAuth tokens before expiration
