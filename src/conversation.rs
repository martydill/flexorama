use crate::anthropic::ContentBlock;
use crate::database::{DatabaseManager, Message as StoredMessage};
use anyhow::Result;
use colored::Colorize;
use log::{debug, info};
use regex::Regex;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

/// Manages conversation state and database operations
pub struct ConversationManager {
    pub conversation: Vec<crate::anthropic::Message>,
    pub system_prompt: Option<String>,
    pub current_conversation_id: Option<String>,
    pub database_manager: Option<Arc<DatabaseManager>>,
    pub model: String,
    pub subagent: Option<String>,
}

#[derive(Debug, Clone)]
enum TimelineEntry {
    Message(StoredMessage),
    ToolCall(crate::database::ToolCallRecord),
    ToolResult(crate::database::ToolCallRecord),
}

impl ConversationManager {
    pub fn new(
        system_prompt: Option<String>,
        database_manager: Option<Arc<DatabaseManager>>,
        model: String,
    ) -> Self {
        Self {
            conversation: Vec::new(),
            system_prompt,
            current_conversation_id: None,
            database_manager,
            model,
            subagent: None,
        }
    }

    /// Start a new conversation
    pub async fn start_new_conversation(&mut self) -> Result<String> {
        if let Some(database_manager) = &self.database_manager {
            // Create new conversation in database
            let conversation_id = database_manager
                .create_conversation(
                    self.system_prompt.clone(),
                    &self.model,
                    self.subagent.as_deref(),
                )
                .await?;

            // Update current conversation tracking
            self.current_conversation_id = Some(conversation_id.clone());

            info!("Started new conversation: {}", conversation_id);
            Ok(conversation_id)
        } else {
            // Fallback: just generate a conversation ID without database
            let conversation_id = Uuid::new_v4().to_string();
            self.current_conversation_id = Some(conversation_id.clone());
            Ok(conversation_id)
        }
    }

    /// Save a message to the current conversation in the database
    pub async fn save_message_to_conversation(
        &mut self,
        role: &str,
        content: &str,
        tokens: i32,
    ) -> Result<()> {
        if let (Some(database_manager), Some(conversation_id)) =
            (&self.database_manager, &self.current_conversation_id)
        {
            database_manager
                .add_message(conversation_id, role, content, &self.model, tokens)
                .await?;
        }
        Ok(())
    }

    /// Update the model for the current conversation
    pub async fn update_conversation_model(&mut self, model: String) -> Result<()> {
        self.model = model.clone();
        if let (Some(database_manager), Some(conversation_id)) =
            (&self.database_manager, &self.current_conversation_id)
        {
            database_manager
                .update_conversation_model(conversation_id, &model)
                .await?;
        }
        Ok(())
    }

    /// Update usage statistics in the database
    pub async fn update_database_usage_stats(
        &mut self,
        input_tokens: i32,
        output_tokens: i32,
    ) -> Result<()> {
        if let Some(database_manager) = &self.database_manager {
            database_manager
                .update_usage_stats(input_tokens, output_tokens)
                .await?;
        }
        Ok(())
    }

    /// Persist a plan (if database is configured) and associate it with the current conversation
    pub async fn save_plan(
        &self,
        user_request: &str,
        plan_markdown: &str,
        title: Option<String>,
    ) -> Result<Option<String>> {
        if let Some(database_manager) = &self.database_manager {
            let plan_id = database_manager
                .create_plan(
                    self.current_conversation_id.as_deref(),
                    title.as_deref(),
                    user_request,
                    plan_markdown,
                )
                .await?;
            Ok(Some(plan_id))
        } else {
            Ok(None)
        }
    }

    /// Clear conversation but keep AGENTS.md files if they exist in context
    /// Create a new conversation in the database and start tracking it
    pub async fn clear_conversation_keep_agents_md(&mut self) -> Result<String> {
        let agent_files = Self::default_agents_files();

        // Clear the conversation first
        self.conversation.clear();

        // Start a new conversation in the database
        let new_conversation_id = self.start_new_conversation().await?;
        debug!(
            "Started new conversation {} after clearing",
            new_conversation_id
        );

        // Re-add AGENTS.md content if it was captured
        for file_path in &agent_files {
            match self.add_context_file(file_path).await {
                Ok(_) => app_println!("{} Re-added context file: {}", "\u{2713}", file_path),
                Err(e) => log::warn!("Failed to re-add context file '{}': {}", file_path, e),
            }
        }

        if agent_files.is_empty() {
            debug!("Clearing conversation (no AGENTS.md files found)");
        }

        Ok(new_conversation_id)
    }

