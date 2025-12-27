use crate::mcp::McpManager;
use crate::security::{BashSecurityManager, FileSecurityManager};
use crate::tools::display::DisplayFactory;
use anyhow::{anyhow, Result};
use colored::*;
use log::{debug, error, info, warn};
use serde_json::json;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

// Structure to save conversation context when switching to subagent
#[derive(Debug, Clone)]
struct SavedConversationContext {
    conversation: Vec<crate::anthropic::Message>,
    system_prompt: Option<String>,
    current_conversation_id: Option<String>,
    model: String,
}

#[derive(Debug, Clone)]
pub struct SnapshotMessage {
    pub role: String,
    pub content: Vec<ContentBlock>,
}

#[derive(Debug, Clone, Serialize)]
pub struct StreamToolEvent {
    pub event: String,
    pub tool_use_id: String,
    pub name: String,
    pub input: Option<serde_json::Value>,
    pub content: Option<String>,
    pub is_error: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct ConversationSnapshot {
    pub id: Option<String>,
    pub system_prompt: Option<String>,
    pub model: String,
    pub messages: Vec<SnapshotMessage>,
}

use crate::anthropic::{ContentBlock, Message, Usage};
use crate::config::{Config, Provider};
use crate::conversation::ConversationManager;
use crate::database::{Conversation as StoredConversation, DatabaseManager};
use crate::llm::LlmClient;
use crate::subagent;
use crate::tools::{
    bash, create_bash_tool, create_create_directory_tool, create_delete_file_tool,
    create_directory, create_edit_file_tool, create_write_file_tool, delete_file, edit_file,
    get_builtin_tools, write_file, Tool, ToolCall, ToolRegistry, ToolResult,
};

#[derive(Debug, Clone)]
pub struct TokenUsage {
    pub request_count: u32,
    pub total_input_tokens: u32,
    pub total_output_tokens: u32,
}

impl TokenUsage {
    pub fn new() -> Self {
        Self {
            request_count: 0,
            total_input_tokens: 0,
            total_output_tokens: 0,
        }
    }

    pub fn add_usage(&mut self, usage: &Usage) {
        self.request_count += 1;
        self.total_input_tokens += usage.input_tokens;
        self.total_output_tokens += usage.output_tokens;
    }

    pub fn total_tokens(&self) -> u32 {
        self.total_input_tokens + self.total_output_tokens
    }

    pub fn reset(&mut self) {
        self.request_count = 0;
        self.total_input_tokens = 0;
        self.total_output_tokens = 0;
    }
}

pub struct Agent {
    client: LlmClient,
    model: String,
    tools: Arc<RwLock<HashMap<String, Tool>>>,
    conversation_manager: ConversationManager,
    token_usage: TokenUsage,
    mcp_manager: Option<Arc<McpManager>>,
    last_mcp_tools_version: u64,
    bash_security_manager: Arc<RwLock<BashSecurityManager>>,
    file_security_manager: Arc<RwLock<FileSecurityManager>>,
    yolo_mode: bool,
    plan_mode: bool,
    plan_mode_saved_system_prompt: Option<Option<String>>,
    // Store previous context when switching to subagent
    saved_conversation_context: Option<SavedConversationContext>,
    // New display system components
    pub tool_registry: Arc<RwLock<ToolRegistry>>,
    provider: Provider,
    base_url: String,
}

impl Agent {
    pub fn snapshot_conversation(&self) -> ConversationSnapshot {
        let messages = self
            .conversation_manager
            .conversation
            .iter()
            .map(|m| SnapshotMessage {
                role: m.role.clone(),
                content: m.content.clone(),
            })
            .collect();

        ConversationSnapshot {
            id: self.conversation_manager.current_conversation_id.clone(),
            system_prompt: self.conversation_manager.system_prompt.clone(),
            model: self.conversation_manager.model.clone(),
            messages,
        }
    }
    pub fn new(config: Config, model: String, yolo_mode: bool, plan_mode: bool) -> Self {
        let base_url = config.base_url.clone();
        let client = LlmClient::new(config.provider, config.api_key, base_url.clone());
        let tools = get_builtin_tools()
            .into_iter()
            .map(|tool| (tool.name.clone(), tool))
            .collect();

        // Create bash security manager
        let bash_security_manager = Arc::new(RwLock::new(BashSecurityManager::new(
            config.bash_security.clone(),
        )));

        // Create file security manager
        let file_security_manager = Arc::new(RwLock::new(FileSecurityManager::new(
            config.file_security.clone(),
        )));

        // Create conversation manager
        let conversation_manager =
            ConversationManager::new(config.default_system_prompt, None, model.clone());

        // Initialize the new tool registry
        let tool_registry = Arc::new(RwLock::new(ToolRegistry::with_builtin_tools()));

        Self {
            client,
            model,
            tools: Arc::new(RwLock::new(tools)),
            conversation_manager,
            token_usage: TokenUsage::new(),
            mcp_manager: None,
            last_mcp_tools_version: 0,
            bash_security_manager,
            file_security_manager,
            yolo_mode,
            plan_mode,
            plan_mode_saved_system_prompt: None,
            saved_conversation_context: None,
            tool_registry,
            provider: config.provider,
            base_url,
        }
    }

