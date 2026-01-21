use crate::acp::capabilities::{ClientCapabilities};
use crate::acp::errors::{AcpError, AcpResult};
use crate::acp::filesystem::FileSystemHandler;
use crate::acp::session::SessionManager;
use crate::acp::types::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use crate::agent::Agent;
use crate::config::Config;
use agent_client_protocol_schema::{
    AgentCapabilities, Implementation, InitializeResponse, PromptCapabilities,
    McpCapabilities, V1 as PROTOCOL_V1, NewSessionResponse, SessionId,
};
use uuid::Uuid;
use log::{debug, error, info, warn};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Flexorama ACP Handler
/// Implements the Agent Client Protocol server-side logic
pub struct FlexoramaAcpHandler {
    /// The underlying Flexorama agent (for legacy agent/* methods)
    agent: Arc<Mutex<Agent>>,

    /// Session manager for ACP sessions
    session_manager: SessionManager,

    /// Workspace root path
    workspace_root: Option<PathBuf>,

    /// Client capabilities
    client_capabilities: Option<ClientCapabilities>,

    /// Whether the server is initialized
    initialized: bool,

    /// Cancellation flag for operations (for legacy agent/* methods)
    cancellation_flag: Arc<AtomicBool>,

    /// Config for creating new agents if needed
    config: Config,

    /// Model name
    model: String,

    /// Debug mode
    debug: bool,

    /// File system handler
    filesystem: FileSystemHandler,

    /// Yolo mode flag
    yolo_mode: bool,

    /// Plan mode flag
    plan_mode: bool,
}

impl FlexoramaAcpHandler {
    pub fn new(mut agent: Agent, config: Config, model: String, debug: bool) -> Self {
        // Suppress output in ACP mode - stdout must only contain JSON-RPC messages
        agent.set_suppress_output(true);

        let file_security = agent.get_file_security_manager();
        let yolo_mode = agent.yolo_mode();
        let plan_mode = agent.plan_mode();

        let filesystem = FileSystemHandler::new(
            file_security,
            None,  // workspace_root will be set during initialization
            yolo_mode,
        );

        // Create session manager for managing ACP sessions
        let session_manager = SessionManager::new(
            config.clone(),
            model.clone(),
            yolo_mode,
            plan_mode,
        );

        Self {
            agent: Arc::new(Mutex::new(agent)),
            session_manager,
            workspace_root: None,
            client_capabilities: None,
            initialized: false,
            cancellation_flag: Arc::new(AtomicBool::new(false)),
            config,
            model,
            debug,
            filesystem,
            yolo_mode,
            plan_mode,
        }
    }

    /// Handle a JSON-RPC request
    pub async fn handle_request(&mut self, request: JsonRpcRequest) -> JsonRpcResponse {
        debug!("Handling method: {}", request.method);

        let result = match request.method.as_str() {
            "initialize" => self.handle_initialize(request.params).await,
            "initialized" => self.handle_initialized().await,
            "shutdown" => self.handle_shutdown().await,

            // ACP session methods (official protocol)
            "session/new" => self.handle_session_new(request.params).await,
            "session/prompt" => self.handle_session_prompt(request.params).await,
            "session/cancel" => self.handle_session_cancel(request.params).await,

            // Legacy methods for backward compatibility
            "agent/prompt" => self.handle_prompt(request.params).await,
            "agent/cancel" => self.handle_cancel(request.params).await,

            // LSP-style methods (for compatibility)
            "workspace/didChangeConfiguration" => self.handle_configuration_change(request.params).await,
            "textDocument/didOpen" => self.handle_text_document_opened(request.params).await,
            "textDocument/didChange" => self.handle_text_document_changed(request.params).await,
            "textDocument/didClose" => self.handle_text_document_closed(request.params).await,

            // File system operations (custom)
            "fs/readFile" => self.handle_read_file(request.params).await,
            "fs/writeFile" => self.handle_write_file(request.params).await,
            "fs/listDirectory" => self.handle_list_directory(request.params).await,
            "fs/glob" => self.handle_glob(request.params).await,
            "fs/delete" => self.handle_delete(request.params).await,
            "fs/createDirectory" => self.handle_create_directory(request.params).await,

            // Context management (custom)
            "context/addFile" => self.handle_add_context_file(request.params).await,
            "context/clear" => self.handle_clear_context(request.params).await,

            // Editing (custom)
            "edit/applyEdit" => self.handle_apply_edit(request.params).await,

            method => {
                warn!("Unknown method: {}", method);
                Err(AcpError::InvalidRequest(format!("Unknown method: {}", method)))
            }
        };

        match result {
            Ok(value) => JsonRpcResponse::success(request.id, value),
            Err(err) => {
                error!("Error handling {}: {}", request.method, err);
                let rpc_error: JsonRpcError = err.into();
                JsonRpcResponse::error(request.id, rpc_error.code, rpc_error.message, rpc_error.data)
            }
        }
    }