    /// Return the default AGENTS.md files the app will auto-load (home + local)
    pub fn default_agents_files() -> Vec<String> {
        let mut files = Vec::new();
        let home_agents_md = dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(".flexorama")
            .join("AGENTS.md");
        if home_agents_md.exists() {
            files.push(home_agents_md.display().to_string());
        }

        let local_agents_md = PathBuf::from("AGENTS.md");
        if local_agents_md.exists() {
            files.push(local_agents_md.display().to_string());
        }
        files
    }

    /// Add a file as context to the conversation
    pub async fn add_context_file(&mut self, file_path: &str) -> Result<()> {
        use path_absolutize::*;
        use shellexpand;
        use tokio::fs;

        let expanded_path = shellexpand::tilde(file_path);
        let absolute_path = Path::new(&*expanded_path).absolutize()?;

        match fs::read_to_string(&absolute_path).await {
            Ok(content) => {
                let context_message = format!(
                    "Context from file '{}':\n\n```\n{}\n```",
                    absolute_path.display(),
                    content
                );

                self.conversation.push(crate::anthropic::Message {
                    role: "user".to_string(),
                    content: vec![crate::anthropic::ContentBlock::text(context_message)],
                });

                debug!("Added context file: {}", absolute_path.display());
                Ok(())
            }
            Err(e) => {
                anyhow::bail!("Failed to read file '{}': {}", absolute_path.display(), e);
            }
        }
    }

    /// Extract file paths from message using @path syntax
    pub fn extract_context_files(&self, message: &str) -> Vec<String> {
        let re = regex::Regex::new(r"@([^\s@]+)").unwrap();
        re.captures_iter(message)
            .map(|cap| cap[1].to_string())
            .collect()
    }

    /// Replace @file syntax with actual file paths and return cleaned message
    pub fn clean_message(&self, message: &str) -> String {
        let re = Regex::new(r"@([^\s@]+)").unwrap();
        re.replace_all(message, |caps: &regex::Captures| {
            let file_path = &caps[1];
            file_path.to_string()
        })
        .trim()
        .to_string()
    }

    /// Replace the current in-memory conversation with records loaded from storage
    pub fn set_conversation_from_records(
        &mut self,
        conversation_id: String,
        system_prompt: Option<String>,
        model: String,
        subagent: Option<String>,
        messages: &[StoredMessage],
        tool_calls: &[crate::database::ToolCallRecord],
    ) {
        self.conversation.clear();
        let mut timeline: Vec<(chrono::DateTime<chrono::Utc>, i32, TimelineEntry)> = Vec::new();

        for message in messages {
            timeline.push((
                message.created_at,
                0,
                TimelineEntry::Message(message.clone()),
            ));
        }

        for tool_call in tool_calls {
            timeline.push((
                tool_call.created_at,
                1,
                TimelineEntry::ToolCall(tool_call.clone()),
            ));
            if tool_call.result_content.is_some() {
                timeline.push((
                    tool_call.created_at + chrono::Duration::milliseconds(1),
                    2,
                    TimelineEntry::ToolResult(tool_call.clone()),
                ));
            }
        }

        timeline.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

        for (_ts, _order, entry) in timeline {
            match entry {
                TimelineEntry::Message(message) => {
                    self.conversation.push(crate::anthropic::Message {
                        role: message.role.clone(),
                        content: vec![ContentBlock::text(message.content.clone())],
                    });
                }
                TimelineEntry::ToolCall(tc) => {
                    let input_value: serde_json::Value = serde_json::from_str(&tc.tool_arguments)
                        .unwrap_or_else(|_| serde_json::Value::String(tc.tool_arguments.clone()));
                    self.conversation.push(crate::anthropic::Message {
                        role: "assistant".to_string(),
                        content: vec![ContentBlock::tool_use(
                            tc.id.clone(),
                            tc.tool_name.clone(),
                            input_value,
                        )],
                    });
                }
                TimelineEntry::ToolResult(tc) => {
                    self.conversation.push(crate::anthropic::Message {
                        role: "user".to_string(),
                        content: vec![ContentBlock::tool_result(
                            tc.id.clone(),
                            tc.result_content.clone().unwrap_or_default(),
                            Some(tc.is_error),
                        )],
                    });
                }
            }
        }

        self.current_conversation_id = Some(conversation_id);
        self.system_prompt = system_prompt;
        self.model = model;
        self.subagent = subagent;
    }