    /// Create a new agent and apply plan mode filtering if needed
    pub async fn new_with_plan_mode(
        config: Config,
        model: String,
        yolo_mode: bool,
        plan_mode: bool,
    ) -> Self {
        let mut agent = Self::new(config, model, yolo_mode, plan_mode);

        // Add bash tool with security
        let security_manager = agent.bash_security_manager.clone();
        let yolo_mode = yolo_mode;
        if !plan_mode {
            let mut tools = agent.tools.write().await;
            tools.insert(
                "bash".to_string(),
                Tool {
                    name: "bash".to_string(),
                    description: if yolo_mode {
                        "Execute shell commands and return the output (YOLO MODE - no security checks)"
                            .to_string()
                    } else {
                        "Execute shell commands and return the output (with security)".to_string()
                    },
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "command": {
                                "type": "string",
                                "description": "Shell command to execute"
                            }
                        },
                        "required": ["command"]
                    }),
                    handler: Box::new(move |call: ToolCall| {
                        let security_manager = security_manager.clone();
                        let yolo_mode = yolo_mode;
                        Box::pin(async move {
                            // Create a wrapper function that handles the mutable reference
                            async fn bash_wrapper(
                                call: ToolCall,
                                security_manager: Arc<RwLock<BashSecurityManager>>,
                                yolo_mode: bool,
                            ) -> Result<ToolResult> {
                                let mut manager = security_manager.write().await;
                                bash(&call, &mut *manager, yolo_mode).await
                            }

                            bash_wrapper(call, security_manager, yolo_mode).await
                        })
                    }),
                    metadata: None, // TODO: Add metadata for bash tool
                },
            );
        }

        // Apply plan mode filtering
        let _ = agent.apply_plan_mode_filtering().await;
        agent
    }

    /// Apply plan mode filtering to tools (async version)
    async fn apply_plan_mode_filtering(&mut self) -> Result<()> {
        if self.plan_mode {
            let mut tools = self.tools.write().await;
            let registry = self.tool_registry.read().await;

            // Filter tools based on readonly status
            tools.retain(|name, _| registry.is_readonly(name));
        }
        Ok(())
    }

    fn derive_plan_title(plan_markdown: &str) -> Option<String> {
        plan_markdown.lines().find_map(|line| {
            let trimmed = line.trim();
            if trimmed.starts_with('#') {
                let title = trimmed.trim_start_matches('#').trim().to_string();
                if !title.is_empty() {
                    Some(title)
                } else {
                    None
                }
            } else {
                None
            }
        })
    }

    async fn persist_plan(
        &self,
        user_request: &str,
        plan_markdown: &str,
    ) -> Result<Option<String>> {
        let title = Self::derive_plan_title(plan_markdown);
        let plan_id = self
            .conversation_manager
            .save_plan(user_request, plan_markdown, title)
            .await?;

        if let Some(ref id) = plan_id {
            info!("Plan saved to database with ID: {}", id);
        } else {
            debug!("No database configured; plan not persisted");
        }

        Ok(plan_id)
    }

    /// Load a stored plan by ID and prepare it for execution
    pub async fn load_plan_for_execution(&mut self, plan_id: &str) -> Result<String> {
        let db = self
            .conversation_manager
            .database_manager
            .as_ref()
            .ok_or_else(|| anyhow!("Database not configured; cannot load plans"))?
            .clone();

        let plan = db
            .get_plan(plan_id)
            .await?
            .ok_or_else(|| anyhow!("Plan {} not found", plan_id))?;

        // Ensure we are not in plan mode for execution
        self.set_plan_mode(false).await?;

        let title = plan
            .title
            .clone()
            .unwrap_or_else(|| "Saved plan".to_string());
        let message = format!(
            "Execute the following saved plan (id: {} - title: {}):\n\n{}",
            plan.id, title, plan.plan_markdown
        );

        Ok(message)
    }

    pub fn with_mcp_manager(mut self, mcp_manager: Arc<McpManager>) -> Self {
        self.mcp_manager = Some(mcp_manager);
        self
    }

    pub fn with_database_manager(mut self, database_manager: Arc<DatabaseManager>) -> Self {
        self.conversation_manager.database_manager = Some(database_manager);
        self
    }

    /// Refresh MCP tools from connected servers (only if they have changed)
    pub async fn refresh_mcp_tools(&mut self) -> Result<()> {
        if self.plan_mode {
            debug!("Plan mode enabled; skipping MCP tool refresh (read-only mode)");
            return Ok(());
        }

        if let Some(mcp_manager) = &self.mcp_manager {
            // Check if tools have changed since last refresh
            let current_version = mcp_manager.get_tools_version().await;

            if current_version == self.last_mcp_tools_version {
                debug!("MCP tools unchanged, skipping refresh");
                return Ok(());
            }

            debug!(
                "MCP tools changed (version {} -> {}), refreshing",
                self.last_mcp_tools_version, current_version
            );

            // Clear existing MCP tools
            let mut tools = self.tools.write().await;
            tools.retain(|name, _| !name.starts_with("mcp_"));

            // Add bash tool with security using centralized function
            let bash_tool = create_bash_tool(self.bash_security_manager.clone(), self.yolo_mode);
            tools.insert("bash".to_string(), bash_tool);

            // Add file operation tools with security
            let file_security_manager = self.file_security_manager.clone();
            let yolo_mode = self.yolo_mode;

            let write_file_tool = create_write_file_tool(file_security_manager.clone(), yolo_mode);
            tools.insert("write_file".to_string(), write_file_tool);

            let edit_file_tool = create_edit_file_tool(file_security_manager.clone(), yolo_mode);
            tools.insert("edit_file".to_string(), edit_file_tool);

            let delete_file_tool =
                create_delete_file_tool(file_security_manager.clone(), yolo_mode);
            tools.insert("delete_file".to_string(), delete_file_tool);

            let create_directory_tool =
                create_create_directory_tool(file_security_manager.clone(), yolo_mode);
            tools.insert("create_directory".to_string(), create_directory_tool);

            // Get all MCP tools
            match mcp_manager.get_all_tools().await {
                Ok(mcp_tools) => {
                    for (server_name, mcp_tool) in mcp_tools {
                        let tool = crate::tools::create_mcp_tool(
                            &server_name,
                            mcp_tool,
                            mcp_manager.clone(),
                        );
                        tools.insert(tool.name.clone(), tool);
                    }
                    self.last_mcp_tools_version = current_version;
                    info!(
                        "Refreshed {} MCP tools",
                        tools
                            .iter()
                            .filter(|(name, _)| name.starts_with("mcp_"))
                            .count()
                    );
                }
                Err(e) => {
                    warn!("Failed to refresh MCP tools: {}", e);
                }
            }
        }
        Ok(())
    }

    /// Force refresh MCP tools regardless of version
    pub async fn force_refresh_mcp_tools(&mut self) -> Result<()> {
        if self.plan_mode {
            debug!("Plan mode enabled; skipping forced MCP tool refresh");
            return Ok(());
        }

        if let Some(_mcp_manager) = &self.mcp_manager {
            // Reset version to force refresh
            self.last_mcp_tools_version = 0;
            self.refresh_mcp_tools().await
        } else {
            // Even without MCP manager, ensure bash and file tools are available
            let mut tools = self.tools.write().await;
            if !tools.contains_key("bash") {
                let bash_tool =
                    create_bash_tool(self.bash_security_manager.clone(), self.yolo_mode);
                tools.insert("bash".to_string(), bash_tool);
            }

            // Ensure file operation tools are available
            let file_security_manager = self.file_security_manager.clone();
            let yolo_mode = self.yolo_mode;

            if !tools.contains_key("write_file") {
                let write_file_tool =
                    create_write_file_tool(file_security_manager.clone(), yolo_mode);
                tools.insert("write_file".to_string(), write_file_tool);
            }

            if !tools.contains_key("edit_file") {
                let edit_file_tool =
                    create_edit_file_tool(file_security_manager.clone(), yolo_mode);
                tools.insert("edit_file".to_string(), edit_file_tool);
            }

            if !tools.contains_key("delete_file") {
                let delete_file_tool =
                    create_delete_file_tool(file_security_manager.clone(), yolo_mode);
                tools.insert("delete_file".to_string(), delete_file_tool);
            }

            if !tools.contains_key("create_directory") {
                let create_directory_tool =
                    create_create_directory_tool(file_security_manager.clone(), yolo_mode);
                tools.insert("create_directory".to_string(), create_directory_tool);
            }
            Ok(())
        }
    }

    /// Set the system prompt for the conversation
    pub fn set_system_prompt(&mut self, system_prompt: String) {
        self.conversation_manager.system_prompt = Some(system_prompt);
    }

    /// Apply the plan-mode system prompt while preserving any existing prompt context
    pub fn apply_plan_mode_prompt(&mut self) {
        let existing_prompt = self.conversation_manager.system_prompt.clone();
        // Save the pre-plan prompt once when enabling
        if self.plan_mode_saved_system_prompt.is_none() {
            self.plan_mode_saved_system_prompt = Some(existing_prompt.clone());
        }
        let mut prompt = String::from(
            "You are operating in plan mode. Do not execute tools that change the system or write to files. \
            Use only read-only tools when absolutely necessary to gather context. \
            Analyze the user's request and produce a complete, actionable implementation plan in Markdown. \
            Structure the plan with clear goals, assumptions/context, ordered steps, risks, and validation. \
            Do not perform the work or request approvals—only return the plan.",
        );

        if let Some(existing) = existing_prompt {
            prompt.push_str("\n\nAdditional context to respect:\n");
            prompt.push_str(&existing);
        }

        self.conversation_manager.system_prompt = Some(prompt);
    }

    /// Toggle plan mode at runtime, updating tool availability and prompts
    pub async fn set_plan_mode(&mut self, enabled: bool) -> Result<()> {
        if enabled == self.plan_mode {
            return Ok(());
        }

        self.plan_mode = enabled;

        {
            let mut tools = self.tools.write().await;
            if enabled {
                // Filter tools based on readonly status from metadata
                let registry = self.tool_registry.read().await;
                tools.retain(|name, _| registry.is_readonly(name));
            } else {
                // Rebuild essential tools when exiting plan mode
                if !tools.contains_key("bash") {
                    let bash_tool =
                        create_bash_tool(self.bash_security_manager.clone(), self.yolo_mode);
                    tools.insert("bash".to_string(), bash_tool);
                }
                let file_security_manager = self.file_security_manager.clone();
                let yolo_mode = self.yolo_mode;

                if !tools.contains_key("write_file") {
                    let write_file_tool =
                        create_write_file_tool(file_security_manager.clone(), yolo_mode);
                    tools.insert("write_file".to_string(), write_file_tool);
                }
                if !tools.contains_key("edit_file") {
                    let edit_file_tool =
                        create_edit_file_tool(file_security_manager.clone(), yolo_mode);
                    tools.insert("edit_file".to_string(), edit_file_tool);
                }
                if !tools.contains_key("delete_file") {
                    let delete_file_tool =
                        create_delete_file_tool(file_security_manager.clone(), yolo_mode);
                    tools.insert("delete_file".to_string(), delete_file_tool);
                }
                if !tools.contains_key("create_directory") {
                    let create_directory_tool =
                        create_create_directory_tool(file_security_manager.clone(), yolo_mode);
                    tools.insert("create_directory".to_string(), create_directory_tool);
                }
            }
        }

        if enabled {
            self.apply_plan_mode_prompt();
        } else if let Some(saved) = self.plan_mode_saved_system_prompt.take() {
            self.conversation_manager.system_prompt = saved;
        }

        // Refresh MCP tools only when leaving plan mode (plan mode skips refresh)
        if !enabled {
            let _ = self.force_refresh_mcp_tools().await;
        }

        Ok(())
    }

    /// Add a file as context to the conversation
    pub async fn add_context_file(&mut self, file_path: &str) -> Result<()> {
        self.conversation_manager.add_context_file(file_path).await
    }

    /// Extract file paths from message using @path syntax
    pub fn extract_context_files(&self, message: &str) -> Vec<String> {
        self.conversation_manager.extract_context_files(message)
    }

    /// Remove @file syntax from message and return cleaned message
    pub fn clean_message(&self, message: &str) -> String {
        self.conversation_manager.clean_message(message)
    }

    pub async fn process_message(
        &mut self,
        message: &str,
        cancellation_flag: Arc<AtomicBool>,
    ) -> Result<String> {
        self.process_message_with_stream(message, None, None, cancellation_flag)
            .await
    }

    pub async fn process_message_with_stream(
        &mut self,
        message: &str,
        on_stream_content: Option<Arc<dyn Fn(String) + Send + Sync + 'static>>,
        on_tool_event: Option<Arc<dyn Fn(StreamToolEvent) + Send + Sync + 'static>>,
        cancellation_flag: Arc<AtomicBool>,
    ) -> Result<String> {
        // Log incoming user message
        debug!("Processing user message: {}", message);
        debug!(
            "Current conversation length: {}",
            self.conversation_manager.conversation.len()
        );

        // Extract and add context files from @ syntax
        let context_files = self.extract_context_files(message);
        for file_path in &context_files {
            debug!("Auto-adding context file from @ syntax: {}", file_path);
            match self.add_context_file(file_path).await {
                Ok(_) => app_println!("{} Added context file: {}", "✓".green(), file_path),
                Err(e) => app_eprintln!(
                    "{} Failed to add context file '{}': {}",
                    "✗".red(),
                    file_path,
                    e
                ),
            }
        }

        // Clean message by removing @file syntax
        let cleaned_message = self.clean_message(message);

        // If message is empty after cleaning (only contained @file references),
        // return early without making an API call
        if cleaned_message.trim().is_empty() {
            debug!("Message only contained @file references, not making API call");
            return Ok("".to_string());
        }

        // Add cleaned user message to conversation
        self.conversation_manager.conversation.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::text(cleaned_message.clone())],
        });

        // Save user message to database
        if let Err(e) = self
            .conversation_manager
            .save_message_to_conversation("user", &cleaned_message, 0)
            .await
        {
            warn!("Failed to save user message to database: {}", e);
        }

        // Refresh MCP tools before processing the message (only if they have changed)
        if let Err(e) = self.refresh_mcp_tools().await {
            warn!("Failed to refresh MCP tools: {}", e);
            error!("MCP tools may not be available - some tool calls might fail");
        }
        let mut final_response = String::new();
        let max_iterations = 500;
        let mut iteration = 0;

        while iteration < max_iterations {
            iteration += 1;

            // Check for cancellation
            if cancellation_flag.load(Ordering::SeqCst) {
                return Err(anyhow::anyhow!("CANCELLED"));
            }

            // Get available tools

            let available_tools: Vec<Tool> = {
                let tools = self.tools.read().await;
                let registry = self.tool_registry.read().await;
                tools
                    .values()
                    .filter(|tool| !self.plan_mode || registry.is_readonly(&tool.name))
                    .cloned()
                    .collect()
            };

            // Call Anthropic API with streaming if callback provided
            let response = if let Some(ref on_content) = on_stream_content {
                self.client
                    .create_message_stream(
                        &self.model,
                        self.conversation_manager.conversation.clone(),
                        &available_tools,
                        4096,
                        0.7,
                        self.conversation_manager.system_prompt.as_ref(),
                        Arc::clone(on_content),
                        cancellation_flag.clone(),
                    )
                    .await?
            } else {
                self.client
                    .create_message(
                        &self.model,
                        self.conversation_manager.conversation.clone(),
                        &available_tools,
                        4096,
                        0.7,
                        self.conversation_manager.system_prompt.as_ref(),
                        cancellation_flag.clone(),
                    )
                    .await?
            };
            // Track token usage
            if let Some(usage) = &response.usage {
                self.token_usage.add_usage(usage);
                debug!(
                    "Updated token usage - Total: {} (Input: {}, Output: {})",
                    self.token_usage.total_tokens(),
                    self.token_usage.total_input_tokens,
                    self.token_usage.total_output_tokens
                );

                // Update usage statistics in database
                if let Err(e) = self
                    .conversation_manager
                    .update_database_usage_stats(
                        usage.input_tokens as i32,
                        usage.output_tokens as i32,
                    )
                    .await
                {
                    warn!("Failed to update database usage stats: {}", e);
                }
            }

            // Extract and output the text response from this API call
            let response_content = self.client.create_response_content(&response.content);
            if !response_content.is_empty() {
                if on_stream_content.is_none() {
                    // Only print if not streaming (streaming handles its own output)
                    app_println!("{}", response_content);
                }
                // Always accumulate response content, even in streaming mode
                final_response = response_content.clone();
            }

            // Check for tool calls
            let tool_calls = self.client.convert_tool_calls(&response.content);

            if tool_calls.is_empty() {
                // No tool calls, return the accumulated text response
                // final_response was already set above during response processing
                if final_response.is_empty() {
                    final_response = "(No response received from assistant)".to_string();
                }

                if self.plan_mode && !final_response.is_empty() {
                    match self.persist_plan(&cleaned_message, &final_response).await {
                        Ok(Some(plan_id)) => {
                            final_response
                                .push_str(&format!("\n\n_Plan saved with ID: `{}`._", plan_id));
                        }
                        Ok(None) => {}
                        Err(e) => {
                            warn!("Failed to persist plan: {}", e);
                        }
                    }
                }
                break;
            }

            // Execute tool calls using the new display system
            let tool_results: Vec<ToolResult> = {
                let mut results = Vec::new();
                for call in &tool_calls {
                    // Check for cancellation before executing each tool
                    if cancellation_flag.load(Ordering::SeqCst) {
                        return Err(anyhow::anyhow!("CANCELLED"));
                    }

                    debug!("Executing tool: {} with ID: {}", call.name, call.id);

                    if let (Some(db), Some(conversation_id)) = (
                        self.conversation_manager.database_manager.clone(),
                        self.conversation_manager.current_conversation_id.clone(),
                    ) {
                        let args_str =
                            serde_json::to_string(&call.arguments).unwrap_or_else(|_| {
                                call.arguments.to_string()
                            });
                        if let Err(e) = db
                            .add_tool_call(
                                &conversation_id,
                                None,
                                &call.id,
                                &call.name,
                                &args_str,
                            )
                            .await
                        {
                            warn!("Failed to record tool call {}: {}", call.name, e);
                        }
                    }

                    if let Some(callback) = &on_tool_event {
                        callback(StreamToolEvent {
                            event: "tool_call".to_string(),
                            tool_use_id: call.id.clone(),
                            name: call.name.clone(),
                            input: Some(call.arguments.clone()),
                            content: None,
                            is_error: None,
                        });
                    }

                    // Handle plan mode restrictions
                    if self.plan_mode {
                        let registry = self.tool_registry.read().await;
                        if !registry.is_readonly(&call.name) {
                            let error_content = format!(
                                "Plan mode is read-only. Tool '{}' is disabled.",
                                call.name
                            );
                            results.push(ToolResult {
                                tool_use_id: call.id.clone(),
                                content: error_content,
                                is_error: true,
                            });
                            continue;
                        }
                    }

                    // Use the new display system and execute tool
                    let result = self.execute_tool_with_display(call).await;
                    if let (Some(db), Some(conversation_id)) = (
                        self.conversation_manager.database_manager.clone(),
                        self.conversation_manager.current_conversation_id.clone(),
                    ) {
                        if let Err(e) = db
                            .complete_tool_call(&call.id, &result.content, result.is_error)
                            .await
                        {
                            warn!("Failed to record tool result {}: {}", call.name, e);
                        }
                    }
                    if let Some(callback) = &on_tool_event {
                        callback(StreamToolEvent {
                            event: "tool_result".to_string(),
                            tool_use_id: call.id.clone(),
                            name: call.name.clone(),
                            input: None,
                            content: Some(result.content.clone()),
                            is_error: Some(result.is_error),
                        });
                    }
                    results.push(result);
                }
                results
            };
            let _tool_results_count = tool_results.len();

            // Add assistant's tool use message to conversation
            let assistant_content: Vec<ContentBlock> = response.content.into_iter().collect();

            self.conversation_manager.conversation.push(Message {
                role: "assistant".to_string(),
                content: assistant_content.clone(),
            });

            // Save assistant response to database
            if let Err(e) = self
                .conversation_manager
                .save_message_to_conversation(
                    "assistant",
                    &self.client.create_response_content(&assistant_content),
                    response
                        .usage
                        .as_ref()
                        .map(|u| u.output_tokens)
                        .unwrap_or(0) as i32,
                )
                .await
            {
                warn!("Failed to save assistant message to database: {}", e);
            }

            // Add tool results to conversation
            for result in tool_results {
                self.conversation_manager.conversation.push(Message {
                    role: "assistant".to_string(),
                    content: vec![ContentBlock::tool_result(
                        result.tool_use_id,
                        result.content,
                        Some(result.is_error),
                    )],
                });
            }
        }

        if iteration >= max_iterations {
            final_response.push_str("\n\n(Note: Maximum tool iterations reached)");
        }

        // Add final assistant response to conversation if it exists
        if !final_response.is_empty() {
            self.conversation_manager.conversation.push(Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::text(final_response.clone())],
            });

            // Save final assistant response to database
            if let Err(e) = self
                .conversation_manager
                .save_message_to_conversation("assistant", &final_response, 0)
                .await
            {
                warn!("Failed to save final assistant message to database: {}", e);
            }
        }
        debug!("Final response generated ({} chars)", final_response.len());
        Ok(final_response)
    }

    pub fn get_token_usage(&self) -> &TokenUsage {
        &self.token_usage
    }

    pub fn reset_token_usage(&mut self) {
        self.token_usage.reset();
    }

    /// Display the current conversation context
    pub fn display_context(&self) {
        self.conversation_manager.display_context();
    }

    /// Get the bash security manager
    pub fn get_bash_security_manager(&self) -> Arc<RwLock<BashSecurityManager>> {
        self.bash_security_manager.clone()
    }

    /// Get the file security manager
    pub fn get_file_security_manager(&self) -> Arc<RwLock<FileSecurityManager>> {
        self.file_security_manager.clone()
    }

    pub async fn set_permission_handler(
        &mut self,
        handler: Option<crate::security::PermissionHandler>,
    ) {
        let mut bash = self.bash_security_manager.write().await;
        bash.set_permission_handler(handler.clone());
        let mut file = self.file_security_manager.write().await;
        file.set_permission_handler(handler);
    }

    /// Display the active LLM provider info
    pub fn display_provider(&self) {
        app_println!("{}", "LLM Provider".cyan().bold());
        app_println!("  Provider: {}", self.provider);
        app_println!("  Model: {}", self.model);
        app_println!("  Base URL: {}", self.base_url);
    }

    /// Get the active LLM provider
    pub fn provider(&self) -> Provider {
        self.provider
    }

    /// Get the active model name
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Update the active model for this session
    pub async fn set_model(&mut self, model: String) -> Result<()> {
        self.model = model.clone();
        self.conversation_manager.update_conversation_model(model).await
    }

    fn set_model_local(&mut self, model: String) {
        self.model = model.clone();
        self.conversation_manager.model = model;
    }

    /// Get current configuration (for saving permissions)
    pub async fn get_config_for_save(&self) -> crate::config::Config {
        use crate::config::Config;

        // Get current security settings from agent
        let security_manager = self.bash_security_manager.read().await;
        let current_security = security_manager.get_security().clone();

        // Create a basic config with the current bash security settings
        Config {
            api_key: "".to_string(), // Don't save API key from this method
            provider: self.provider,
            base_url: self.base_url.clone(),
            default_model: self.model.clone(),
            max_tokens: 4096,
            temperature: 0.7,
            default_system_prompt: self.conversation_manager.system_prompt.clone(),
            bash_security: current_security,
            file_security: crate::security::FileSecurity::default(),
            mcp: crate::config::McpConfig::default(),
        }
    }

    /// Get the database manager (if configured)
    pub fn database_manager(&self) -> Option<Arc<DatabaseManager>> {
        self.conversation_manager.database_manager.clone()
    }

    /// Get the current conversation ID (if any)
    pub fn current_conversation_id(&self) -> Option<String> {
        self.conversation_manager.current_conversation_id.clone()
    }

    /// Get the currently active subagent name (if any)
    pub fn active_subagent_name(&self) -> Option<String> {
        self.conversation_manager.subagent.clone()
    }

    /// Get the number of messages in the active conversation
    pub fn conversation_len(&self) -> usize {
        self.conversation_manager.conversation.len()
    }

    /// List recent conversations from the database
    pub async fn list_recent_conversations(
        &self,
        limit: i64,
        search_filter: Option<&str>,
    ) -> Result<Vec<StoredConversation>> {
        let database_manager = self
            .conversation_manager
            .database_manager
            .as_ref()
            .ok_or_else(|| anyhow!("Database is not configured"))?;

        database_manager
            .get_recent_conversations(limit, search_filter)
            .await
    }

    /// Replace the active conversation with one loaded from the database
    pub async fn resume_conversation(&mut self, conversation_id: &str) -> Result<()> {
        let database_manager = self
            .conversation_manager
            .database_manager
            .as_ref()
            .ok_or_else(|| anyhow!("Database is not configured"))?;

        let conversation = database_manager
            .get_conversation(conversation_id)
            .await?
            .ok_or_else(|| anyhow!("Conversation {} not found", conversation_id))?;

        let messages = database_manager
            .get_conversation_messages(conversation_id)
            .await?;
        let tool_calls = database_manager
            .get_conversation_tool_calls(conversation_id)
            .await?;

        self.model = conversation.model.clone();
        self.conversation_manager.set_conversation_from_records(
            conversation.id.clone(),
            conversation.system_prompt.clone(),
            conversation.model.clone(),
            conversation.subagent.clone(),
            &messages,
            &tool_calls,
        );

        Ok(())
    }

    /// Start a new conversation
    pub async fn start_new_conversation(&mut self) -> Result<String> {
        self.conversation_manager.start_new_conversation().await
    }

    /// Save a message to the current conversation in the database
    pub async fn save_message_to_conversation(
        &mut self,
        role: &str,
        content: &str,
        tokens: i32,
    ) -> Result<()> {
        self.conversation_manager
            .save_message_to_conversation(role, content, tokens)
            .await
    }

    /// Update usage statistics in the database
    pub async fn update_database_usage_stats(
        &mut self,
        input_tokens: i32,
        output_tokens: i32,
    ) -> Result<()> {
        self.conversation_manager
            .update_database_usage_stats(input_tokens, output_tokens)
            .await
    }

    pub async fn switch_to_subagent(
        &mut self,
        subagent_config: &subagent::SubagentConfig,
    ) -> Result<()> {
        // Save current conversation context before switching
        self.saved_conversation_context = Some(SavedConversationContext {
            conversation: self.conversation_manager.conversation.clone(),
            system_prompt: self.conversation_manager.system_prompt.clone(),
            current_conversation_id: self.conversation_manager.current_conversation_id.clone(),
            model: self.model.clone(),
        });

        // Set system prompt
        self.conversation_manager.system_prompt = Some(subagent_config.system_prompt.clone());

        // Set subagent name in conversation manager
        self.conversation_manager.subagent = Some(subagent_config.name.clone());

        // Update model if specified
        if let Some(ref new_model) = subagent_config.model {
            self.set_model_local(new_model.clone());
        }

        // Start a new conversation for the subagent
        let _ = self.conversation_manager.start_new_conversation().await;

        // Filter tools based on subagent configuration
        let mut tools = self.tools.write().await;
        self.filter_tools_for_subagent(&mut tools, subagent_config);

        Ok(())
    }

    fn filter_tools_for_subagent(
        &self,
        tools: &mut std::collections::HashMap<String, Tool>,
        config: &subagent::SubagentConfig,
    ) {
        // Remove denied tools
        for tool_name in &config.denied_tools {
            tools.remove(tool_name);
        }

        // If allowed_tools is not empty, keep only those
        if !config.allowed_tools.is_empty() {
            tools.retain(|name, _| config.allowed_tools.contains(name));
        }
    }

    pub async fn exit_subagent(&mut self) -> Result<()> {
        // Clear subagent field
        self.conversation_manager.subagent = None;

        // Restore saved conversation context if available
        if let Some(saved_context) = self.saved_conversation_context.take() {
            self.conversation_manager.conversation = saved_context.conversation;
            self.conversation_manager.system_prompt = saved_context.system_prompt;
            self.conversation_manager.current_conversation_id =
                saved_context.current_conversation_id;
            self.set_model_local(saved_context.model);
        } else {
            // Reset to default configuration if no saved context
            self.conversation_manager.system_prompt = None;
        }

        // Restore all tools
        let _ = self.force_refresh_mcp_tools().await;
        Ok(())
    }

    pub fn is_subagent_mode(&self) -> bool {
        self.conversation_manager.system_prompt.is_some()
            && !self
                .conversation_manager
                .system_prompt
                .as_ref()
                .unwrap_or(&String::new())
                .contains("default_system_prompt")
    }

    pub fn get_system_prompt(&self) -> Option<&String> {
        self.conversation_manager.system_prompt.as_ref()
    }

    pub async fn clear_conversation_keep_agents_md(&mut self) -> Result<()> {
        self.conversation_manager
            .clear_conversation_keep_agents_md()
            .await?;
        Ok(())
    }

    /// Execute a tool with the new display system
    async fn execute_tool_with_display(&self, call: &ToolCall) -> ToolResult {
        // Use the new display system
        let registry = self.tool_registry.read().await;
        let mut display = DisplayFactory::create_display(&call.name, &call.arguments, &registry);

        // Show tool call details
        display.show_call_details(&call.arguments);

        // Execute the tool using internal logic
        let result = self.execute_tool_internal(call).await;

        // Complete the display
        match &result {
            Ok(tool_result) => {
                if tool_result.is_error {
                    display.complete_error(&tool_result.content);
                } else {
                    display.complete_success(&tool_result.content);
                }
            }
            Err(e) => {
                display.complete_error(&e.to_string());
            }
        }

        result.unwrap_or_else(|e| ToolResult {
            tool_use_id: call.id.clone(),
            content: e.to_string(),
            is_error: true,
        })
    }

    /// Internal tool execution logic (shared between old and new display systems)
    async fn execute_tool_internal(&self, call: &ToolCall) -> Result<ToolResult> {
        // Handle MCP tools
        if call.name.starts_with("mcp_") {
            if let Some(mcp_manager) = &self.mcp_manager {
                // Extract server name and tool name from the call
                let parts: Vec<&str> = call.name.splitn(3, '_').collect();
                if parts.len() >= 3 {
                    let server_name = parts[1];
                    let tool_name = parts[2..].join("_");

                    match mcp_manager
                        .call_tool(server_name, &tool_name, Some(call.arguments.clone()))
                        .await
                    {
                        Ok(result) => {
                            debug!("MCP tool '{}' executed successfully", call.name);
                            let result_content = serde_json::to_string_pretty(&result)
                                .unwrap_or_else(|_| "Invalid JSON result".to_string());

                            Ok(ToolResult {
                                tool_use_id: call.id.clone(),
                                content: result_content,
                                is_error: false,
                            })
                        }
                        Err(e) => {
                            error!("Error executing MCP tool '{}': {}", call.name, e);
                            error!(
                                "MCP server '{}' may have encountered an error or is unavailable",
                                server_name
                            );

                            // Provide detailed error information
                            let error_content = format!(
                                "MCP tool call failed: {}. \n\
                                Tool: {}\n\
                                Server: {}\n\
                                Arguments: {}\n\
                                Please check:\n\
                                1. MCP server '{}' is running\n\
                                2. Server is responsive\n\
                                3. Tool arguments are correct\n\
                                4. Server has proper permissions",
                                e,
                                call.name,
                                server_name,
                                serde_json::to_string_pretty(&call.arguments)
                                    .unwrap_or_else(|_| "Invalid JSON".to_string()),
                                server_name
                            );

                            Ok(ToolResult {
                                tool_use_id: call.id.clone(),
                                content: error_content,
                                is_error: true,
                            })
                        }
                    }
                } else {
                    let error_content = format!("Invalid MCP tool name format: {}", call.name);
                    Ok(ToolResult {
                        tool_use_id: call.id.clone(),
                        content: error_content,
                        is_error: true,
                    })
                }
            } else {
                let error_content = "MCP manager not available. MCP tools cannot be executed without proper initialization.";
                Ok(ToolResult {
                    tool_use_id: call.id.clone(),
                    content: error_content.to_string(),
                    is_error: true,
                })
            }
        } else if call.name == "bash" {
            // Handle bash tool with security
            let security_manager = self.bash_security_manager.clone();
            let call_clone = call.clone();

            // We need to get a mutable reference to the security manager
            let mut manager = security_manager.write().await;
            let result = bash(&call_clone, &mut *manager, self.yolo_mode).await;
            drop(manager); // Explicitly drop the lock guard
            result
        } else if call.name == "write_file" {
            // Handle write_file tool with security
            let file_security_manager = self.file_security_manager.clone();
            let call_clone = call.clone();

            let mut manager = file_security_manager.write().await;
            let result = write_file(&call_clone, &mut *manager, self.yolo_mode).await;
            drop(manager); // Explicitly drop the lock guard
            result
        } else if call.name == "edit_file" {
            // Handle edit_file tool with security
            let file_security_manager = self.file_security_manager.clone();
            let call_clone = call.clone();

            let mut manager = file_security_manager.write().await;
            let result = edit_file(&call_clone, &mut *manager, self.yolo_mode).await;
            drop(manager); // Explicitly drop the lock guard
            result
        } else if call.name == "delete_file" {
            // Handle delete_file tool with security
            let file_security_manager = self.file_security_manager.clone();
            let call_clone = call.clone();

            let mut manager = file_security_manager.write().await;
            let result = delete_file(&call_clone, &mut *manager, self.yolo_mode).await;
            drop(manager); // Explicitly drop the lock guard
            result
        } else if call.name == "create_directory" {
            // Handle create_directory tool with security
            let file_security_manager = self.file_security_manager.clone();
            let call_clone = call.clone();

            let mut manager = file_security_manager.write().await;
            let result = create_directory(&call_clone, &mut *manager, self.yolo_mode).await;
            drop(manager); // Explicitly drop the lock guard
            result
        } else if call.name == "glob" {
            // Handle glob tool (read-only, no security needed)
            crate::tools::glob::glob_files(&call).await
        } else if let Some(tool) = {
            let tools = self.tools.read().await;
            tools.get(&call.name).cloned()
        } {
            // Execute the tool with its handler
            (tool.handler)(call.clone()).await
        } else {
            let error_content = format!("Unknown tool: {}", call.name);
            Ok(ToolResult {
                tool_use_id: call.id.clone(),
                content: error_content,
                is_error: true,
            })
        }
    }
}