    /// Handle initialize request
    async fn handle_initialize(&mut self, params: Option<Value>) -> AcpResult<Value> {
        info!("Handling initialize request");

        let params = params.ok_or_else(|| AcpError::InvalidRequest("Missing params".to_string()))?;

        // Log the received params for debugging
        if self.debug {
            debug!("Initialize params: {}", serde_json::to_string_pretty(&params).unwrap_or_default());
        }

        // Parse protocol version if present (ACP handshake style)
        if let Some(protocol_version) = params.get("protocolVersion").and_then(|v| v.as_u64()) {
            info!("ACP Protocol Version: {}", protocol_version);
        }

        // Parse client info if present (ACP handshake style)
        if let Some(client_info) = params.get("clientInfo") {
            if let Some(name) = client_info.get("name").and_then(|v| v.as_str()) {
                info!("Client: {}", name);
            }
            if let Some(version) = client_info.get("version").and_then(|v| v.as_str()) {
                info!("Client version: {}", version);
            }
        }

        // Parse workspace root
        if let Some(workspace_root) = params.get("workspaceRoot").and_then(|v| v.as_str()) {
            let root_path = PathBuf::from(workspace_root);
            self.workspace_root = Some(root_path.clone());
            self.filesystem.set_workspace_root(root_path);
            info!("Workspace root: {}", workspace_root);
        } else if let Some(root_uri) = params.get("rootUri").and_then(|v| v.as_str()) {
            // Handle file:// URIs
            if let Some(path) = root_uri.strip_prefix("file://") {
                let root_path = PathBuf::from(path);
                self.workspace_root = Some(root_path.clone());
                self.filesystem.set_workspace_root(root_path);
                info!("Workspace root from URI: {}", path);
            }
        }

        // Parse client capabilities - support both "capabilities" and "clientCapabilities"
        let caps_field = params.get("clientCapabilities").or_else(|| params.get("capabilities"));
        if let Some(caps) = caps_field {
            match serde_json::from_value(caps.clone()) {
                Ok(client_caps) => {
                    self.client_capabilities = Some(client_caps);
                    debug!("Client capabilities parsed");
                }
                Err(e) => {
                    warn!("Failed to parse client capabilities: {}", e);
                }
            }
        }

        self.initialized = true;

        // Build official ACP InitializeResponse
        let agent_capabilities = AgentCapabilities {
            load_session: false,  // We don't support session loading yet
            prompt_capabilities: PromptCapabilities {
                image: false,  // We support text prompts
                audio: false,
                embedded_context: true,  // We support embedded context (file contents, etc.)
                meta: None,
            },
            mcp_capabilities: McpCapabilities::default(),  // Default MCP support
            meta: None,
        };

        let response = InitializeResponse {
            protocol_version: PROTOCOL_V1,  // ACP v1
            agent_capabilities,
            auth_methods: vec![],  // No authentication required
            agent_info: Some(Implementation {
                name: "flexorama".to_string(),
                title: Some("Flexorama".to_string()),
                version: env!("CARGO_PKG_VERSION").to_string(),
            }),
            meta: None,
        };

        Ok(serde_json::to_value(response).unwrap())
    }

    /// Handle initialized notification
    async fn handle_initialized(&mut self) -> AcpResult<Value> {
        debug!("Client confirmed initialization");
        Ok(json!(null))
    }