    /// Display the current conversation context
    pub fn display_context(&self) {
        app_println!("{}", "📝 Current Conversation Context".cyan().bold());
        app_println!("{}", "─".repeat(50).dimmed());
        app_println!();

        // Display system prompt if set
        if let Some(system_prompt) = &self.system_prompt {
            app_println!("{}", "System Prompt:".green().bold());
            app_println!("  {}", system_prompt);
            app_println!();
        }

        if self.conversation.is_empty() {
            app_println!(
                "{}",
                "No context yet. Start a conversation to see context here.".dimmed()
            );
            app_println!();
            return;
        }

        for (i, message) in self.conversation.iter().enumerate() {
            let role_color = match message.role.as_str() {
                "user" => "blue",
                "assistant" => "green",
                _ => "yellow",
            };

            app_println!(
                "{} {}: {}",
                format!("[{}]", i + 1).dimmed(),
                format!("{}", message.role.to_uppercase()).color(role_color),
                format!("({} content blocks)", message.content.len()).dimmed()
            );

            for (j, block) in message.content.iter().enumerate() {
                match block.block_type.as_str() {
                    "text" => {
                        if let Some(ref text) = block.text {
                            // Show first 100 characters of text content
                            let preview = if text.len() > 100 {
                                // Use safe character boundary slicing
                                let safe_end = text
                                    .char_indices()
                                    .nth(100)
                                    .map(|(idx, _)| idx)
                                    .unwrap_or(text.len());
                                format!("{}...", &text[..safe_end])
                            } else {
                                text.clone()
                            };
                            app_println!(
                                "  {} {}: {}",
                                format!("└─ Block {}", j + 1).dimmed(),
                                "Text".green(),
                                preview.replace('\n', " ")
                            );
                        }
                    }
                    "tool_use" => {
                        if let (Some(ref id), Some(ref name), Some(ref input)) =
                            (&block.id, &block.name, &block.input)
                        {
                            app_println!(
                                "  {} {}: {} ({})",
                                format!("└─ Block {}", j + 1).dimmed(),
                                "Tool Use".yellow(),
                                name,
                                id
                            );
                            // Safely handle the input as a string
                            let input_str = match input {
                                Value::String(s) => s.clone(),
                                _ => serde_json::to_string_pretty(input)
                                    .unwrap_or_else(|_| "Invalid JSON".to_string()),
                            };
                            let preview = if input_str.len() > 80 {
                                // Use safe character boundary slicing
                                let safe_end = input_str
                                    .char_indices()
                                    .nth(80)
                                    .map(|(idx, _)| idx)
                                    .unwrap_or(input_str.len());
                                format!("{}...", &input_str[..safe_end])
                            } else {
                                input_str
                            };
                            app_println!("    {} {}", "Input:".dimmed(), preview);
                        }
                    }
                    "tool_result" => {
                        if let (Some(ref tool_use_id), Some(ref content), ref is_error) =
                            (&block.tool_use_id, &block.content, &block.is_error)
                        {
                            let result_type = if is_error.unwrap_or(false) {
                                "Error".red()
                            } else {
                                "Result".green()
                            };
                            app_println!(
                                "  {} {}: {} ({})",
                                format!("└─ Block {}", j + 1).dimmed(),
                                result_type,
                                tool_use_id,
                                format!("{} chars", content.len()).dimmed()
                            );
                            let preview = if content.len() > 80 {
                                // Use safe character boundary slicing
                                let safe_end = content
                                    .char_indices()
                                    .nth(80)
                                    .map(|(idx, _)| idx)
                                    .unwrap_or(content.len());
                                format!("{}...", &content[..safe_end])
                            } else {
                                content.clone()
                            };
                            app_println!(
                                "    {} {}",
                                "Content:".dimmed(),
                                preview.replace('\n', " ")
                            );
                        }
                    }
                    _ => {
                        app_println!(
                            "  {} {}",
                            format!("└─ Block {}", j + 1).dimmed(),
                            "Unknown".red()
                        );
                    }
                }
            }
            app_println!();
        }

        app_println!("{}", "─".repeat(50).dimmed());
        app_println!(
            "{}: {} messages, {} total content blocks",
            "Summary".bold(),
            self.conversation.len(),
            self.conversation
                .iter()
                .map(|m| m.content.len())
                .sum::<usize>()
        );
        app_println!();
    }
}
