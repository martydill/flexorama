use anyhow::Result;
use futures_util::SinkExt;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::process::{Child, Command as TokioCommand};
use tokio::sync::{Mutex, RwLock};
use tokio_tungstenite::{connect_async, tungstenite::Message};

// Re-export from config module to maintain compatibility
pub use crate::config::{McpConfig, McpServerConfig};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method")]
pub struct McpRequest {
    pub jsonrpc: String,
    pub id: Option<String>,
    #[serde(flatten)]
    pub method: McpMethod,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", content = "params")]
pub enum McpMethod {
    #[serde(rename = "initialize")]
    Initialize {
        protocol_version: String,
        capabilities: McpClientCapabilities,
        client_info: McpClientInfo,
    },
    #[serde(rename = "tools/list")]
    ListTools,
    #[serde(rename = "tools/call")]
    CallTool {
        name: String,
        arguments: Option<Value>,
    },
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "notifications/initialized")]
    Initialized,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpClientCapabilities {
    pub tools: Option<McpToolsCapability>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolsCapability {
    pub list_changed: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpResponse {
    pub jsonrpc: String,
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<McpError>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpError {
    pub code: i32,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpTool {
    pub name: String,
    pub description: Option<String>,
    #[serde(alias = "inputSchema", rename = "input_schema")]
    pub input_schema: Value,
}

#[derive(Debug)]
pub struct McpConnection {
    pub name: String,
    pub process: Option<Child>,
    pub websocket:
        Option<tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<TcpStream>>>,
    pub reader: Option<BufReader<tokio::process::ChildStdout>>,
    pub writer: Option<tokio::process::ChildStdin>,
    pub request_id: u64,
    pub pending_requests: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<McpResponse>>>>,
    pub tools: Arc<RwLock<Vec<McpTool>>>,
    pub tools_version: Arc<RwLock<u64>>,
}

impl McpConnection {
    pub fn new(name: String) -> Self {
        Self {
            name,
            process: None,
            websocket: None,
            reader: None,
            writer: None,
            request_id: 1,
            pending_requests: Arc::new(Mutex::new(HashMap::new())),
            tools: Arc::new(RwLock::new(Vec::new())),
            tools_version: Arc::new(RwLock::new(0)),
        }
    }

    /// Log detailed information about a tool and its parameters
    fn log_tool_details(&self, tool: &McpTool) {
        info!("📋 Tool Details:");
        info!("   Name: {}", tool.name);

        if let Some(description) = &tool.description {
            info!("   Description: {}", description);
        } else {
            info!("   Description: <No description provided>");
        }

        // Log input schema details
        if let Some(schema_obj) = tool.input_schema.as_object() {
            if let Some(properties) = schema_obj.get("properties").and_then(|p| p.as_object()) {
                if properties.is_empty() {
                    info!("   Parameters: <No parameters>");
                } else {
                    info!("   Parameters ({}):", properties.len());
                    for (param_name, param_schema) in properties {
                        let param_type = param_schema
                            .get("type")
                            .and_then(|t| t.as_str())
                            .unwrap_or("unknown");
                        let param_desc = param_schema
                            .get("description")
                            .and_then(|d| d.as_str())
                            .unwrap_or("<No description>");
                        let required = schema_obj
                            .get("required")
                            .and_then(|r| r.as_array())
                            .map(|reqs| reqs.iter().any(|req| req.as_str() == Some(param_name)))
                            .unwrap_or(false);

                        let required_marker = if required { " (required)" } else { "" };
                        info!(
                            "     • {} [{}]{}: {}",
                            param_name, param_type, required_marker, param_desc
                        );
                    }
                }
            } else {
                info!("   Parameters: <No parameters defined>");
            }
        } else {
            info!("   Parameters: <Invalid schema>");
        }

        info!(
            "   Raw Schema: {}",
            serde_json::to_string(&tool.input_schema)
                .unwrap_or_else(|_| "<Invalid JSON>".to_string())
        );
    }

    pub async fn connect_stdio(
        &mut self,
        command: &str,
        args: &[String],
        env: &HashMap<String, String>,
    ) -> Result<()> {
        info!("Starting MCP server: {} {}", command, args.join(" "));
        debug!("MCP server details:");
        debug!("  Command: {}", command);
        debug!("  Args: {:?}", args);
        debug!("  Environment variables: {}", env.len());

        // Handle Windows-specific command resolution
        let (cmd, cmd_args) = if cfg!(target_os = "windows") {
            // On Windows, try to resolve the command properly
            if command == "npx" {
                // Try to find npx in common locations
                let npx_path = self.find_npx_on_windows().await?;
                (npx_path, args.to_vec())
            } else {
                // For other commands, try to find them in PATH
                match which::which(command) {
                    Ok(path) => (path.to_string_lossy().to_string(), args.to_vec()),
                    Err(_) => (command.to_string(), args.to_vec()),
                }
            }
        } else {
            (command.to_string(), args.to_vec())
        };

        let mut cmd_process = TokioCommand::new(&cmd);
        cmd_process
            .args(&cmd_args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        for (key, value) in env {
            cmd_process.env(key, value);
        }

        // Add more detailed error logging for debugging
        debug!("Executing command: {} with args: {:?}", cmd, cmd_args);

        let mut child = cmd_process.spawn()
            .map_err(|e| anyhow::anyhow!("Failed to spawn MCP server process '{}': {}\nPlease ensure:\n1. The command exists and is executable\n2. All required dependencies are installed\n3. The command is in your PATH\n4. On Windows: Node.js and npm are properly installed", cmd, e))?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to get stdin from child process"))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| anyhow::anyhow!("Failed to get stdout from child process"))?;

        self.reader = Some(BufReader::new(stdout));
        self.writer = Some(stdin);
        self.process = Some(child);

        // Start message handling loop
        let pending_requests = self.pending_requests.clone();
        let tools = self.tools.clone();
        let tools_version = self.tools_version.clone();
        let name = self.name.clone();
        let mut reader = self.reader.take().unwrap();

        tokio::spawn(async move {
            let mut buffer = String::new();
            loop {
                match reader.read_line(&mut buffer).await {
                    Ok(0) => {
                        debug!("MCP server {} closed connection", name);
                        break;
                    }
                    Ok(_) => {
                        if buffer.trim().is_empty() {
                            buffer.clear();
                            continue;
                        }

                        debug!("Received from MCP server {}: {}", name, buffer.trim());

                        match serde_json::from_str::<McpResponse>(&buffer.trim()) {
                            Ok(response) => {
                                if let Some(id) = &response.id {
                                    let mut pending = pending_requests.lock().await;
                                    if let Some(sender) = pending.remove(id) {
                                        let _ = sender.send(response);
                                    }
                                } else if let Some(result) = &response.result {
                                    // Handle notifications
                                    if let Some(tools_list) = result.get("tools") {
                                        debug!(
                                            "Received tools via notification from {}: {}",
                                            name,
                                            serde_json::to_string_pretty(tools_list)
                                                .unwrap_or_else(|_| "Invalid JSON".to_string())
                                        );

                                        // Try to parse tools with better error handling
                                        match serde_json::from_value::<Vec<Value>>(
                                            tools_list.clone(),
                                        ) {
                                            Ok(raw_tools) => {
                                                let mut parsed_tools = Vec::new();
                                                for (i, raw_tool) in
                                                    raw_tools.into_iter().enumerate()
                                                {
                                                    match serde_json::from_value::<McpTool>(
                                                        raw_tool.clone(),
                                                    ) {
                                                        Ok(tool) => {
                                                            debug!("Successfully parsed tool: {} from {}", tool.name, name);
                                                            parsed_tools.push(tool);
                                                        }
                                                        Err(e) => {
                                                            warn!("Failed to parse tool {} from server '{}' (index: {}): {}. Tool data: {}", 
                                                                  i, name, i, e, serde_json::to_string_pretty(&raw_tool).unwrap_or_else(|_| "Invalid JSON".to_string()));

                                                            // Try to create a minimal tool with the available data
                                                            if let Some(tool_name) = raw_tool
                                                                .get("name")
                                                                .and_then(|v| v.as_str())
                                                            {
                                                                let fallback_tool = McpTool {
                                                                    name: tool_name.to_string(),
                                                                    description: raw_tool
                                                                        .get("description")
                                                                        .and_then(|v| v.as_str())
                                                                        .map(|s| s.to_string()),
                                                                    input_schema: json!({
                                                                        "type": "object",
                                                                        "properties": {},
                                                                        "required": []
                                                                    }),
                                                                };
                                                                warn!("⚠️  Created fallback tool '{}' with default schema (original tool had null/invalid schema)", tool_name);
                                                                parsed_tools.push(fallback_tool);
                                                            }
                                                        }
                                                    }
                                                }
                                                *tools.write().await = parsed_tools;
                                                // Increment version for this connection
                                                let mut version = tools_version.write().await;
                                                *version += 1;
                                                info!("Updated {} tools from MCP server {} via notification", tools.read().await.len(), name);
                                            }
                                            Err(e) => {
                                                warn!("Failed to parse tools array from notification {}: {}. Raw response: {}", name, e, serde_json::to_string_pretty(tools_list).unwrap_or_else(|_| "Invalid JSON".to_string()));
                                            }
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                warn!(
                                    "Failed to parse MCP response from {}: {}. Response: {}",
                                    name,
                                    e,
                                    buffer.trim()
                                );
                                error!(
                                    "MCP server '{}' sent invalid JSON data. This may indicate:",
                                    name
                                );
                                error!("1. Server is not following MCP protocol correctly");
                                error!(
                                    "2. Server process is crashing or outputting error messages"
                                );
                                error!("3. Version mismatch between client and server");
                                debug!("Raw response that failed to parse: {}", buffer.trim());
                            }
                        }
                        buffer.clear();
                    }
                    Err(e) => {
                        error!("Error reading from MCP server {}: {}", name, e);
                        error!(
                            "MCP server {} connection broken - tools may be unavailable",
                            name
                        );
                        break;
                    }
                }
            }
        });

        // Initialize connection
        match self.initialize().await {
            Ok(_) => {
                info!(
                    "MCP server '{}' initialization completed successfully",
                    self.name
                );

                // Verify the process is still running after initialization
                if let Some(ref mut process) = self.process {
                    match process.try_wait() {
                        Ok(Some(status)) => {
                            error!(
                                "MCP server '{}' process exited unexpectedly with status: {}",
                                self.name, status
                            );
                            return Err(anyhow::anyhow!(
                                "MCP server '{}' process exited during initialization",
                                self.name
                            ));
                        }
                        Ok(None) => {
                            debug!("MCP server '{}' process is running normally", self.name);
                        }
                        Err(e) => {
                            warn!("Failed to check MCP server '{}' status: {}", self.name, e);
                        }
                    }
                }
                Ok(())
            }
            Err(e) => {
                error!("MCP server '{}' initialization failed: {}", self.name, e);

                // Check if the process is still running
                if let Some(ref mut process) = self.process {
                    match process.try_wait() {
                        Ok(Some(status)) => {
                            error!(
                                "MCP server '{}' process exited with status: {}",
                                self.name, status
                            );
                        }
                        Ok(None) => {
                            debug!("MCP server '{}' process is still running but initialization failed", self.name);
                        }
                        Err(_) => {}
                    }
                }

                Err(e)
            }
        }
    }

    /// Get the current tools version
    pub async fn get_tools_version(&self) -> u64 {
        *self.tools_version.read().await
    }

    /// Find npx executable on Windows
    async fn find_npx_on_windows(&self) -> Result<String> {
        // Try common Node.js installation paths on Windows
        let common_paths = vec![
            r"C:\Program Files\nodejs\npx.cmd",
            r"C:\Program Files (x86)\nodejs\npx.cmd",
            r"%APPDATA%\npm\npx.cmd",
        ];

        // First try to find npx in PATH
        if let Ok(npx_path) = which::which("npx") {
            return Ok(npx_path.to_string_lossy().to_string());
        }

        // Try common installation paths
        for path in &common_paths {
            let expanded_path = env::var("APPDATA").unwrap_or_default();
            let full_path = path.replace("%APPDATA%", &expanded_path);

            if Path::new(&full_path).exists() {
                info!("Found npx at: {}", full_path);
                return Ok(full_path);
            }
        }

        // Try to find Node.js and use npx from there
        if let Ok(node_path) = which::which("node") {
            if let Some(parent) = Path::new(&node_path).parent() {
                let npx_path = parent.join("npx.cmd");
                if npx_path.exists() {
                    info!("Found npx at: {}", npx_path.display());
                    return Ok(npx_path.to_string_lossy().to_string());
                }
            }
        }

        Err(anyhow::anyhow!(
            "npx not found. Please install Node.js and npm from https://nodejs.org/"
        ))
    }

    pub async fn connect_websocket(&mut self, url: &str) -> Result<()> {
        info!("Connecting to MCP server via WebSocket: {}", url);

        let (ws_stream, _) = connect_async(url).await?;
        self.websocket = Some(ws_stream);

        // Initialize connection
        self.initialize().await?;
        Ok(())
    }

    async fn initialize(&mut self) -> Result<()> {
        let init_request = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(self.next_id()),
            method: McpMethod::Initialize {
                protocol_version: "2024-11-05".to_string(),
                capabilities: McpClientCapabilities {
                    tools: Some(McpToolsCapability {
                        list_changed: Some(true),
                    }),
                },
                client_info: McpClientInfo {
                    name: "aixplosion".to_string(),
                    version: "0.1.0".to_string(),
                },
            },
        };

        let response = self.send_request(init_request).await?;

        if response.error.is_some() {
            return Err(anyhow::anyhow!(
                "MCP initialization failed: {:?}",
                response.error
            ));
        }

        // Send initialized notification
        let initialized = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: None,
            method: McpMethod::Initialized,
        };

        self.send_notification(initialized).await?;

        // Load tools
        self.load_tools().await?;

        info!("MCP server {} initialized successfully", self.name);
        Ok(())
    }

    async fn load_tools(&mut self) -> Result<()> {
        info!("Loading tools from MCP server '{}'...", self.name);

        let tools_request = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(self.next_id()),
            method: McpMethod::ListTools,
        };

        // Add timeout for tools loading to prevent hanging
        let response_result = tokio::time::timeout(
            std::time::Duration::from_secs(10), // 10 second timeout for tools loading
            self.send_request(tools_request),
        )
        .await;

        let response = match response_result {
            Ok(Ok(response)) => response,
            Ok(Err(e)) => {
                error!(
                    "Failed to send tools request to MCP server '{}': {}",
                    self.name, e
                );
                return Err(e);
            }
            Err(_) => {
                error!(
                    "MCP server '{}' tools loading timed out after 10 seconds",
                    self.name
                );
                return Err(anyhow::anyhow!(
                    "Tools loading timed out for MCP server '{}'",
                    self.name
                ));
            }
        };

        if let Some(error) = response.error {
            error!(
                "Failed to list tools from MCP server '{}': {:?}",
                self.name, error
            );
            return Err(anyhow::anyhow!("Failed to list tools: {:?}", error));
        }

        if let Some(result) = response.result {
            if let Some(tools_value) = result.get("tools") {
                debug!(
                    "Raw tools response from {}: {}",
                    self.name,
                    serde_json::to_string_pretty(tools_value)?
                );

                // Try to parse tools with better error handling
                match serde_json::from_value::<Vec<Value>>(tools_value.clone()) {
                    Ok(raw_tools) => {
                        info!(
                            "MCP server '{}' returned {} tools",
                            self.name,
                            raw_tools.len()
                        );

                        let mut parsed_tools = Vec::new();
                        for (i, raw_tool) in raw_tools.into_iter().enumerate() {
                            // Log the raw tool data before parsing
                            debug!(
                                "Raw tool data {} from server '{}': {}",
                                i,
                                self.name,
                                serde_json::to_string_pretty(&raw_tool)
                                    .unwrap_or_else(|_| "Invalid JSON".to_string())
                            );

                            // Check if inputSchema exists and what its value is
                            if let Some(input_schema) = raw_tool.get("inputSchema") {
                                debug!(
                                    "  inputSchema field found: {}",
                                    serde_json::to_string_pretty(input_schema)
                                        .unwrap_or_else(|_| "Invalid JSON".to_string())
                                );
                                if input_schema.is_null() {
                                    debug!("  inputSchema is null - will need fallback");
                                } else {
                                    debug!("  inputSchema has valid data");
                                }
                            } else if let Some(input_schema) = raw_tool.get("input_schema") {
                                debug!(
                                    "  input_schema field found (snake_case): {}",
                                    serde_json::to_string_pretty(input_schema)
                                        .unwrap_or_else(|_| "Invalid JSON".to_string())
                                );
                                if input_schema.is_null() {
                                    debug!("  input_schema is null - will need fallback");
                                } else {
                                    debug!("  input_schema has valid data");
                                }
                            } else {
                                debug!("  No input schema field found - will use serde default");
                            }

                            match serde_json::from_value::<McpTool>(raw_tool.clone()) {
                                Ok(tool) => {
                                    debug!(
                                        "Successfully parsed tool: {} from {}",
                                        tool.name, self.name
                                    );
                                    debug!(
                                        "  Parsed schema: {}",
                                        serde_json::to_string(&tool.input_schema)
                                            .unwrap_or_else(|_| "Invalid JSON".to_string())
                                    );
                                    info!(
                                        "✓ Loaded tool: {} from server '{}'",
                                        tool.name, self.name
                                    );
                                    self.log_tool_details(&tool);
                                    parsed_tools.push(tool);
                                }
                                Err(e) => {
                                    warn!("Failed to parse tool {} from server '{}' (index: {}): {}. Tool data: {}", 
                                          i, self.name, i, e, serde_json::to_string_pretty(&raw_tool).unwrap_or_else(|_| "Invalid JSON".to_string()));

                                    // Try to create a minimal tool with the available data
                                    if let Some(name) =
                                        raw_tool.get("name").and_then(|v| v.as_str())
                                    {
                                        let fallback_tool = McpTool {
                                            name: name.to_string(),
                                            description: raw_tool
                                                .get("description")
                                                .and_then(|v| v.as_str())
                                                .map(|s| s.to_string()),
                                            input_schema: json!({
                                                "type": "object",
                                                "properties": {},
                                                "required": []
                                            }),
                                        };
                                        warn!("⚠️  Created fallback tool '{}' with default schema (original tool had null/invalid schema)", name);
                                        self.log_tool_details(&fallback_tool);
                                        parsed_tools.push(fallback_tool);
                                    }
                                }
                            }
                        }

                        *self.tools.write().await = parsed_tools;
                        // Increment version for this connection
                        let mut version = self.tools_version.write().await;
                        *version += 1;

                        info!(
                            "✅ Successfully loaded {} tools from MCP server '{}'",
                            self.tools.read().await.len(),
                            self.name
                        );
                    }
                    Err(e) => {
                        warn!(
                            "Failed to parse tools array from {}: {}. Raw response: {}",
                            self.name,
                            e,
                            serde_json::to_string_pretty(tools_value)?
                        );
                        return Err(anyhow::anyhow!(
                            "Invalid tools response format from MCP server '{}': {}",
                            self.name,
                            e
                        ));
                    }
                }
            } else {
                warn!(
                    "No 'tools' field found in response from MCP server '{}'",
                    self.name
                );
            }
        } else {
            warn!(
                "No result found in tools response from MCP server '{}'",
                self.name
            );
        }

        Ok(())
    }

    pub async fn call_tool(&mut self, name: &str, arguments: Option<Value>) -> Result<Value> {
        // Check if connection is still alive
        if let Some(ref mut process) = self.process {
            match process.try_wait() {
                Ok(Some(_)) => {
                    return Err(anyhow::anyhow!(
                        "MCP server '{}' process has terminated",
                        self.name
                    ));
                }
                Ok(None) => {
                    // Process is still running, good
                }
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "Failed to check MCP server '{}' status: {}",
                        self.name,
                        e
                    ));
                }
            }
        }

        // Log the tool call with details
        info!("🔧 Calling MCP tool '{}' on server '{}'", name, self.name);

        if let Some(ref args) = arguments {
            if !args.is_null() {
                info!(
                    "   Arguments: {}",
                    serde_json::to_string_pretty(args)
                        .unwrap_or_else(|_| "<Invalid JSON>".to_string())
                );
            } else {
                info!("   Arguments: <No arguments>");
            }
        } else {
            info!("   Arguments: <No arguments>");
        }

        let tool_request = McpRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(self.next_id()),
            method: McpMethod::CallTool {
                name: name.to_string(),
                arguments,
            },
        };

        let response = self.send_request(tool_request).await?;

        if let Some(error) = response.error {
            error!(
                "❌ MCP tool '{}' failed on server '{}': {:?}",
                name, self.name, error
            );
            return Err(anyhow::anyhow!("Tool call failed: {:?}", error));
        }

        info!(
            "✅ MCP tool '{}' completed successfully on server '{}'",
            name, self.name
        );

        if let Some(ref result) = response.result {
            debug!(
                "   Result: {}",
                serde_json::to_string_pretty(result)
                    .unwrap_or_else(|_| "<Invalid JSON>".to_string())
            );
        }

        Ok(response.result.unwrap_or(json!({})))
    }

    async fn send_request(&mut self, request: McpRequest) -> Result<McpResponse> {
        let id = request.id.clone().unwrap();
        let request_json = serde_json::to_string(&request)?;

        debug!("Sending MCP request to {}: {}", self.name, request_json);

        // Create response channel
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending_requests.lock().await.insert(id.clone(), tx);

        // Send request
        if let Some(writer) = &mut self.writer {
            writer.write_all(request_json.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        } else if let Some(websocket) = &mut self.websocket {
            websocket.send(Message::Text(request_json)).await?;
        } else {
            return Err(anyhow::anyhow!("No connection available"));
        }

        // Wait for response with timeout
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(30), // 30 second timeout
            rx,
        )
        .await
        .map_err(|_| anyhow::anyhow!("MCP server '{}' timed out after 30 seconds", self.name))?
        .map_err(|_| anyhow::anyhow!("MCP server '{}' response channel was dropped", self.name))?;

        Ok(response)
    }

    async fn send_notification(&mut self, notification: McpRequest) -> Result<()> {
        let notification_json = serde_json::to_string(&notification)?;

        debug!(
            "Sending MCP notification to {}: {}",
            self.name, notification_json
        );

        if let Some(writer) = &mut self.writer {
            writer.write_all(notification_json.as_bytes()).await?;
            writer.write_all(b"\n").await?;
            writer.flush().await?;
        } else if let Some(websocket) = &mut self.websocket {
            websocket.send(Message::Text(notification_json)).await?;
        } else {
            return Err(anyhow::anyhow!("No connection available"));
        }

        Ok(())
    }

    pub async fn get_tools(&self) -> Vec<McpTool> {
        self.tools.read().await.clone()
    }

    fn next_id(&mut self) -> String {
        let id = self.request_id.to_string();
        self.request_id += 1;
        id
    }

    pub async fn disconnect(&mut self) -> Result<()> {
        if let Some(mut process) = self.process.take() {
            let _ = process.kill().await;
        }

        if let Some(mut websocket) = self.websocket.take() {
            let _ = websocket.close(None).await;
        }

        info!("Disconnected from MCP server {}", self.name);
        Ok(())
    }
}

#[derive(Debug)]
pub struct McpManager {
    connections: Arc<RwLock<HashMap<String, McpConnection>>>,
    config: Arc<RwLock<McpConfig>>,
    config_path: Option<PathBuf>,
}

impl McpManager {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(RwLock::new(HashMap::new())),
            config: Arc::new(RwLock::new(McpConfig::default())),
            config_path: None,
        }
    }

    /// Initialize with MCP configuration from unified config
    pub async fn initialize(&self, mcp_config: McpConfig) -> Result<()> {
        *self.config.write().await = mcp_config;
        info!(
            "MCP manager initialized with {} servers",
            self.config.read().await.servers.len()
        );
        Ok(())
    }

    /// Save current MCP configuration to unified config file
    pub async fn save_to_config_file(&self) -> Result<()> {
        use crate::config::Config;

        let config_path = self
            .config_path
            .as_ref()
            .map(|path| path.to_string_lossy().to_string());

        // Load existing unified config to preserve other settings
        let mut unified_config = Config::load(config_path.as_deref()).await?;

        // Update MCP configuration
        let current_mcp_config = self.config.read().await.clone();
        unified_config.mcp = current_mcp_config;

        // Save unified config
        unified_config.save(config_path.as_deref()).await?;
        info!("Saved MCP configuration to unified config file");
        Ok(())
    }

    pub async fn load_config(&self) -> Result<McpConfig> {
        Ok(self.config.read().await.clone())
    }

    pub async fn get_server(&self, name: &str) -> Option<McpServerConfig> {
        let config = self.config.read().await;
        config.servers.get(name).cloned()
    }

    pub async fn add_server(&self, name: &str, server_config: McpServerConfig) -> Result<()> {
        let mut config = self.config.write().await;
        config.servers.insert(name.to_string(), server_config);
        drop(config);
        self.save_to_config_file().await?;
        Ok(())
    }

    pub async fn remove_server(&self, name: &str) -> Result<()> {
        let mut config = self.config.write().await;
        config.servers.remove(name);
        drop(config);
        self.save_to_config_file().await?;

        // Disconnect if connected
        let mut connections = self.connections.write().await;
        if let Some(mut connection) = connections.remove(name) {
            let _ = connection.disconnect().await;
        }

        Ok(())
    }

    pub async fn upsert_server(&self, name: &str, server_config: McpServerConfig) -> Result<()> {
        {
            let mut config = self.config.write().await;
            config
                .servers
                .insert(name.to_string(), server_config.clone());
        }
        self.save_to_config_file().await?;

        // Restart the connection based on the new config
        let _ = self.disconnect_server(name).await;
        if server_config.enabled {
            let _ = self.connect_server(name).await?;
        }

        Ok(())
    }

    pub async fn connect_server(&self, name: &str) -> Result<()> {
        let config = self.load_config().await?;
        let server_config = config
            .servers
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("Server '{}' not found in configuration", name))?;

        if !server_config.enabled {
            return Err(anyhow::anyhow!("Server '{}' is disabled", name));
        }

        info!("🔌 Connecting to MCP server: {}", name);
        info!("   Configuration:");

        if let Some(url) = &server_config.url {
            info!("     Type: WebSocket");
            info!("     URL: {}", url);
        } else if let Some(command) = &server_config.command {
            info!("     Type: STDIO");
            info!("     Command: {}", command);
            if let Some(args) = &server_config.args {
                info!("     Args: {}", args.join(" "));
            }
            if let Some(env_vars) = &server_config.env {
                info!("     Environment Variables: {}", env_vars.len());
                for (key, value) in env_vars {
                    info!("       {}={}", key, value);
                }
            }
        }
        info!("     Enabled: {}", server_config.enabled);

        let mut connection = McpConnection::new(name.to_string());

        if let Some(url) = &server_config.url {
            // Connect via WebSocket
            connection.connect_websocket(url).await?;
        } else if let Some(command) = &server_config.command {
            // Connect via stdio
            let args = server_config.args.as_deref().unwrap_or(&[]);
            let env_vars = server_config.env.as_ref().cloned().unwrap_or_default();
            connection.connect_stdio(command, args, &env_vars).await?;
        } else {
            return Err(anyhow::anyhow!(
                "Server '{}' has no command or URL configured",
                name
            ));
        }

        self.connections
            .write()
            .await
            .insert(name.to_string(), connection);
        info!("✅ Successfully connected to MCP server: {}", name);

        // Log summary of available tools
        if let Some(connection) = self.connections.read().await.get(name) {
            let tools = connection.get_tools().await;
            info!("📊 Available tools from '{}': {}", name, tools.len());
            for tool in &tools {
                info!(
                    "   • {} - {}",
                    tool.name,
                    tool.description.as_deref().unwrap_or("<No description>")
                );
            }
        }

        Ok(())
    }

    pub async fn disconnect_server(&self, name: &str) -> Result<()> {
        let mut connections = self.connections.write().await;
        if let Some(mut connection) = connections.remove(name) {
            connection.disconnect().await?;
            info!("Disconnected from MCP server: {}", name);
        }
        Ok(())
    }

    pub async fn reconnect_server(&self, name: &str) -> Result<()> {
        self.disconnect_server(name).await?;
        self.connect_server(name).await?;
        Ok(())
    }

    pub async fn list_servers(&self) -> Result<Vec<(String, McpServerConfig, bool)>> {
        let config = self.config.read().await;
        let connections = self.connections.read().await;

        let mut servers = Vec::new();
        for (name, server_config) in config.servers.iter() {
            let connected = connections.contains_key(name);
            servers.push((name.clone(), server_config.clone(), connected));
        }

        Ok(servers)
    }

    pub async fn get_all_tools(&self) -> Result<Vec<(String, McpTool)>> {
        let connections = self.connections.read().await;
        let mut all_tools = Vec::new();

        for (name, connection) in connections.iter() {
            let tools = connection.get_tools().await;
            for tool in tools {
                all_tools.push((name.clone(), tool));
            }
        }

        Ok(all_tools)
    }

    pub async fn disconnect_all(&self) -> Result<()> {
        let connections = self.connections.read().await;
        let server_names: Vec<String> = connections.keys().cloned().collect();
        drop(connections);

        for name in server_names {
            let _ = self.disconnect_server(&name).await;
        }

        Ok(())
    }

    pub async fn is_connected(&self, name: &str) -> bool {
        let connections = self.connections.read().await;
        connections.contains_key(name)
    }

    /// Get the global tools version (sum of all connection versions)
    pub async fn get_tools_version(&self) -> u64 {
        let connections = self.connections.read().await;
        let mut total_version = 0u64;

        for connection in connections.values() {
            total_version = total_version.wrapping_add(connection.get_tools_version().await);
        }

        total_version
    }

    pub async fn call_tool(
        &self,
        server_name: &str,
        tool_name: &str,
        arguments: Option<Value>,
    ) -> Result<Value> {
        let mut connections = self.connections.write().await;
        if let Some(connection) = connections.get_mut(server_name) {
            connection.call_tool(tool_name, arguments).await
        } else {
            Err(anyhow::anyhow!("Server '{}' is not connected", server_name))
        }
    }

    pub async fn connect_all_enabled(&self) -> Result<()> {
        let config = self.config.read().await;
        let mut connected_count = 0;
        let mut failed_count = 0;

        info!("🌐 Connecting to all enabled MCP servers...");
        info!("   Total servers configured: {}", config.servers.len());

        for (name, server_config) in config.servers.iter() {
            if server_config.enabled {
                info!("   Attempting to connect to: {}", name);

                // Add timeout for individual server connections
                let connect_result = tokio::time::timeout(
                    std::time::Duration::from_secs(10), // 10 second timeout per server
                    self.connect_server(name),
                )
                .await;

                match connect_result {
                    Ok(Ok(_)) => {
                        connected_count += 1;
                        info!("✅ Connected to MCP server: {}", name);
                    }
                    Ok(Err(e)) => {
                        failed_count += 1;
                        warn!("❌ Failed to connect to MCP server '{}': {}", name, e);
                    }
                    Err(_) => {
                        failed_count += 1;
                        warn!(
                            "⏰ MCP server '{}' connection timed out after 10 seconds",
                            name
                        );
                    }
                }
            } else {
                info!("⏭️  Skipping disabled server: {}", name);
            }
        }

        info!("📊 MCP Connection Summary:");
        info!("   Successfully connected: {}", connected_count);
        info!("   Failed connections: {}", failed_count);
        info!(
            "   Skipped (disabled): {}",
            config.servers.len() - connected_count - failed_count
        );

        if connected_count > 0 {
            // Log total tools available across all servers
            let all_tools = self.get_all_tools().await?;
            info!(
                "🛠️  Total tools available across all MCP servers: {}",
                all_tools.len()
            );

            // Group tools by server for better organization
            let mut tools_by_server = std::collections::HashMap::new();
            for (server_name, tool) in all_tools {
                tools_by_server
                    .entry(server_name)
                    .or_insert_with(Vec::new)
                    .push(tool);
            }

            for (server_name, tools) in tools_by_server {
                info!("   Server '{}': {} tools", server_name, tools.len());
            }
        }

        Ok(())
    }
}

impl Default for McpManager {
    fn default() -> Self {
        Self::new()
    }
}