    /// Handle shutdown request
    async fn handle_shutdown(&mut self) -> AcpResult<Value> {
        info!("Shutting down ACP server");
        self.initialized = false;
        Ok(json!(null))
    }

    /// Handle session/new request (ACP official protocol)
    async fn handle_session_new(&mut self, params: Option<Value>) -> AcpResult<Value> {
        if !self.initialized {
            return Err(AcpError::InvalidRequest(
                "Server not initialized. Call initialize first.".to_string(),
            ));
        }

        info!("Creating new ACP session");

        // Parse params if provided (cwd, mcpServers, etc.)
        let session_workspace = if let Some(params) = params {
            // Extract working directory if provided
            params.get("cwd")
                .and_then(|v| v.as_str())
                .map(|cwd| {
                    info!("Session working directory: {}", cwd);
                    PathBuf::from(cwd)
                })
        } else {
            None
        };

        // Generate a session ID (using UUID v4)
        let session_id = SessionId::from(Uuid::new_v4().to_string());

        // Create a new session with its own agent and conversation
        let session = self.session_manager.create_session(session_id.clone()).await
            .map_err(|e| AcpError::Agent(e))?;

        // If a workspace was specified, update the session's workspace
        if let Some(workspace) = session_workspace {
            // Note: We could set workspace on the session's agent here if needed
            // For now, workspace is set globally in initialize
            info!("Session {} workspace: {}", session_id, workspace.display());
        }

        info!(
            "Created session: {} with conversation ID: {:?}",
            session_id,
            session.conversation_id
        );

        // Build response using official ACP type
        let response = NewSessionResponse {
            session_id,
            modes: None,  // We don't support modes yet
            meta: None,
        };

        Ok(serde_json::to_value(response).unwrap())
    }

    /// Handle session/prompt request (ACP official protocol)
    async fn handle_session_prompt(&mut self, params: Option<Value>) -> AcpResult<Value> {
        if !self.initialized {
            return Err(AcpError::InvalidRequest(
                "Server not initialized".to_string(),
            ));
        }

        let params = params.ok_or_else(|| AcpError::InvalidRequest("Missing params".to_string()))?;

        // Extract session_id (required)
        let session_id = params
            .get("sessionId")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AcpError::InvalidRequest("Missing sessionId".to_string()))?;

        // Extract prompt content blocks
        let prompt_blocks = params
            .get("prompt")
            .ok_or_else(|| AcpError::InvalidRequest("Missing prompt".to_string()))?;

