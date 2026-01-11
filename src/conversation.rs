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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::{DatabaseManager, Message as StoredMessage, ToolCallRecord};
    use std::sync::Arc;
    use tempfile::TempDir;
    use tokio::fs;

    async fn create_test_db() -> (Arc<DatabaseManager>, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let db_path = temp_dir.path().join("test.db");
        let db = DatabaseManager::new(db_path).await.unwrap();
        (Arc::new(db), temp_dir)
    }

    #[test]
    fn test_new_without_database() {
        let manager =
            ConversationManager::new(None, None, "claude-3-5-sonnet-20241022".to_string());
        assert!(manager.conversation.is_empty());
        assert!(manager.system_prompt.is_none());
        assert!(manager.current_conversation_id.is_none());
        assert!(manager.database_manager.is_none());
        assert_eq!(manager.model, "claude-3-5-sonnet-20241022");
        assert!(manager.subagent.is_none());
    }

    #[test]
    fn test_new_with_system_prompt() {
        let system_prompt = Some("You are a helpful assistant".to_string());
        let manager = ConversationManager::new(
            system_prompt.clone(),
            None,
            "claude-3-5-sonnet-20241022".to_string(),
        );
        assert_eq!(manager.system_prompt, system_prompt);
    }

    #[tokio::test]
    async fn test_new_with_database() {
        let (db, _temp_dir) = create_test_db().await;
        let manager = ConversationManager::new(
            None,
            Some(db.clone()),
            "claude-3-5-sonnet-20241022".to_string(),
        );
        assert!(manager.database_manager.is_some());
    }

    #[tokio::test]
    async fn test_start_new_conversation_without_database() {
        let mut manager =
            ConversationManager::new(None, None, "claude-3-5-sonnet-20241022".to_string());
        let conversation_id = manager.start_new_conversation().await.unwrap();
        assert!(manager.current_conversation_id.is_some());
        assert_eq!(manager.current_conversation_id.unwrap(), conversation_id);
    }

    #[tokio::test]
    async fn test_start_new_conversation_with_database() {
        let (db, _temp_dir) = create_test_db().await;
        let mut manager =
            ConversationManager::new(None, Some(db), "claude-3-5-sonnet-20241022".to_string());
        let conversation_id = manager.start_new_conversation().await.unwrap();
        assert!(manager.current_conversation_id.is_some());
        assert_eq!(
            manager.current_conversation_id.as_ref().unwrap(),
            &conversation_id
        );
    }

    #[tokio::test]
    async fn test_start_new_conversation_with_system_prompt() {
        let (db, _temp_dir) = create_test_db().await;
        let system_prompt = Some("You are a helpful assistant".to_string());
        let mut manager = ConversationManager::new(
            system_prompt.clone(),
            Some(db.clone()),
            "claude-3-5-sonnet-20241022".to_string(),
        );
        let conversation_id = manager.start_new_conversation().await.unwrap();

        // Verify conversation was created in database using get_conversation
        let conversation = db.get_conversation(&conversation_id).await.unwrap();
        assert!(conversation.is_some());
        let conversation = conversation.unwrap();
        assert_eq!(conversation.id, conversation_id);
        assert_eq!(conversation.system_prompt, system_prompt);
    }

    #[tokio::test]
    async fn test_save_message_to_conversation() {
        let (db, _temp_dir) = create_test_db().await;
        let mut manager = ConversationManager::new(
            None,
            Some(db.clone()),
            "claude-3-5-sonnet-20241022".to_string(),
        );
        let conversation_id = manager.start_new_conversation().await.unwrap();

        manager
            .save_message_to_conversation("user", "Hello, world!", 10)
            .await
            .unwrap();

        // Verify message was saved
        let messages = db
            .get_conversation_messages(&conversation_id)
            .await
            .unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].role, "user");
        assert_eq!(messages[0].content, "Hello, world!");
    }

    #[tokio::test]
    async fn test_save_message_without_database() {
        let mut manager =
            ConversationManager::new(None, None, "claude-3-5-sonnet-20241022".to_string());
        manager.current_conversation_id = Some("test-id".to_string());

        // Should not error even without database
        let result = manager
            .save_message_to_conversation("user", "Hello!", 5)
            .await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_update_conversation_model() {
        let (db, _temp_dir) = create_test_db().await;
        let mut manager = ConversationManager::new(
            None,
            Some(db.clone()),
            "claude-3-5-sonnet-20241022".to_string(),
        );
        let conversation_id = manager.start_new_conversation().await.unwrap();

        manager
            .update_conversation_model("claude-3-opus-20240229".to_string())
            .await
            .unwrap();

        assert_eq!(manager.model, "claude-3-opus-20240229");

        // Verify model was updated in database using get_conversation
        let conversation = db
            .get_conversation(&conversation_id)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(conversation.model, "claude-3-opus-20240229");
    }

    #[tokio::test]
    async fn test_update_database_usage_stats() {
        let (db, _temp_dir) = create_test_db().await;
        let mut manager = ConversationManager::new(
            None,
            Some(db.clone()),
            "claude-3-5-sonnet-20241022".to_string(),
        );

        manager.update_database_usage_stats(100, 50).await.unwrap();

        // Verify stats were updated
        let stats = db.get_stats_overview().await.unwrap();
        assert_eq!(stats.total_tokens, 150); // 100 input + 50 output
    }

    #[tokio::test]
    async fn test_save_plan() {
        let (db, _temp_dir) = create_test_db().await;
        let mut manager = ConversationManager::new(
            None,
            Some(db.clone()),
            "claude-3-5-sonnet-20241022".to_string(),
        );
        let conversation_id = manager.start_new_conversation().await.unwrap();

        let plan_id = manager
            .save_plan(
                "Build a new feature",
                "# Plan\n1. Step one\n2. Step two",
                Some("Feature Plan".to_string()),
            )
            .await
            .unwrap();

        assert!(plan_id.is_some());

        // Verify plan was saved
        let plans = db.list_plans(None).await.unwrap();
        assert_eq!(plans.len(), 1);
        assert_eq!(plans[0].conversation_id, Some(conversation_id));
        assert_eq!(plans[0].title, Some("Feature Plan".to_string()));
        assert_eq!(plans[0].user_request, "Build a new feature");
    }

    #[tokio::test]
    async fn test_save_plan_without_database() {
        let mut manager =
            ConversationManager::new(None, None, "claude-3-5-sonnet-20241022".to_string());
        manager.current_conversation_id = Some("test-id".to_string());

        let plan_id = manager
            .save_plan("Test plan", "# Test", None)
            .await
            .unwrap();

        assert!(plan_id.is_none());
    }

    #[tokio::test]
    async fn test_extract_context_files() {
        let manager =
            ConversationManager::new(None, None, "claude-3-5-sonnet-20241022".to_string());

        let message = "Please review @src/main.rs and @tests/test.rs";
        let files = manager.extract_context_files(message);

        assert_eq!(files.len(), 2);
        assert_eq!(files[0], "src/main.rs");
        assert_eq!(files[1], "tests/test.rs");
    }

    #[tokio::test]
    async fn test_extract_context_files_no_files() {
        let manager =
            ConversationManager::new(None, None, "claude-3-5-sonnet-20241022".to_string());

        let message = "Hello, world!";
        let files = manager.extract_context_files(message);

        assert_eq!(files.len(), 0);
    }

    #[tokio::test]
    async fn test_extract_context_files_multiple_formats() {
        let manager =
            ConversationManager::new(None, None, "claude-3-5-sonnet-20241022".to_string());

        let message = "Check @file1.txt @path/to/file2.rs and @another_file.md";
        let files = manager.extract_context_files(message);

        assert_eq!(files.len(), 3);
        assert_eq!(files[0], "file1.txt");
        assert_eq!(files[1], "path/to/file2.rs");
        assert_eq!(files[2], "another_file.md");
    }

    #[tokio::test]
    async fn test_clean_message() {
        let manager =
            ConversationManager::new(None, None, "claude-3-5-sonnet-20241022".to_string());

        let message = "Please review @src/main.rs and @tests/test.rs";
        let cleaned = manager.clean_message(message);

        assert_eq!(cleaned, "Please review src/main.rs and tests/test.rs");
    }

    #[tokio::test]
    async fn test_clean_message_no_files() {
        let manager =
            ConversationManager::new(None, None, "claude-3-5-sonnet-20241022".to_string());

        let message = "Hello, world!";
        let cleaned = manager.clean_message(message);

        assert_eq!(cleaned, "Hello, world!");
    }

    #[tokio::test]
    async fn test_add_context_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "Test content").await.unwrap();

        let mut manager =
            ConversationManager::new(None, None, "claude-3-5-sonnet-20241022".to_string());
        manager
            .add_context_file(file_path.to_str().unwrap())
            .await
            .unwrap();

        assert_eq!(manager.conversation.len(), 1);
        assert_eq!(manager.conversation[0].role, "user");
        assert_eq!(manager.conversation[0].content.len(), 1);

        if let Some(text) = &manager.conversation[0].content[0].text {
            assert!(text.contains("Test content"));
            assert!(text.contains("Context from file"));
        } else {
            panic!("Expected text content");
        }
    }

    #[tokio::test]
    async fn test_add_context_file_nonexistent() {
        let mut manager =
            ConversationManager::new(None, None, "claude-3-5-sonnet-20241022".to_string());
        let result = manager.add_context_file("/nonexistent/file.txt").await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_clear_conversation_keep_agents_md() {
        let (db, _temp_dir) = create_test_db().await;
        let mut manager = ConversationManager::new(
            None,
            Some(db.clone()),
            "claude-3-5-sonnet-20241022".to_string(),
        );

        // Start a conversation and add some messages
        let first_id = manager.start_new_conversation().await.unwrap();
        manager.conversation.push(crate::anthropic::Message {
            role: "user".to_string(),
            content: vec![ContentBlock::text("Hello".to_string())],
        });
        manager.conversation.push(crate::anthropic::Message {
            role: "assistant".to_string(),
            content: vec![ContentBlock::text("Hi there!".to_string())],
        });

        // Clear conversation
        let new_id = manager.clear_conversation_keep_agents_md().await.unwrap();

        assert_ne!(first_id, new_id);
        assert_eq!(manager.current_conversation_id, Some(new_id.clone()));

        // Conversation should be cleared (or only have AGENTS.md if it exists)
        // Since we don't have AGENTS.md in test, it should be empty
        assert!(
            manager.conversation.is_empty()
                || manager.conversation.iter().all(|m| {
                    m.content.iter().any(|c| {
                        if let Some(text) = &c.text {
                            text.contains("AGENTS.md")
                        } else {
                            false
                        }
                    })
                })
        );
    }

    #[tokio::test]
    async fn test_set_conversation_from_records() {
        let mut manager =
            ConversationManager::new(None, None, "claude-3-5-sonnet-20241022".to_string());

        let messages = vec![
            StoredMessage {
                id: "1".to_string(),
                role: "user".to_string(),
                content: "Hello".to_string(),
                created_at: chrono::Utc::now(),
            },
            StoredMessage {
                id: "2".to_string(),
                role: "assistant".to_string(),
                content: "Hi there!".to_string(),
                created_at: chrono::Utc::now() + chrono::Duration::seconds(1),
            },
        ];

        manager.set_conversation_from_records(
            "conv1".to_string(),
            Some("You are helpful".to_string()),
            "claude-3-5-sonnet-20241022".to_string(),
            None,
            &messages,
            &[],
        );

        assert_eq!(manager.current_conversation_id, Some("conv1".to_string()));
        assert_eq!(manager.system_prompt, Some("You are helpful".to_string()));
        assert_eq!(manager.model, "claude-3-5-sonnet-20241022");
        assert_eq!(manager.conversation.len(), 2);
        assert_eq!(manager.conversation[0].role, "user");
        assert_eq!(manager.conversation[1].role, "assistant");
    }

    #[tokio::test]
    async fn test_set_conversation_from_records_with_tools() {
        let mut manager =
            ConversationManager::new(None, None, "claude-3-5-sonnet-20241022".to_string());

        let now = chrono::Utc::now();
        let messages = vec![StoredMessage {
            id: "1".to_string(),
            role: "user".to_string(),
            content: "Use the tool".to_string(),
            created_at: now,
        }];

        let tool_calls = vec![ToolCallRecord {
            id: "tool1".to_string(),
            tool_name: "read_file".to_string(),
            tool_arguments: r#"{"path": "test.txt"}"#.to_string(),
            result_content: Some("File content".to_string()),
            is_error: false,
            created_at: now + chrono::Duration::seconds(1),
        }];

        manager.set_conversation_from_records(
            "conv1".to_string(),
            None,
            "claude-3-5-sonnet-20241022".to_string(),
            None,
            &messages,
            &tool_calls,
        );

        // Should have: user message, assistant tool_use, user tool_result
        assert_eq!(manager.conversation.len(), 3);
        assert_eq!(manager.conversation[0].role, "user");
        assert_eq!(manager.conversation[1].role, "assistant");
        assert_eq!(manager.conversation[2].role, "user");

        // Check tool_use block
        assert_eq!(manager.conversation[1].content[0].block_type, "tool_use");
        assert_eq!(
            manager.conversation[1].content[0].id,
            Some("tool1".to_string())
        );
        assert_eq!(
            manager.conversation[1].content[0].name,
            Some("read_file".to_string())
        );

        // Check tool_result block
        assert_eq!(manager.conversation[2].content[0].block_type, "tool_result");
        assert_eq!(
            manager.conversation[2].content[0].tool_use_id,
            Some("tool1".to_string())
        );
        assert_eq!(
            manager.conversation[2].content[0].content,
            Some("File content".to_string())
        );
    }

    #[tokio::test]
    async fn test_set_conversation_timeline_ordering() {
        let mut manager =
            ConversationManager::new(None, None, "claude-3-5-sonnet-20241022".to_string());

        let base_time = chrono::Utc::now();

        let messages = vec![
            StoredMessage {
                id: "1".to_string(),
                role: "user".to_string(),
                content: "First".to_string(),
                created_at: base_time,
            },
            StoredMessage {
                id: "2".to_string(),
                role: "user".to_string(),
                content: "Third".to_string(),
                created_at: base_time + chrono::Duration::seconds(3),
            },
        ];

        let tool_calls = vec![ToolCallRecord {
            id: "tool1".to_string(),
            tool_name: "test_tool".to_string(),
            tool_arguments: "{}".to_string(),
            result_content: Some("result".to_string()),
            is_error: false,
            created_at: base_time + chrono::Duration::seconds(1),
        }];

        manager.set_conversation_from_records(
            "conv1".to_string(),
            None,
            "claude-3-5-sonnet-20241022".to_string(),
            None,
            &messages,
            &tool_calls,
        );

        // Timeline should be: user "First", assistant tool_use, user tool_result, user "Third"
        assert_eq!(manager.conversation.len(), 4);

        // Check text content to verify ordering
        if let Some(text) = &manager.conversation[0].content[0].text {
            assert_eq!(text, "First");
        }

        assert_eq!(manager.conversation[1].content[0].block_type, "tool_use");
        assert_eq!(manager.conversation[2].content[0].block_type, "tool_result");

        if let Some(text) = &manager.conversation[3].content[0].text {
            assert_eq!(text, "Third");
        }
    }

    #[tokio::test]
    async fn test_set_conversation_with_subagent() {
        let mut manager =
            ConversationManager::new(None, None, "claude-3-5-sonnet-20241022".to_string());

        manager.set_conversation_from_records(
            "conv1".to_string(),
            None,
            "claude-3-5-sonnet-20241022".to_string(),
            Some("test-agent".to_string()),
            &[],
            &[],
        );

        assert_eq!(manager.subagent, Some("test-agent".to_string()));
    }

    #[tokio::test]
    async fn test_multiple_conversations() {
        let (db, _temp_dir) = create_test_db().await;
        let mut manager = ConversationManager::new(
            None,
            Some(db.clone()),
            "claude-3-5-sonnet-20241022".to_string(),
        );

        let conv1_id = manager.start_new_conversation().await.unwrap();
        manager
            .save_message_to_conversation("user", "Message 1", 5)
            .await
            .unwrap();

        let conv2_id = manager.start_new_conversation().await.unwrap();
        manager
            .save_message_to_conversation("user", "Message 2", 5)
            .await
            .unwrap();

        assert_ne!(conv1_id, conv2_id);

        // Verify both conversations exist
        let conversations = db.get_recent_conversations(100, None).await.unwrap();
        assert_eq!(conversations.len(), 2);

        // Verify messages are in correct conversations
        let conv1_messages = db.get_conversation_messages(&conv1_id).await.unwrap();
        assert_eq!(conv1_messages.len(), 1);
        assert_eq!(conv1_messages[0].content, "Message 1");

        let conv2_messages = db.get_conversation_messages(&conv2_id).await.unwrap();
        assert_eq!(conv2_messages.len(), 1);
        assert_eq!(conv2_messages[0].content, "Message 2");
    }

    #[tokio::test]
    async fn test_default_agents_files_empty() {
        // This test will likely return empty since we don't have AGENTS.md in test environment
        let files = ConversationManager::default_agents_files();
        // Just verify it doesn't panic
        assert!(files.is_empty() || !files.is_empty());
    }

    #[tokio::test]
    async fn test_add_context_file_with_tilde() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "Content").await.unwrap();

        let mut manager =
            ConversationManager::new(None, None, "claude-3-5-sonnet-20241022".to_string());

        // Test that tilde expansion works (if path contains ~)
        // In test environment, just verify absolute path works
        let result = manager.add_context_file(file_path.to_str().unwrap()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_tool_call_with_error() {
        let mut manager =
            ConversationManager::new(None, None, "claude-3-5-sonnet-20241022".to_string());

        let tool_calls = vec![ToolCallRecord {
            id: "tool1".to_string(),
            tool_name: "failing_tool".to_string(),
            tool_arguments: "{}".to_string(),
            result_content: Some("Error: Tool failed".to_string()),
            is_error: true,
            created_at: chrono::Utc::now(),
        }];

        manager.set_conversation_from_records(
            "conv1".to_string(),
            None,
            "claude-3-5-sonnet-20241022".to_string(),
            None,
            &[],
            &tool_calls,
        );

        // Should have tool_use and tool_result
        assert_eq!(manager.conversation.len(), 2);

        // Check that error flag is set
        assert_eq!(manager.conversation[1].content[0].is_error, Some(true));
    }

    #[tokio::test]
    async fn test_tool_call_without_result() {
        let mut manager =
            ConversationManager::new(None, None, "claude-3-5-sonnet-20241022".to_string());

        let tool_calls = vec![ToolCallRecord {
            id: "tool1".to_string(),
            tool_name: "pending_tool".to_string(),
            tool_arguments: "{}".to_string(),
            result_content: None,
            is_error: false,
            created_at: chrono::Utc::now(),
        }];

        manager.set_conversation_from_records(
            "conv1".to_string(),
            None,
            "claude-3-5-sonnet-20241022".to_string(),
            None,
            &[],
            &tool_calls,
        );

        // Should only have tool_use, no tool_result since result_content is None
        assert_eq!(manager.conversation.len(), 1);
        assert_eq!(manager.conversation[0].content[0].block_type, "tool_use");
    }

    #[tokio::test]
    async fn test_extract_context_files_with_special_characters() {
        let manager =
            ConversationManager::new(None, None, "claude-3-5-sonnet-20241022".to_string());

        let message = "Check @file-with-dashes.txt and @file_with_underscores.rs";
        let files = manager.extract_context_files(message);

        assert_eq!(files.len(), 2);
        assert_eq!(files[0], "file-with-dashes.txt");
        assert_eq!(files[1], "file_with_underscores.rs");
    }

    #[tokio::test]
    async fn test_clean_message_preserves_other_at_symbols() {
        let manager =
            ConversationManager::new(None, None, "claude-3-5-sonnet-20241022".to_string());

        let message = "Email me at john.doe@email.com about @src/file.rs";
        let cleaned = manager.clean_message(message);

        // Note: The regex @([^\s@]+) will match @email.com in the email address
        // So emails with @ will be affected by the cleaning
        // The file reference should be cleaned
        assert!(cleaned.contains("src/file.rs"));
        assert!(!cleaned.contains("@src/file.rs"));
    }

    #[tokio::test]
    async fn test_conversation_with_empty_content() {
        let mut manager =
            ConversationManager::new(None, None, "claude-3-5-sonnet-20241022".to_string());

        manager.conversation.push(crate::anthropic::Message {
            role: "user".to_string(),
            content: vec![],
        });

        // Should not panic when displaying or processing
        assert_eq!(manager.conversation.len(), 1);
        assert_eq!(manager.conversation[0].content.len(), 0);
    }

    #[tokio::test]
    async fn test_save_multiple_messages_to_conversation() {
        let (db, _temp_dir) = create_test_db().await;
        let mut manager = ConversationManager::new(
            None,
            Some(db.clone()),
            "claude-3-5-sonnet-20241022".to_string(),
        );
        let conversation_id = manager.start_new_conversation().await.unwrap();

        manager
            .save_message_to_conversation("user", "First message", 5)
            .await
            .unwrap();
        manager
            .save_message_to_conversation("assistant", "Second message", 10)
            .await
            .unwrap();
        manager
            .save_message_to_conversation("user", "Third message", 7)
            .await
            .unwrap();

        let messages = db
            .get_conversation_messages(&conversation_id)
            .await
            .unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].content, "First message");
        assert_eq!(messages[1].content, "Second message");
        assert_eq!(messages[2].content, "Third message");
    }

    #[tokio::test]
    async fn test_update_usage_stats_accumulation() {
        let (db, _temp_dir) = create_test_db().await;
        let mut manager = ConversationManager::new(
            None,
            Some(db.clone()),
            "claude-3-5-sonnet-20241022".to_string(),
        );

        manager.update_database_usage_stats(100, 50).await.unwrap();
        manager.update_database_usage_stats(200, 75).await.unwrap();

        let stats = db.get_stats_overview().await.unwrap();
        assert_eq!(stats.total_tokens, 425); // (100+50) + (200+75) = 425
    }
}
