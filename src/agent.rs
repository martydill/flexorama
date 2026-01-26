use crate::mcp::McpManager;
use crate::security::{BashSecurityManager, FileSecurityManager};
use crate::tools::display::DisplayFactory;
use anyhow::{anyhow, Result};
use colored::*;
use log::{debug, error, info, warn};
use serde::Serialize;
use serde_json::json;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::{Mutex as AsyncMutex, RwLock};

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
use crate::hooks::{HookAction, HookManager};
use crate::llm::LlmClient;
use crate::subagent;
use crate::tools::{
    bash, create_directory, delete_file, edit_file, get_builtin_tools, write_file, Tool, ToolCall,
    ToolRegistry, ToolResult,
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
    // Skill management
    skill_manager: Option<Arc<AsyncMutex<crate::skill::SkillManager>>>,
    active_skills: Vec<String>,
    // Todo management
    todos: Arc<AsyncMutex<Vec<crate::tools::create_todo::TodoItem>>>,
    todos_by_conversation:
        Arc<AsyncMutex<HashMap<String, Vec<crate::tools::create_todo::TodoItem>>>>,
    // Available models for the current provider
    available_models: Arc<RwLock<Vec<String>>>,
    // Suppress output (for ACP mode where stdout must be clean)
    suppress_output: bool,
    hook_manager: Option<HookManager>,
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

        // Initialize todo list
        let todos = Arc::new(AsyncMutex::new(Vec::new()));
        let todos_by_conversation = Arc::new(AsyncMutex::new(HashMap::new()));

        // Initialize available models with defaults from config
        let default_models = crate::config::provider_models(config.provider)
            .iter()
            .map(|s| s.to_string())
            .collect();
        let available_models = Arc::new(RwLock::new(default_models));

        let hook_manager = match HookManager::load() {
            Ok(manager) => manager,
            Err(err) => {
                warn!("Failed to load Flexorama hooks: {}", err);
                None
            }
        };

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
            skill_manager: None,
            active_skills: Vec::new(),
            todos,
            todos_by_conversation,
            available_models,
            suppress_output: false,
            hook_manager,
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

        // Add core tools (bash and file operations) with security
        if !plan_mode {
            let mut tools = agent.tools.write().await;
            Self::add_core_tools(
                &mut tools,
                agent.bash_security_manager.clone(),
                agent.file_security_manager.clone(),
                yolo_mode,
                false, // Don't check existence, replace all
            );
        }

        // Add use_skill tool for progressive disclosure
        {
            let mut tools = agent.tools.write().await;
            tools.insert(
                "use_skill".to_string(),
                Tool {
                    name: "use_skill".to_string(),
                    description: "Load the full content of an active skill to access its detailed instructions and guidelines. Use this when you need the complete skill knowledge to help with the user's request.".to_string(),
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "name": {
                                "type": "string",
                                "description": "The name of the skill to load (must be an active skill)"
                            }
                        },
                        "required": ["name"]
                    }),
                    handler: Box::new(|call: ToolCall| {
                        Box::pin(async move {
                            // This is a placeholder - actual execution happens in execute_tool_internal
                            Ok(ToolResult {
                                tool_use_id: call.id.clone(),
                                content: "Skill loading is handled internally".to_string(),
                                is_error: false,
                            })
                        })
                    }),
                    metadata: None,
                },
            );
        }

        // Apply plan mode filtering
        let _ = agent.apply_plan_mode_filtering().await;

        // Fetch Ollama models if using Ollama provider
        let _ = agent.fetch_ollama_models().await;

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

    /// Fetch available models from Ollama if using Ollama provider
    pub async fn fetch_ollama_models(&self) -> Result<()> {
        if self.provider == Provider::Ollama {
            // Try to fetch models from Ollama
            let ollama_client = crate::ollama::OllamaClient::new(
                String::new(), // API key not needed for local Ollama
                self.base_url.clone(),
            );

            match ollama_client.fetch_available_models().await {
                Ok(models) => {
                    let mut available = self.available_models.write().await;
                    *available = models;
                    debug!("Fetched {} models from Ollama", available.len());
                }
                Err(e) => {
                    warn!("Failed to fetch Ollama models: {}. Using defaults.", e);
                }
            }
        }
        Ok(())
    }

    /// Get list of available models for the current provider
    pub async fn get_available_models(&self) -> Vec<String> {
        self.available_models.read().await.clone()
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

    pub fn with_skill_manager(
        mut self,
        skill_manager: Arc<AsyncMutex<crate::skill::SkillManager>>,
    ) -> Self {
        self.skill_manager = Some(skill_manager);
        self
    }

    /// List all available skills
    pub async fn list_skills(&self) -> Result<Vec<crate::skill::Skill>> {
        if let Some(skill_manager) = &self.skill_manager {
            let manager = skill_manager.lock().await;
            Ok(manager
                .list_skills()
                .iter()
                .map(|skill| (*skill).clone())
                .collect())
        } else {
            Err(anyhow!("Skill manager not initialized"))
        }
    }

    /// Create a new skill
    pub async fn create_skill(&mut self, skill: crate::skill::Skill) -> Result<()> {
        if let Some(skill_manager) = &self.skill_manager {
            let mut manager = skill_manager.lock().await;
            manager.create_skill(skill).await?;
            Ok(())
        } else {
            Err(anyhow!("Skill manager not initialized"))
        }
    }

    /// Update an existing skill
    pub async fn update_skill(&mut self, skill: crate::skill::Skill) -> Result<()> {
        if let Some(skill_manager) = &self.skill_manager {
            let mut manager = skill_manager.lock().await;
            manager.update_skill(&skill).await?;
            Ok(())
        } else {
            Err(anyhow!("Skill manager not initialized"))
        }
    }

    /// Delete a skill
    pub async fn delete_skill(&mut self, name: &str) -> Result<()> {
        let is_active = self.active_skills.contains(&name.to_string());
        if is_active {
            self.deactivate_skill(name).await?;
        }

        if let Some(skill_manager) = &self.skill_manager {
            let mut manager = skill_manager.lock().await;
            manager.delete_skill(name).await?;
            Ok(())
        } else {
            Err(anyhow!("Skill manager not initialized"))
        }
    }

    /// Activate a skill
    pub async fn activate_skill(&mut self, name: &str) -> Result<()> {
        if let Some(skill_manager) = &self.skill_manager {
            let mut manager = skill_manager.lock().await;

            // Get the skill
            let skill = manager
                .get_skill(name)
                .ok_or_else(|| anyhow!("Skill '{}' not found", name))?
                .clone();

            // Activate in skill manager (this updates and saves config)
            manager.activate_skill(name).await?;

            // Add to active skills list
            if !self.active_skills.contains(&name.to_string()) {
                self.active_skills.push(name.to_string());
            }

            // Filter tools based on skill restrictions
            let mut tools = self.tools.write().await;

            // Remove denied tools
            for tool_name in &skill.denied_tools {
                tools.remove(tool_name);
            }

            // If allowed_tools is not empty, keep only those
            if !skill.allowed_tools.is_empty() {
                tools.retain(|name, _| skill.allowed_tools.contains(name));
            }

            info!("✅ Activated skill: {}", skill.name);
            Ok(())
        } else {
            Err(anyhow!("Skill manager not initialized"))
        }
    }

    /// Deactivate a skill
    pub async fn deactivate_skill(&mut self, name: &str) -> Result<()> {
        if let Some(skill_manager) = &self.skill_manager {
            {
                let mut manager = skill_manager.lock().await;

                // Deactivate in skill manager (this updates and saves config)
                manager.deactivate_skill(name).await?;
            } // Drop manager lock here

            // Remove from active skills list
            self.active_skills.retain(|s| s != name);

            // Restore original tools by refreshing from builtin and MCP
            let builtin_tools = crate::tools::builtin::get_builtin_tools();
            {
                let mut tools = self.tools.write().await;
                tools.clear();

                for tool in builtin_tools {
                    tools.insert(tool.name.clone(), tool);
                }
            } // Drop tools lock here

            // Refresh MCP tools
            self.force_refresh_mcp_tools().await?;

            info!("❌ Deactivated skill: {}", name);
            Ok(())
        } else {
            Err(anyhow!("Skill manager not initialized"))
        }
    }

    /// Get list of active skills
    pub fn get_active_skills(&self) -> &[String] {
        &self.active_skills
    }

    /// Helper method to add core tools (bash and file operations) to the tools map
    ///
    /// # Arguments
    /// * `tools` - Mutable reference to the tools HashMap
    /// * `bash_security_manager` - The bash security manager
    /// * `file_security_manager` - The file security manager
    /// * `yolo_mode` - Whether yolo mode is enabled
    /// * `check_exists` - If true, only add tools if they don't already exist
    fn add_core_tools(
        tools: &mut HashMap<String, crate::tools::Tool>,
        bash_security_manager: Arc<RwLock<BashSecurityManager>>,
        file_security_manager: Arc<RwLock<FileSecurityManager>>,
        yolo_mode: bool,
        check_exists: bool,
    ) {
        use crate::tools::{
            create_bash_tool, create_create_directory_tool, create_delete_file_tool,
            create_edit_file_tool, create_write_file_tool,
        };

        // Add bash tool
        if !check_exists || !tools.contains_key("bash") {
            let bash_tool = create_bash_tool(bash_security_manager, yolo_mode);
            tools.insert("bash".to_string(), bash_tool);
        }

        // Add file operation tools
        if !check_exists || !tools.contains_key("write_file") {
            let write_file_tool = create_write_file_tool(file_security_manager.clone(), yolo_mode);
            tools.insert("write_file".to_string(), write_file_tool);
        }

        if !check_exists || !tools.contains_key("edit_file") {
            let edit_file_tool = create_edit_file_tool(file_security_manager.clone(), yolo_mode);
            tools.insert("edit_file".to_string(), edit_file_tool);
        }

        if !check_exists || !tools.contains_key("delete_file") {
            let delete_file_tool =
                create_delete_file_tool(file_security_manager.clone(), yolo_mode);
            tools.insert("delete_file".to_string(), delete_file_tool);
        }

        if !check_exists || !tools.contains_key("create_directory") {
            let create_directory_tool =
                create_create_directory_tool(file_security_manager.clone(), yolo_mode);
            tools.insert("create_directory".to_string(), create_directory_tool);
        }
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

            // Add core tools (bash and file operations) with security
            Self::add_core_tools(
                &mut tools,
                self.bash_security_manager.clone(),
                self.file_security_manager.clone(),
                self.yolo_mode,
                false, // Don't check existence, replace all
            );

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
            Self::add_core_tools(
                &mut tools,
                self.bash_security_manager.clone(),
                self.file_security_manager.clone(),
                self.yolo_mode,
                true, // Check existence before adding
            );
            Ok(())
        }
    }

    /// Set the system prompt for the conversation
    pub fn set_system_prompt(&mut self, system_prompt: String) {
        self.conversation_manager.system_prompt = Some(system_prompt);
    }

    /// Set whether to suppress informational output (for ACP mode)
    pub fn set_suppress_output(&mut self, suppress: bool) {
        self.suppress_output = suppress;
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
                Self::add_core_tools(
                    &mut tools,
                    self.bash_security_manager.clone(),
                    self.file_security_manager.clone(),
                    self.yolo_mode,
                    true, // Check existence before adding
                );
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
                Ok(_) => info!("✓ Added context file: {}", file_path),
                Err(e) => app_eprintln!(
                    "{} Failed to add context file '{}': {}",
                    "✗".red(),
                    file_path,
                    e
                ),
            }
        }

        // Clean message by removing @file syntax
        let mut cleaned_message = self.clean_message(message);

        if let Some(hook_manager) = &self.hook_manager {
            let hook_decision = hook_manager
                .run_pre_message(
                    &cleaned_message,
                    message,
                    &context_files,
                    self.conversation_manager.current_conversation_id.as_deref(),
                    &self.model,
                )
                .await?;
            if hook_decision.action == HookAction::Abort {
                let reason = hook_decision
                    .message
                    .unwrap_or_else(|| "Hook aborted the request.".to_string());
                return Err(anyhow!(reason));
            }
            if let Some(updated) = hook_decision.updated_message {
                cleaned_message = updated;
            }
        }

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

        // Inject active skills into system prompt
        if let Some(skill_manager) = &self.skill_manager {
            let manager = skill_manager.lock().await;
            let skills_content = manager.get_active_skills_content();

            if !skills_content.is_empty() {
                // Prepend skills to system prompt
                let current_prompt = self
                    .conversation_manager
                    .system_prompt
                    .clone()
                    .unwrap_or_default();
                let enhanced_prompt = format!("{}\n\n{}", skills_content, current_prompt);
                self.conversation_manager.system_prompt = Some(enhanced_prompt);
            }
        }

        let mut final_response = String::new();
        let mut final_response_tokens: Option<i32> = None;
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
            let response_total_tokens = response
                .usage
                .as_ref()
                .map(|u| (u.input_tokens + u.output_tokens) as i32)
                .unwrap_or(0);
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
                if on_stream_content.is_none() && !self.suppress_output {
                    // Only print if not streaming and output not suppressed (ACP mode)
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
                final_response_tokens = Some(response_total_tokens);

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

                    let mut call_to_run = call.clone();

                    if let Some(hook_manager) = &self.hook_manager {
                        let hook_decision = hook_manager
                            .run_pre_tool(
                                &call.id,
                                &call.name,
                                &call_to_run.arguments,
                                self.conversation_manager.current_conversation_id.as_deref(),
                                &self.model,
                            )
                            .await?;
                        if hook_decision.action == HookAction::Abort {
                            let reason = hook_decision
                                .message
                                .unwrap_or_else(|| "Hook aborted tool execution.".to_string());
                            return Err(anyhow!(reason));
                        }
                        if let Some(updated_arguments) = hook_decision.updated_arguments {
                            call_to_run.arguments = updated_arguments;
                        }
                    }

                    debug!(
                        "Executing tool: {} with ID: {}",
                        call_to_run.name, call_to_run.id
                    );

                    if let (Some(db), Some(conversation_id)) = (
                        self.conversation_manager.database_manager.clone(),
                        self.conversation_manager.current_conversation_id.clone(),
                    ) {
                        let args_str = serde_json::to_string(&call_to_run.arguments)
                            .unwrap_or_else(|_| call_to_run.arguments.to_string());
                        if let Err(e) = db
                            .add_tool_call(
                                &conversation_id,
                                None,
                                &call_to_run.id,
                                &call_to_run.name,
                                &args_str,
                            )
                            .await
                        {
                            warn!("Failed to record tool call {}: {}", call_to_run.name, e);
                        }
                    }

                    if let Some(callback) = &on_tool_event {
                        callback(StreamToolEvent {
                            event: "tool_call".to_string(),
                            tool_use_id: call_to_run.id.clone(),
                            name: call_to_run.name.clone(),
                            input: Some(call_to_run.arguments.clone()),
                            content: None,
                            is_error: None,
                        });
                    }

                    // Handle plan mode restrictions
                    if self.plan_mode {
                        let registry = self.tool_registry.read().await;
                        if !registry.is_readonly(&call_to_run.name) {
                            let error_content = format!(
                                "Plan mode is read-only. Tool '{}' is disabled.",
                                call_to_run.name
                            );
                            results.push(ToolResult {
                                tool_use_id: call_to_run.id.clone(),
                                content: error_content,
                                is_error: true,
                            });
                            continue;
                        }
                    }

                    // Use the new display system and execute tool
                    let result = self.execute_tool_with_display(&call_to_run).await;
                    if let (Some(db), Some(_conversation_id)) = (
                        self.conversation_manager.database_manager.clone(),
                        self.conversation_manager.current_conversation_id.clone(),
                    ) {
                        if let Err(e) = db
                            .complete_tool_call(&call_to_run.id, &result.content, result.is_error)
                            .await
                        {
                            warn!("Failed to record tool result {}: {}", call_to_run.name, e);
                        }
                    }
                    if let Some(callback) = &on_tool_event {
                        callback(StreamToolEvent {
                            event: "tool_result".to_string(),
                            tool_use_id: call_to_run.id.clone(),
                            name: call_to_run.name.clone(),
                            input: None,
                            content: Some(result.content.clone()),
                            is_error: Some(result.is_error),
                        });
                    }

                    if let Some(hook_manager) = &self.hook_manager {
                        let hook_decision = hook_manager
                            .run_post_tool(
                                &call_to_run.id,
                                &call_to_run.name,
                                &call_to_run.arguments,
                                &result.content,
                                result.is_error,
                                self.conversation_manager.current_conversation_id.as_deref(),
                                &self.model,
                            )
                            .await?;
                        if hook_decision.action == HookAction::Abort {
                            let reason = hook_decision.message.unwrap_or_else(|| {
                                "Hook aborted after tool execution.".to_string()
                            });
                            return Err(anyhow!(reason));
                        }
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
                    response_total_tokens,
                )
                .await
            {
                warn!("Failed to save assistant message to database: {}", e);
            }

            // Add tool results to conversation
            for result in tool_results {
                self.conversation_manager.conversation.push(Message {
                    role: "user".to_string(),
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
                .save_message_to_conversation(
                    "assistant",
                    &final_response,
                    final_response_tokens.unwrap_or(0),
                )
                .await
            {
                warn!("Failed to save final assistant message to database: {}", e);
            }
        }
        if let Some(hook_manager) = &self.hook_manager {
            let hook_decision = hook_manager
                .run_post_message(
                    &final_response,
                    self.conversation_manager.current_conversation_id.as_deref(),
                    &self.model,
                )
                .await?;
            if hook_decision.action == HookAction::Abort {
                let reason = hook_decision
                    .message
                    .unwrap_or_else(|| "Hook aborted after response.".to_string());
                return Err(anyhow!(reason));
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

    /// Get the current todo list (for TUI display)
    pub async fn get_todos(&self) -> Vec<crate::tools::create_todo::TodoItem> {
        self.get_todos_for(None).await
    }

    pub async fn get_todos_for(
        &self,
        conversation_id: Option<&str>,
    ) -> Vec<crate::tools::create_todo::TodoItem> {
        let key = self.todo_conversation_key(conversation_id);
        let map = self.todos_by_conversation.lock().await;
        if let Some(todos) = map.get(&key) {
            return todos.clone();
        }
        drop(map);
        if conversation_id.is_none()
            && Some(key.as_str()) == self.conversation_manager.current_conversation_id.as_deref()
        {
            let todos = self.todos.lock().await;
            return todos.clone();
        }
        Vec::new()
    }

    pub fn todos_handle(&self) -> Arc<AsyncMutex<Vec<crate::tools::create_todo::TodoItem>>> {
        Arc::clone(&self.todos)
    }

    pub async fn clear_todos_for_current_conversation(&self) {
        {
            let mut todos = self.todos.lock().await;
            todos.clear();
        }
        self.store_todos_for_current_conversation(Vec::new()).await;
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
        if !self.suppress_output {
            app_println!("{}", "LLM Provider".cyan().bold());
            app_println!("  Provider: {}", self.provider);
            app_println!("  Model: {}", self.model);
            app_println!("  Base URL: {}", self.base_url);
        }
    }

    /// Get the active LLM provider
    pub fn provider(&self) -> Provider {
        self.provider
    }

    /// Get the active model name
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Get the current plan mode state
    pub fn plan_mode(&self) -> bool {
        self.plan_mode
    }

    /// Get the current yolo mode state
    pub fn yolo_mode(&self) -> bool {
        self.yolo_mode
    }

    /// Update the active model for this session
    pub async fn set_model(&mut self, model: String) -> Result<()> {
        self.model = model.clone();
        self.conversation_manager
            .update_conversation_model(model)
            .await
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
            skills: crate::config::SkillConfig::default(),
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

    fn todo_conversation_key(&self, conversation_id: Option<&str>) -> String {
        conversation_id
            .map(|id| id.to_string())
            .or_else(|| self.conversation_manager.current_conversation_id.clone())
            .unwrap_or_else(|| "default".to_string())
    }

    async fn sync_todos_for_current_conversation(&self) {
        let key = self.todo_conversation_key(None);
        let snapshot = {
            let mut map = self.todos_by_conversation.lock().await;
            map.entry(key).or_default().clone()
        };
        let mut todos = self.todos.lock().await;
        *todos = snapshot;
    }

    async fn store_todos_for_current_conversation(
        &self,
        todos: Vec<crate::tools::create_todo::TodoItem>,
    ) {
        let key = self.todo_conversation_key(None);
        let mut map = self.todos_by_conversation.lock().await;
        map.insert(key, todos);
    }

    /// Get the currently active subagent name (if any)
    pub fn active_subagent_name(&self) -> Option<String> {
        self.conversation_manager.subagent.clone()
    }

    /// Get the number of messages in the active conversation
    pub fn conversation_len(&self) -> usize {
        self.conversation_manager.conversation.len()
    }

    /// Add an image to the conversation
    pub fn add_image(
        &mut self,
        media_type: String,
        base64_data: String,
        description: Option<String>,
    ) {
        let mut content = vec![ContentBlock::image(media_type, base64_data)];

        if let Some(desc) = description {
            content.push(ContentBlock::text(desc));
        }

        self.conversation_manager.conversation.push(Message {
            role: "user".to_string(),
            content,
        });
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
        self.sync_todos_for_current_conversation().await;

        Ok(())
    }

    /// Start a new conversation
    pub async fn start_new_conversation(&mut self) -> Result<String> {
        let id = self.conversation_manager.start_new_conversation().await?;
        self.sync_todos_for_current_conversation().await;
        Ok(id)
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
        self.sync_todos_for_current_conversation().await;
        self.sync_todos_for_current_conversation().await;

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
        self.sync_todos_for_current_conversation().await;

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
        self.sync_todos_for_current_conversation().await;
        Ok(())
    }

    /// Execute a tool with the new display system
    async fn execute_tool_with_display(&self, call: &ToolCall) -> ToolResult {
        if is_todo_tool(&call.name) {
            return self
                .execute_tool_internal(call)
                .await
                .unwrap_or_else(|e| ToolResult {
                    tool_use_id: call.id.clone(),
                    content: e.to_string(),
                    is_error: true,
                });
        }

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
        } else if call.name == "use_skill" {
            // Handle use_skill tool for progressive disclosure
            if let Some(skill_manager) = &self.skill_manager {
                let skill_name = call
                    .arguments
                    .get("name")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| anyhow!("Missing 'name' parameter for use_skill"))?;

                let manager = skill_manager.lock().await;
                match manager.get_skill_full_content(skill_name) {
                    Ok(content) => {
                        info!("Loaded full content for skill: {}", skill_name);
                        Ok(ToolResult {
                            tool_use_id: call.id.clone(),
                            content,
                            is_error: false,
                        })
                    }
                    Err(e) => {
                        error!("Failed to load skill '{}': {}", skill_name, e);
                        Ok(ToolResult {
                            tool_use_id: call.id.clone(),
                            content: format!("Error loading skill '{}': {}", skill_name, e),
                            is_error: true,
                        })
                    }
                }
            } else {
                Ok(ToolResult {
                    tool_use_id: call.id.clone(),
                    content: "Skill manager not available. Skills cannot be loaded.".to_string(),
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
        } else if call.name == "create_todo" {
            // Handle create_todo tool
            let mut todos = self.todos.lock().await;
            let result = crate::tools::create_todo::create_todo(call, &mut todos).await;
            let snapshot = todos.clone();
            drop(todos);
            self.store_todos_for_current_conversation(snapshot).await;
            result
        } else if call.name == "complete_todo" {
            // Handle complete_todo tool
            let mut todos = self.todos.lock().await;
            let result = crate::tools::complete_todo::complete_todo(call, &mut todos).await;
            let snapshot = todos.clone();
            drop(todos);
            self.store_todos_for_current_conversation(snapshot).await;
            result
        } else if call.name == "list_todos" {
            // Handle list_todos tool
            let todos = self.todos.lock().await;
            crate::tools::list_todos::list_todos(call, &todos).await
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

fn is_todo_tool(tool_name: &str) -> bool {
    matches!(tool_name, "create_todo" | "complete_todo" | "list_todos")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_usage_tracks_and_resets() {
        let mut usage_tracker = TokenUsage::new();
        let usage = Usage {
            input_tokens: 12,
            output_tokens: 34,
        };

        usage_tracker.add_usage(&usage);

        assert_eq!(usage_tracker.request_count, 1);
        assert_eq!(usage_tracker.total_input_tokens, 12);
        assert_eq!(usage_tracker.total_output_tokens, 34);
        assert_eq!(usage_tracker.total_tokens(), 46);

        usage_tracker.reset();

        assert_eq!(usage_tracker.request_count, 0);
        assert_eq!(usage_tracker.total_input_tokens, 0);
        assert_eq!(usage_tracker.total_output_tokens, 0);
        assert_eq!(usage_tracker.total_tokens(), 0);
    }

    #[tokio::test]
    async fn new_with_plan_mode_controls_bash_tool() {
        let config = Config::default();

        let agent_with_bash =
            Agent::new_with_plan_mode(config.clone(), "test-model".to_string(), false, false).await;
        let tools_with_bash = agent_with_bash.tools.read().await;
        assert!(tools_with_bash.contains_key("bash"));

        let agent_without_bash =
            Agent::new_with_plan_mode(config, "test-model".to_string(), false, true).await;
        let tools_without_bash = agent_without_bash.tools.read().await;
        assert!(!tools_without_bash.contains_key("bash"));
    }

    #[test]
    fn snapshot_conversation_includes_seeded_state() {
        let mut agent = Agent::new(
            Config::default(),
            "snapshot-model".to_string(),
            false,
            false,
        );

        agent.conversation_manager.conversation = vec![
            Message {
                role: "user".to_string(),
                content: vec![ContentBlock::text("Hello".to_string())],
            },
            Message {
                role: "assistant".to_string(),
                content: vec![ContentBlock::text("Hi there".to_string())],
            },
        ];
        agent.conversation_manager.current_conversation_id = Some("conv-123".to_string());
        agent.conversation_manager.system_prompt = Some("System prompt".to_string());
        agent.conversation_manager.model = "snapshot-model".to_string();

        let snapshot = agent.snapshot_conversation();

        assert_eq!(snapshot.id.as_deref(), Some("conv-123"));
        assert_eq!(snapshot.system_prompt.as_deref(), Some("System prompt"));
        assert_eq!(snapshot.model, "snapshot-model");
        assert_eq!(snapshot.messages.len(), 2);
        assert_eq!(snapshot.messages[0].role, "user");
        assert_eq!(
            snapshot.messages[0].content[0].text.as_deref(),
            Some("Hello")
        );
        assert_eq!(snapshot.messages[1].role, "assistant");
        assert_eq!(
            snapshot.messages[1].content[0].text.as_deref(),
            Some("Hi there")
        );
    }

    #[test]
    fn agent_new_initializes_with_defaults() {
        let config = Config::default();
        let agent = Agent::new(config.clone(), "test-model".to_string(), false, false);

        assert_eq!(agent.model, "test-model");
        assert_eq!(agent.provider, config.provider);
        assert_eq!(agent.base_url, config.base_url);
        assert!(!agent.yolo_mode);
        assert!(!agent.plan_mode);
        assert!(agent.mcp_manager.is_none());
        assert_eq!(agent.last_mcp_tools_version, 0);
        assert!(agent.active_skills.is_empty());
        assert!(agent.saved_conversation_context.is_none());
        assert_eq!(agent.token_usage.request_count, 0);
    }

    #[test]
    fn agent_new_with_yolo_mode() {
        let config = Config::default();
        let agent = Agent::new(config, "test-model".to_string(), true, false);

        assert!(agent.yolo_mode);
        assert!(!agent.plan_mode);
    }

    #[test]
    fn set_system_prompt_updates_conversation_manager() {
        let config = Config::default();
        let mut agent = Agent::new(config, "test-model".to_string(), false, false);

        // Config::default() sets a default system prompt, so it's not None initially
        assert!(agent.conversation_manager.system_prompt.is_some());

        agent.set_system_prompt("Custom system prompt".to_string());

        assert_eq!(
            agent.conversation_manager.system_prompt.as_deref(),
            Some("Custom system prompt")
        );
    }

    #[test]
    fn apply_plan_mode_prompt_preserves_existing_context() {
        let config = Config::default();
        let mut agent = Agent::new(config, "test-model".to_string(), false, true);

        agent.set_system_prompt("Existing prompt".to_string());
        agent.apply_plan_mode_prompt();

        let prompt = agent.conversation_manager.system_prompt.unwrap();
        assert!(prompt.contains("You are operating in plan mode"));
        assert!(prompt.contains("Existing prompt"));
        assert!(agent.plan_mode_saved_system_prompt.is_some());
    }

    #[test]
    fn apply_plan_mode_prompt_saves_original() {
        let config = Config::default();
        let mut agent = Agent::new(config, "test-model".to_string(), false, true);

        agent.set_system_prompt("Original prompt".to_string());
        agent.apply_plan_mode_prompt();

        assert_eq!(
            agent.plan_mode_saved_system_prompt,
            Some(Some("Original prompt".to_string()))
        );
    }

    #[test]
    fn derive_plan_title_from_first_heading() {
        let markdown = "# My Great Plan\n\nThis is the plan content.";
        let title = Agent::derive_plan_title(markdown);
        assert_eq!(title.as_deref(), Some("My Great Plan"));
    }

    #[test]
    fn derive_plan_title_from_multiple_hashes() {
        let markdown = "## Secondary Heading\n\nContent here.";
        let title = Agent::derive_plan_title(markdown);
        assert_eq!(title.as_deref(), Some("Secondary Heading"));
    }

    #[test]
    fn derive_plan_title_returns_none_for_no_heading() {
        let markdown = "Just some text without headings.";
        let title = Agent::derive_plan_title(markdown);
        assert_eq!(title, None);
    }

    #[test]
    fn derive_plan_title_ignores_empty_headings() {
        let markdown = "#\n## \n### Actual Title\n\nContent.";
        let title = Agent::derive_plan_title(markdown);
        assert_eq!(title.as_deref(), Some("Actual Title"));
    }

    #[test]
    fn extract_context_files_from_message() {
        let config = Config::default();
        let agent = Agent::new(config, "test-model".to_string(), false, false);

        let message = "Please read @file1.txt and @dir/file2.rs for context";
        let files = agent.extract_context_files(message);

        assert_eq!(files.len(), 2);
        assert!(files.contains(&"file1.txt".to_string()));
        assert!(files.contains(&"dir/file2.rs".to_string()));
    }

    #[test]
    fn extract_context_files_handles_multiple_at_symbols() {
        let config = Config::default();
        let agent = Agent::new(config, "test-model".to_string(), false, false);

        let message = "@file1.txt @file2.txt @file3.txt";
        let files = agent.extract_context_files(message);

        assert_eq!(files.len(), 3);
    }

    #[test]
    fn clean_message_removes_at_file_syntax() {
        let config = Config::default();
        let agent = Agent::new(config, "test-model".to_string(), false, false);

        let message = "Read @file1.txt and explain it";
        let cleaned = agent.clean_message(message);

        assert!(!cleaned.contains("@file1.txt"));
        assert!(cleaned.contains("Read") && cleaned.contains("and explain it"));
    }

    #[test]
    fn clean_message_with_only_files() {
        let config = Config::default();
        let agent = Agent::new(config, "test-model".to_string(), false, false);

        let message = "@file1.txt @file2.txt";
        let cleaned = agent.clean_message(message);

        // The clean_message function removes @file references
        // Check that file references are not present in the cleaned message
        assert!(!cleaned.contains("@file1.txt"));
        assert!(!cleaned.contains("@file2.txt"));
    }

    #[test]
    fn token_usage_accumulates_multiple_requests() {
        let mut usage_tracker = TokenUsage::new();

        usage_tracker.add_usage(&Usage {
            input_tokens: 10,
            output_tokens: 20,
        });
        usage_tracker.add_usage(&Usage {
            input_tokens: 15,
            output_tokens: 25,
        });

        assert_eq!(usage_tracker.request_count, 2);
        assert_eq!(usage_tracker.total_input_tokens, 25);
        assert_eq!(usage_tracker.total_output_tokens, 45);
        assert_eq!(usage_tracker.total_tokens(), 70);
    }

    #[test]
    fn get_token_usage_returns_reference() {
        let config = Config::default();
        let mut agent = Agent::new(config, "test-model".to_string(), false, false);

        agent.token_usage.add_usage(&Usage {
            input_tokens: 100,
            output_tokens: 200,
        });

        let usage = agent.get_token_usage();
        assert_eq!(usage.total_input_tokens, 100);
        assert_eq!(usage.total_output_tokens, 200);
    }

    #[test]
    fn reset_token_usage_clears_all_counts() {
        let config = Config::default();
        let mut agent = Agent::new(config, "test-model".to_string(), false, false);

        agent.token_usage.add_usage(&Usage {
            input_tokens: 100,
            output_tokens: 200,
        });
        agent.reset_token_usage();

        assert_eq!(agent.token_usage.request_count, 0);
        assert_eq!(agent.token_usage.total_input_tokens, 0);
        assert_eq!(agent.token_usage.total_output_tokens, 0);
    }

    #[test]
    fn provider_returns_correct_value() {
        let config = Config::default();
        let agent = Agent::new(config.clone(), "test-model".to_string(), false, false);

        assert_eq!(agent.provider(), config.provider);
    }

    #[test]
    fn model_returns_current_model() {
        let config = Config::default();
        let agent = Agent::new(config, "my-model".to_string(), false, false);

        assert_eq!(agent.model(), "my-model");
    }

    #[test]
    fn plan_mode_returns_current_state() {
        let config = Config::default();
        let agent_normal = Agent::new(config.clone(), "test-model".to_string(), false, false);
        let agent_plan = Agent::new(config, "test-model".to_string(), false, true);

        assert!(!agent_normal.plan_mode());
        assert!(agent_plan.plan_mode());
    }

    #[test]
    fn set_model_local_updates_both_agent_and_conversation() {
        let config = Config::default();
        let mut agent = Agent::new(config, "old-model".to_string(), false, false);

        agent.set_model_local("new-model".to_string());

        assert_eq!(agent.model, "new-model");
        assert_eq!(agent.conversation_manager.model, "new-model");
    }

    #[test]
    fn conversation_len_returns_message_count() {
        let config = Config::default();
        let mut agent = Agent::new(config, "test-model".to_string(), false, false);

        agent.conversation_manager.conversation.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::text("Hello".to_string())],
        });
        agent.conversation_manager.conversation.push(Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::text("Hi".to_string())],
        });

        assert_eq!(agent.conversation_len(), 2);
    }

    #[test]
    fn current_conversation_id_returns_none_initially() {
        let config = Config::default();
        let agent = Agent::new(config, "test-model".to_string(), false, false);

        assert_eq!(agent.current_conversation_id(), None);
    }

    #[test]
    fn active_subagent_name_returns_none_initially() {
        let config = Config::default();
        let agent = Agent::new(config, "test-model".to_string(), false, false);

        assert_eq!(agent.active_subagent_name(), None);
    }

    #[test]
    fn get_system_prompt_returns_default_initially() {
        let config = Config::default();
        let agent = Agent::new(config, "test-model".to_string(), false, false);

        // Config::default() sets a default system prompt
        assert!(agent.get_system_prompt().is_some());
    }

    #[test]
    fn get_system_prompt_returns_set_value() {
        let config = Config::default();
        let mut agent = Agent::new(config, "test-model".to_string(), false, false);

        agent.set_system_prompt("Test prompt".to_string());

        assert_eq!(agent.get_system_prompt(), Some(&"Test prompt".to_string()));
    }

    #[test]
    fn database_manager_returns_none_when_not_configured() {
        let config = Config::default();
        let agent = Agent::new(config, "test-model".to_string(), false, false);

        assert!(agent.database_manager().is_none());
    }

    #[test]
    fn get_active_skills_returns_empty_initially() {
        let config = Config::default();
        let agent = Agent::new(config, "test-model".to_string(), false, false);

        assert_eq!(agent.get_active_skills().len(), 0);
    }

    #[tokio::test]
    async fn set_plan_mode_to_same_value_is_noop() {
        let config = Config::default();
        let mut agent =
            Agent::new_with_plan_mode(config, "test-model".to_string(), false, true).await;

        let result = agent.set_plan_mode(true).await;

        assert!(result.is_ok());
        assert!(agent.plan_mode);
    }

    #[tokio::test]
    async fn set_plan_mode_enabling_filters_tools() {
        let config = Config::default();
        let mut agent =
            Agent::new_with_plan_mode(config, "test-model".to_string(), false, false).await;

        let tools_before = agent.tools.read().await;
        let has_bash_before = tools_before.contains_key("bash");
        drop(tools_before);

        assert!(has_bash_before);

        agent.set_plan_mode(true).await.unwrap();

        let tools_after = agent.tools.read().await;
        let has_bash_after = tools_after.contains_key("bash");
        drop(tools_after);

        assert!(!has_bash_after);
        assert!(agent.plan_mode);
    }

    #[tokio::test]
    async fn set_plan_mode_disabling_restores_tools() {
        let config = Config::default();
        let mut agent =
            Agent::new_with_plan_mode(config, "test-model".to_string(), false, true).await;

        let tools_before = agent.tools.read().await;
        let has_bash_before = tools_before.contains_key("bash");
        drop(tools_before);

        assert!(!has_bash_before);

        agent.set_plan_mode(false).await.unwrap();

        let tools_after = agent.tools.read().await;
        let has_bash_after = tools_after.contains_key("bash");
        drop(tools_after);

        assert!(has_bash_after);
        assert!(!agent.plan_mode);
    }

    #[tokio::test]
    async fn set_plan_mode_restores_saved_system_prompt() {
        let config = Config::default();
        let mut agent =
            Agent::new_with_plan_mode(config, "test-model".to_string(), false, false).await;

        agent.set_system_prompt("Original prompt".to_string());
        agent.set_plan_mode(true).await.unwrap();

        let plan_prompt = agent.conversation_manager.system_prompt.clone();
        assert!(plan_prompt.unwrap().contains("plan mode"));

        agent.set_plan_mode(false).await.unwrap();

        assert_eq!(
            agent.conversation_manager.system_prompt.as_deref(),
            Some("Original prompt")
        );
    }

    #[tokio::test]
    async fn filter_tools_for_subagent_removes_denied() {
        use std::collections::HashSet;
        let config = Config::default();
        let agent = Agent::new(config, "test-model".to_string(), false, false);

        let mut tools = get_builtin_tools()
            .into_iter()
            .map(|t| (t.name.clone(), t))
            .collect();

        let mut denied = HashSet::new();
        denied.insert("read_file".to_string());

        let subagent_config = subagent::SubagentConfig {
            name: "test-subagent".to_string(),
            system_prompt: "Test prompt".to_string(),
            model: None,
            allowed_tools: HashSet::new(),
            denied_tools: denied,
            max_tokens: None,
            temperature: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        agent.filter_tools_for_subagent(&mut tools, &subagent_config);

        assert!(!tools.contains_key("read_file"));
    }

    #[tokio::test]
    async fn filter_tools_for_subagent_keeps_only_allowed() {
        use std::collections::HashSet;
        let config = Config::default();
        let agent = Agent::new(config, "test-model".to_string(), false, false);

        let mut tools = get_builtin_tools()
            .into_iter()
            .map(|t| (t.name.clone(), t))
            .collect();

        let mut allowed = HashSet::new();
        allowed.insert("read_file".to_string());
        allowed.insert("list_directory".to_string());

        let subagent_config = subagent::SubagentConfig {
            name: "test-subagent".to_string(),
            system_prompt: "Test prompt".to_string(),
            model: None,
            allowed_tools: allowed,
            denied_tools: HashSet::new(),
            max_tokens: None,
            temperature: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        agent.filter_tools_for_subagent(&mut tools, &subagent_config);

        assert_eq!(tools.len(), 2);
        assert!(tools.contains_key("read_file"));
        assert!(tools.contains_key("list_directory"));
    }

    #[tokio::test]
    async fn switch_to_subagent_saves_context() {
        use std::collections::HashSet;
        let config = Config::default();
        let mut agent = Agent::new(config, "test-model".to_string(), false, false);

        agent.conversation_manager.conversation.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::text("Before switch".to_string())],
        });
        agent.set_system_prompt("Original system".to_string());
        agent.conversation_manager.current_conversation_id = Some("conv-1".to_string());

        let subagent_config = subagent::SubagentConfig {
            name: "subagent-1".to_string(),
            system_prompt: "Subagent prompt".to_string(),
            model: Some("subagent-model".to_string()),
            allowed_tools: HashSet::new(),
            denied_tools: HashSet::new(),
            max_tokens: None,
            temperature: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        agent.switch_to_subagent(&subagent_config).await.unwrap();

        assert!(agent.saved_conversation_context.is_some());
        let saved = agent.saved_conversation_context.as_ref().unwrap();
        assert_eq!(saved.conversation.len(), 1);
        assert_eq!(saved.system_prompt.as_deref(), Some("Original system"));
        assert_eq!(saved.current_conversation_id.as_deref(), Some("conv-1"));
        assert_eq!(saved.model, "test-model");
    }

    #[tokio::test]
    async fn switch_to_subagent_updates_system_prompt() {
        use std::collections::HashSet;
        let config = Config::default();
        let mut agent = Agent::new(config, "test-model".to_string(), false, false);

        let subagent_config = subagent::SubagentConfig {
            name: "subagent-1".to_string(),
            system_prompt: "Subagent prompt".to_string(),
            model: None,
            allowed_tools: HashSet::new(),
            denied_tools: HashSet::new(),
            max_tokens: None,
            temperature: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        agent.switch_to_subagent(&subagent_config).await.unwrap();

        assert_eq!(
            agent.conversation_manager.system_prompt.as_deref(),
            Some("Subagent prompt")
        );
        assert_eq!(
            agent.conversation_manager.subagent.as_deref(),
            Some("subagent-1")
        );
    }

    #[tokio::test]
    async fn switch_to_subagent_updates_model() {
        use std::collections::HashSet;
        let config = Config::default();
        let mut agent = Agent::new(config, "test-model".to_string(), false, false);

        let subagent_config = subagent::SubagentConfig {
            name: "subagent-1".to_string(),
            system_prompt: "Subagent prompt".to_string(),
            model: Some("new-model".to_string()),
            allowed_tools: HashSet::new(),
            denied_tools: HashSet::new(),
            max_tokens: None,
            temperature: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        agent.switch_to_subagent(&subagent_config).await.unwrap();

        assert_eq!(agent.model, "new-model");
        assert_eq!(agent.conversation_manager.model, "new-model");
    }

    #[tokio::test]
    async fn exit_subagent_restores_context() {
        use std::collections::HashSet;
        let config = Config::default();
        let mut agent = Agent::new(config, "test-model".to_string(), false, false);

        agent.conversation_manager.conversation.push(Message {
            role: "user".to_string(),
            content: vec![ContentBlock::text("Original message".to_string())],
        });
        agent.set_system_prompt("Original system".to_string());
        agent.conversation_manager.current_conversation_id = Some("conv-1".to_string());

        let subagent_config = subagent::SubagentConfig {
            name: "subagent-1".to_string(),
            system_prompt: "Subagent prompt".to_string(),
            model: Some("subagent-model".to_string()),
            allowed_tools: HashSet::new(),
            denied_tools: HashSet::new(),
            max_tokens: None,
            temperature: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };

        agent.switch_to_subagent(&subagent_config).await.unwrap();
        agent.exit_subagent().await.unwrap();

        assert_eq!(agent.conversation_manager.conversation.len(), 1);
        assert_eq!(
            agent.conversation_manager.system_prompt.as_deref(),
            Some("Original system")
        );
        assert_eq!(
            agent
                .conversation_manager
                .current_conversation_id
                .as_deref(),
            Some("conv-1")
        );
        assert_eq!(agent.model, "test-model");
        assert!(agent.conversation_manager.subagent.is_none());
    }

    #[tokio::test]
    async fn exit_subagent_without_saved_context_resets() {
        let config = Config::default();
        let mut agent = Agent::new(config, "test-model".to_string(), false, false);

        agent.exit_subagent().await.unwrap();

        assert_eq!(agent.conversation_manager.system_prompt, None);
        assert!(agent.conversation_manager.subagent.is_none());
    }

    #[test]
    fn snapshot_message_clone() {
        let msg = SnapshotMessage {
            role: "user".to_string(),
            content: vec![ContentBlock::text("Test".to_string())],
        };

        let cloned = msg.clone();
        assert_eq!(cloned.role, "user");
        assert_eq!(cloned.content.len(), 1);
    }

    #[test]
    fn saved_conversation_context_clone() {
        let context = SavedConversationContext {
            conversation: vec![Message {
                role: "user".to_string(),
                content: vec![ContentBlock::text("Test".to_string())],
            }],
            system_prompt: Some("Prompt".to_string()),
            current_conversation_id: Some("id-1".to_string()),
            model: "model-1".to_string(),
        };

        let cloned = context.clone();
        assert_eq!(cloned.conversation.len(), 1);
        assert_eq!(cloned.system_prompt.as_deref(), Some("Prompt"));
        assert_eq!(cloned.current_conversation_id.as_deref(), Some("id-1"));
        assert_eq!(cloned.model, "model-1");
    }

    #[test]
    fn stream_tool_event_serializes() {
        let event = StreamToolEvent {
            event: "tool_call".to_string(),
            tool_use_id: "id-1".to_string(),
            name: "read_file".to_string(),
            input: Some(json!({"file_path": "test.txt"})),
            content: None,
            is_error: None,
        };

        let serialized = serde_json::to_string(&event).unwrap();
        assert!(serialized.contains("tool_call"));
        assert!(serialized.contains("read_file"));
    }

    #[tokio::test]
    async fn new_with_plan_mode_adds_use_skill_tool() {
        let config = Config::default();
        let agent = Agent::new_with_plan_mode(config, "test-model".to_string(), false, false).await;

        let tools = agent.tools.read().await;
        assert!(tools.contains_key("use_skill"));
    }

    #[test]
    fn add_image_adds_to_conversation() {
        let config = Config::default();
        let mut agent = Agent::new(config, "test-model".to_string(), false, false);

        let media_type = "image/png".to_string();
        let base64_data = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==".to_string();

        agent.add_image(media_type.clone(), base64_data.clone(), None);

        assert_eq!(agent.conversation_manager.conversation.len(), 1);
        let message = &agent.conversation_manager.conversation[0];
        assert_eq!(message.role, "user");
        assert_eq!(message.content.len(), 1);

        let block = &message.content[0];
        assert_eq!(block.block_type, "image");
        assert!(block.source.is_some());

        let source = block.source.as_ref().unwrap();
        assert_eq!(source.source_type, "base64");
        assert_eq!(source.media_type, media_type);
        assert_eq!(source.data, base64_data);
    }

    #[test]
    fn add_image_with_description() {
        let config = Config::default();
        let mut agent = Agent::new(config, "test-model".to_string(), false, false);

        let media_type = "image/jpeg".to_string();
        let base64_data = "fake_base64_data".to_string();
        let description = "Test image description".to_string();

        agent.add_image(
            media_type.clone(),
            base64_data.clone(),
            Some(description.clone()),
        );

        assert_eq!(agent.conversation_manager.conversation.len(), 1);
        let message = &agent.conversation_manager.conversation[0];
        assert_eq!(message.role, "user");
        assert_eq!(message.content.len(), 2);

        // First block should be the image
        let image_block = &message.content[0];
        assert_eq!(image_block.block_type, "image");
        assert!(image_block.source.is_some());

        // Second block should be the description
        let text_block = &message.content[1];
        assert_eq!(text_block.block_type, "text");
        assert_eq!(text_block.text.as_deref(), Some(description.as_str()));
    }

    #[test]
    fn add_image_without_description() {
        let config = Config::default();
        let mut agent = Agent::new(config, "test-model".to_string(), false, false);

        agent.add_image("image/png".to_string(), "data123".to_string(), None);

        let message = &agent.conversation_manager.conversation[0];
        // Should only have the image block, no text block
        assert_eq!(message.content.len(), 1);
        assert_eq!(message.content[0].block_type, "image");
    }
}