        // Convert ContentBlocks to a simple text prompt for now
        // TODO: Handle images, resources, etc.
        let prompt_text = if let Some(blocks) = prompt_blocks.as_array() {
            blocks
                .iter()
                .filter_map(|block| {
                    if let Some(text) = block.get("text").and_then(|v| v.as_str()) {
                        Some(text.to_string())
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            prompt_blocks.to_string()
        };

        info!("Processing prompt for session {}: {}", session_id, &prompt_text[..prompt_text.len().min(50)]);

        // Look up the session
        let session = self.session_manager.get_session(session_id).await
            .ok_or_else(|| AcpError::InvalidRequest(format!("Session not found: {}", session_id)))?;

        // Reset cancellation flag for this session
        session.cancellation_flag.store(false, std::sync::atomic::Ordering::SeqCst);

        // Process with the session's agent
        let mut agent = session.agent.lock().await;
        let response = agent
            .process_message(&prompt_text, session.cancellation_flag.clone())
            .await
            .map_err(|e| {
                if e.to_string().contains("CANCELLED") {
                    AcpError::Cancelled
                } else {
                    AcpError::Agent(e)
                }
            })?;

        info!("Session {} prompt completed", session_id);

        // Build ACP PromptResponse
        let acp_response = json!({
            "stopReason": "endTurn",  // We completed successfully
            "meta": {
                "response": response
            }
        });

        Ok(acp_response)
    }

    /// Handle session/cancel request (ACP official protocol)
    async fn handle_session_cancel(&mut self, params: Option<Value>) -> AcpResult<Value> {
        // Extract session_id if provided
        if let Some(params) = params {
            if let Some(session_id) = params.get("sessionId").and_then(|v| v.as_str()) {
                info!("Cancelling session: {}", session_id);

                // Look up the session and cancel it
                if let Some(session) = self.session_manager.get_session(session_id).await {
                    session.cancel();
                    info!("Session {} cancelled", session_id);
                } else {
                    warn!("Session {} not found for cancellation", session_id);
                }
            }
        } else {
            // No session ID provided - cancel all sessions (legacy behavior)
            warn!("No sessionId provided, cancelling all sessions");
            // Cancel all active sessions
            for session_id in self.session_manager.list_session_ids().await {
                if let Some(session) = self.session_manager.get_session(&session_id).await {
                    session.cancel();
                }
            }
        }

        Ok(json!({"cancelled": true}))
    }

    /// Handle configuration change
    async fn handle_configuration_change(&mut self, _params: Option<Value>) -> AcpResult<Value> {
        debug!("Configuration changed");
        Ok(json!(null))
    }

    /// Handle text document opened
    async fn handle_text_document_opened(&mut self, params: Option<Value>) -> AcpResult<Value> {
        debug!("Text document opened: {:?}", params);
        // Could add to context automatically
        Ok(json!(null))
    }

    /// Handle text document changed
    async fn handle_text_document_changed(&mut self, params: Option<Value>) -> AcpResult<Value> {
        debug!("Text document changed: {:?}", params);
        Ok(json!(null))
    }

    /// Handle text document closed
    async fn handle_text_document_closed(&mut self, params: Option<Value>) -> AcpResult<Value> {
        debug!("Text document closed: {:?}", params);
        Ok(json!(null))
    }

    /// Handle prompt request (main interaction)
    async fn handle_prompt(&mut self, params: Option<Value>) -> AcpResult<Value> {
        if !self.initialized {
            return Err(AcpError::InvalidRequest(
                "Server not initialized".to_string(),
            ));
        }

        let params = params.ok_or_else(|| AcpError::InvalidRequest("Missing params".to_string()))?;

        let prompt = params
            .get("prompt")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AcpError::InvalidRequest("Missing prompt".to_string()))?;

        info!("Processing prompt: {}", &prompt[..prompt.len().min(50)]);

        // Reset cancellation flag
        self.cancellation_flag.store(false, std::sync::atomic::Ordering::SeqCst);

        // Process with Flexorama agent
        let mut agent = self.agent.lock().await;
        let response = agent
            .process_message(prompt, self.cancellation_flag.clone())
            .await
            .map_err(|e| {
                if e.to_string().contains("CANCELLED") {
                    AcpError::Cancelled
                } else {
                    AcpError::Agent(e)
                }
            })?;

        // Get token usage
        let usage = agent.get_token_usage();

        Ok(json!({
            "response": response,
            "usage": {
                "inputTokens": usage.total_input_tokens,
                "outputTokens": usage.total_output_tokens,
                "totalTokens": usage.total_tokens()
            }
        }))
    }

    /// Handle cancel request
    async fn handle_cancel(&mut self, _params: Option<Value>) -> AcpResult<Value> {
        info!("Cancelling current operation");
        self.cancellation_flag.store(true, std::sync::atomic::Ordering::SeqCst);
        Ok(json!({"cancelled": true}))
    }

    /// Handle read file request
    async fn handle_read_file(&self, params: Option<Value>) -> AcpResult<Value> {
        let params = params.ok_or_else(|| AcpError::InvalidRequest("Missing params".to_string()))?;
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AcpError::InvalidRequest("Missing path parameter".to_string()))?;

        let content = self.filesystem.read_file(path).await?;
        Ok(json!({"content": content}))
    }

    /// Handle write file request
    async fn handle_write_file(&self, params: Option<Value>) -> AcpResult<Value> {
        let params = params.ok_or_else(|| AcpError::InvalidRequest("Missing params".to_string()))?;
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AcpError::InvalidRequest("Missing path parameter".to_string()))?;
        let content = params
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AcpError::InvalidRequest("Missing content parameter".to_string()))?;

        self.filesystem.write_file(path, content).await?;
        Ok(json!({"success": true}))
    }

    /// Handle list directory request
    async fn handle_list_directory(&self, params: Option<Value>) -> AcpResult<Value> {
        let params = params.ok_or_else(|| AcpError::InvalidRequest("Missing params".to_string()))?;
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AcpError::InvalidRequest("Missing path parameter".to_string()))?;

        let entries = self.filesystem.list_directory(path).await?;
        let entries_json: Vec<Value> = entries
            .iter()
            .map(|e| {
                json!({
                    "name": e.name,
                    "isDirectory": e.is_directory,
                    "path": e.path
                })
            })
            .collect();

        Ok(json!({"entries": entries_json}))
    }

    /// Handle glob request
    async fn handle_glob(&self, params: Option<Value>) -> AcpResult<Value> {
        let params = params.ok_or_else(|| AcpError::InvalidRequest("Missing params".to_string()))?;
        let pattern = params
            .get("pattern")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AcpError::InvalidRequest("Missing pattern parameter".to_string()))?;

        let files = self.filesystem.glob(pattern).await?;
        Ok(json!({"files": files}))
    }

    /// Handle delete request
    async fn handle_delete(&self, params: Option<Value>) -> AcpResult<Value> {
        let params = params.ok_or_else(|| AcpError::InvalidRequest("Missing params".to_string()))?;
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AcpError::InvalidRequest("Missing path parameter".to_string()))?;

        self.filesystem.delete(path).await?;
        Ok(json!({"success": true}))
    }

    /// Handle create directory request
    async fn handle_create_directory(&self, params: Option<Value>) -> AcpResult<Value> {
        let params = params.ok_or_else(|| AcpError::InvalidRequest("Missing params".to_string()))?;
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AcpError::InvalidRequest("Missing path parameter".to_string()))?;

        self.filesystem.create_directory(path).await?;
        Ok(json!({"success": true}))
    }

    /// Handle add context file request
    async fn handle_add_context_file(&self, params: Option<Value>) -> AcpResult<Value> {
        let params = params.ok_or_else(|| AcpError::InvalidRequest("Missing params".to_string()))?;
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AcpError::InvalidRequest("Missing path parameter".to_string()))?;

        // Resolve path relative to workspace
        let resolved_path = self.filesystem.resolve_path(path)?;
        let path_str = resolved_path
            .to_str()
            .ok_or_else(|| AcpError::InvalidPath(resolved_path.display().to_string()))?;

        // Add to agent context
        let mut agent = self.agent.lock().await;
        agent.add_context_file(path_str).await
            .map_err(|e| AcpError::Agent(e))?;

        Ok(json!({"success": true, "path": path_str}))
    }

    /// Handle clear context request
    async fn handle_clear_context(&self, _params: Option<Value>) -> AcpResult<Value> {
        let mut agent = self.agent.lock().await;
        agent.clear_conversation_keep_agents_md().await
            .map_err(|e| AcpError::Agent(e))?;

        Ok(json!({"success": true}))
    }

    /// Handle apply edit request
    async fn handle_apply_edit(&self, params: Option<Value>) -> AcpResult<Value> {
        let params = params.ok_or_else(|| AcpError::InvalidRequest("Missing params".to_string()))?;
        let path = params
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AcpError::InvalidRequest("Missing path parameter".to_string()))?;
        let old_string = params
            .get("oldString")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AcpError::InvalidRequest("Missing oldString parameter".to_string()))?;
        let new_string = params
            .get("newString")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AcpError::InvalidRequest("Missing newString parameter".to_string()))?;

        // Resolve path
        let resolved_path = self.filesystem.resolve_path(path)?;
        let path_str = resolved_path
            .to_str()
            .ok_or_else(|| AcpError::InvalidPath(resolved_path.display().to_string()))?;

        // Use edit_file tool
        let call = crate::tools::ToolCall {
            id: "acp-edit".to_string(),
            name: "edit_file".to_string(),
            arguments: json!({
                "file_path": path_str,
                "old_string": old_string,
                "new_string": new_string
            }),
        };

        let security_manager = self.agent.lock().await.get_file_security_manager();
        let mut manager = security_manager.write().await;
        let result = crate::tools::edit_file::edit_file(&call, &mut *manager, self.yolo_mode).await?;

        if result.is_error {
            Err(AcpError::Agent(anyhow::anyhow!(result.content)))
        } else {
            Ok(json!({"success": true, "message": result.content}))
        }
    }

    /// Get workspace root
    pub fn workspace_root(&self) -> Option<&PathBuf> {
        self.workspace_root.as_ref()
    }

    /// Check if initialized
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Provider;

    fn create_test_handler() -> FlexoramaAcpHandler {
        let config = Config {
            api_key: "test-key".to_string(),
            provider: Provider::Anthropic,
            base_url: "https://api.anthropic.com/v1".to_string(),
            default_model: "test-model".to_string(),
            max_tokens: 4096,
            temperature: 0.7,
            default_system_prompt: None,
            bash_security: Default::default(),
            file_security: Default::default(),
            mcp: Default::default(),
            skills: Default::default(),
        };

        let agent = Agent::new(config.clone(), "test-model".to_string(), false, false);

        FlexoramaAcpHandler::new(agent, config, "test-model".to_string(), false)
    }

    #[tokio::test]
    async fn test_initialize() {
        let mut handler = create_test_handler();

        let params = json!({
            "workspaceRoot": "/test/workspace",
            "capabilities": {}
        });

        let result = handler.handle_initialize(Some(params)).await;
        assert!(result.is_ok());
        assert!(handler.is_initialized());
        assert_eq!(
            handler.workspace_root(),
            Some(&PathBuf::from("/test/workspace"))
        );
    }

    #[tokio::test]
    async fn test_shutdown() {
        let mut handler = create_test_handler();
        handler.initialized = true;

        let result = handler.handle_shutdown().await;
        assert!(result.is_ok());
        assert!(!handler.is_initialized());
    }

    #[tokio::test]
    async fn test_handle_request_unknown_method() {
        let mut handler = create_test_handler();

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "unknown/method".to_string(),
            params: None,
        };

        let response = handler.handle_request(request).await;

        assert!(response.error.is_some());
        assert!(response.result.is_none());
        let error = response.error.unwrap();
        assert!(error.message.contains("Unknown method"));
    }

    #[tokio::test]
    async fn test_handle_initialized() {
        let mut handler = create_test_handler();

        let result = handler.handle_initialized().await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_handle_cancel() {
        let mut handler = create_test_handler();

        let result = handler.handle_cancel(None).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), json!({"cancelled": true}));
    }

    #[tokio::test]
    async fn test_handle_clear_context() {
        let mut handler = create_test_handler();

        let result = handler.handle_clear_context(None).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), json!({"success": true}));
    }

    #[tokio::test]
    async fn test_handle_configuration_change() {
        let mut handler = create_test_handler();

        let result = handler.handle_configuration_change(None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_handle_request_missing_params() {
        let mut handler = create_test_handler();

        // Test fs/readFile without params
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "fs/readFile".to_string(),
            params: None,
        };

        let response = handler.handle_request(request).await;
        assert!(response.error.is_some());
        let error = response.error.unwrap();
        assert!(error.message.contains("Missing params"));
    }

    #[tokio::test]
    async fn test_handle_read_file_missing_path_param() {
        let handler = create_test_handler();

        let result = handler.handle_read_file(Some(json!({}))).await;
        assert!(result.is_err());

        match result {
            Err(AcpError::InvalidRequest(msg)) => {
                assert!(msg.contains("Missing path parameter"));
            }
            _ => panic!("Expected InvalidRequest error"),
        }
    }

    #[tokio::test]
    async fn test_workspace_root_getter() {
        let mut handler = create_test_handler();

        assert!(handler.workspace_root().is_none());

        handler.workspace_root = Some(PathBuf::from("/test"));
        assert_eq!(handler.workspace_root(), Some(&PathBuf::from("/test")));
    }

    #[tokio::test]
    async fn test_is_initialized_getter() {
        let mut handler = create_test_handler();

        assert!(!handler.is_initialized());

        handler.initialized = true;
        assert!(handler.is_initialized());
    }

    #[tokio::test]
    async fn test_session_new() {
        let mut handler = create_test_handler();
        handler.initialized = true;

        let params = json!({
            "cwd": "/tmp/test",
            "mcpServers": []
        });

        let result = handler.handle_session_new(Some(params)).await;
        assert!(result.is_ok());

        let value = result.unwrap();
        assert!(value.get("sessionId").is_some());
        assert!(value.get("sessionId").unwrap().is_string());

        // Verify that a session was created
        let session_id = value.get("sessionId").unwrap().as_str().unwrap();
        let session = handler.session_manager.get_session(session_id).await;
        assert!(session.is_some());
    }

    #[tokio::test]
    async fn test_session_new_not_initialized() {
        let mut handler = create_test_handler();
        // Don't set initialized = true

        let params = json!({
            "cwd": "/tmp",
            "mcpServers": []
        });

        let result = handler.handle_session_new(Some(params)).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not initialized"));
    }

    #[tokio::test]
    async fn test_session_new_no_params() {
        let mut handler = create_test_handler();
        handler.initialized = true;

        // Should succeed even without params (cwd is optional)
        let result = handler.handle_session_new(None).await;
        assert!(result.is_ok());

        let value = result.unwrap();
        assert!(value.get("sessionId").is_some());
    }

    #[tokio::test]
    async fn test_session_prompt_missing_session_id() {
        let mut handler = create_test_handler();
        handler.initialized = true;

        let params = json!({
            "prompt": [{"text": "Hello"}]
        });

        let result = handler.handle_session_prompt(Some(params)).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("sessionId"));
    }

    #[tokio::test]
    async fn test_session_prompt_missing_prompt() {
        let mut handler = create_test_handler();
        handler.initialized = true;

        let params = json!({
            "sessionId": "test-session-123"
        });

        let result = handler.handle_session_prompt(Some(params)).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("prompt"));
    }

    #[tokio::test]
    async fn test_session_prompt_not_initialized() {
        let mut handler = create_test_handler();
        // Don't set initialized = true

        let params = json!({
            "sessionId": "test-session-123",
            "prompt": [{"text": "Hello"}]
        });

        let result = handler.handle_session_prompt(Some(params)).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("not initialized"));
    }

    #[tokio::test]
    async fn test_session_cancel() {
        let mut handler = create_test_handler();

        let params = json!({
            "sessionId": "test-session-123"
        });

        let result = handler.handle_session_cancel(Some(params)).await;
        assert!(result.is_ok());

        let value = result.unwrap();
        assert_eq!(value.get("cancelled"), Some(&json!(true)));
    }

    #[tokio::test]
    async fn test_session_cancel_no_params() {
        let mut handler = create_test_handler();

        // Should succeed even without params
        let result = handler.handle_session_cancel(None).await;
        assert!(result.is_ok());

        let value = result.unwrap();
        assert_eq!(value.get("cancelled"), Some(&json!(true)));
    }

    #[tokio::test]
    async fn test_handle_request_session_new() {
        let mut handler = create_test_handler();
        handler.initialized = true;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(1)),
            method: "session/new".to_string(),
            params: Some(json!({
                "cwd": "/tmp",
                "mcpServers": []
            })),
        };

        let response = handler.handle_request(request).await;
        assert!(response.error.is_none());
        assert!(response.result.is_some());

        let result = response.result.unwrap();
        assert!(result.get("sessionId").is_some());
    }

    #[tokio::test]
    async fn test_handle_request_session_prompt() {
        let mut handler = create_test_handler();
        handler.initialized = true;

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(2)),
            method: "session/prompt".to_string(),
            params: Some(json!({
                "sessionId": "test-123",
                "prompt": [
                    {"text": "What is 2+2?"}
                ]
            })),
        };

        let response = handler.handle_request(request).await;
        // Note: This will fail because we can't actually call the LLM in tests
        // but it should route correctly
        assert!(response.id == Some(json!(2)));
        assert_eq!(response.jsonrpc, "2.0");
    }

    #[tokio::test]
    async fn test_handle_request_session_cancel() {
        let mut handler = create_test_handler();

        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            id: Some(json!(3)),
            method: "session/cancel".to_string(),
            params: Some(json!({
                "sessionId": "test-123"
            })),
        };

        let response = handler.handle_request(request).await;
        assert!(response.error.is_none());
        assert!(response.result.is_some());

        let result = response.result.unwrap();
        assert_eq!(result.get("cancelled"), Some(&json!(true)));
    }
}
