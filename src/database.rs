use anyhow::{anyhow, Result};
use chrono::{DateTime, NaiveDate, Utc};
use log::{debug, info};
use serde::Serialize;
use sqlx::{Row, SqlitePool};
use std::path::PathBuf;
use std::str::FromStr;
use uuid::Uuid;

/// Database manager for AIxplosion
pub struct DatabaseManager {
    pool: SqlitePool,
    db_path: PathBuf,
}

impl DatabaseManager {
    /// Create a new database manager with the specified database path
    pub async fn new(db_path: PathBuf) -> Result<Self> {
        info!("Initializing database at: {}", db_path.display());

        // Create parent directories if they don't exist
        if let Some(parent) = db_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // Create database connection pool with proper Windows path handling
        let db_path_str = db_path.to_string_lossy();

        // Use sqlite: with forward slashes for Windows compatibility
        let normalized_path = db_path_str.replace('\\', "/");
        let connection_string = format!("sqlite:{}", normalized_path);
        debug!("Database connection string: {}", connection_string);
        debug!("Database file path: {}", db_path_str);
        debug!("Normalized path: {}", normalized_path);

        // Only test file creation if the database doesn't already exist
        if !tokio::fs::metadata(&db_path).await.is_ok() {
            debug!("Database file doesn't exist, testing creation permissions");
            match tokio::fs::File::create(&db_path).await {
                Ok(_) => {
                    debug!("Database file creation test successful");
                    // Remove the empty file so SQLite can create the proper database
                    tokio::fs::remove_file(&db_path).await?;
                }
                Err(e) => {
                    return Err(anyhow::anyhow!(
                        "Cannot create database file at {}: {}",
                        db_path_str,
                        e
                    ));
                }
            }
        } else {
            debug!("Database file already exists, skipping creation test");
        }

        // Connect with create_if_missing option
        let connect_opts = sqlx::sqlite::SqliteConnectOptions::from_str(&connection_string)?
            .create_if_missing(true);

        let pool = SqlitePool::connect_with(connect_opts).await?;

        let manager = Self {
            pool,
            db_path: db_path.clone(),
        };

        // Run migrations
        manager.run_migrations().await?;

        info!(
            "Database initialized successfully at: {}",
            db_path.display()
        );
        Ok(manager)
    }

    /// Run database migrations to create necessary tables
    async fn run_migrations(&self) -> Result<()> {
        debug!("Running database migrations...");

        // Create conversations table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS conversations (
                id TEXT PRIMARY KEY,
                created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                system_prompt TEXT,
                model TEXT NOT NULL,
                subagent TEXT,
                total_tokens INTEGER DEFAULT 0,
                request_count INTEGER DEFAULT 0
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Add subagent column to existing conversations table if it doesn't exist
        // This migration handles databases created before the subagent feature
        sqlx::query(
            r#"
            ALTER TABLE conversations ADD COLUMN subagent TEXT
            "#,
        )
        .execute(&self.pool)
        .await
        .ok(); // Ignore error if column already exists

        // Create messages table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS messages (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'system')),
                content TEXT NOT NULL,
                model TEXT NOT NULL DEFAULT '',
                tokens INTEGER DEFAULT 0,
                created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Add model column to existing messages table if it doesn't exist
        sqlx::query(
            r#"
            ALTER TABLE messages ADD COLUMN model TEXT NOT NULL DEFAULT ''
            "#,
        )
        .execute(&self.pool)
        .await
        .ok(); // Ignore error if column already exists

        // Create context_files table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS context_files (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                file_path TEXT NOT NULL,
                file_content TEXT,
                file_size INTEGER DEFAULT 0,
                added_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create tool_calls table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS tool_calls (
                id TEXT PRIMARY KEY,
                conversation_id TEXT NOT NULL,
                message_id TEXT,
                tool_name TEXT NOT NULL,
                tool_arguments TEXT NOT NULL,
                result_content TEXT,
                is_error BOOLEAN NOT NULL DEFAULT FALSE,
                created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE CASCADE,
                FOREIGN KEY (message_id) REFERENCES messages(id) ON DELETE SET NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create usage_stats table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS usage_stats (
                id TEXT PRIMARY KEY,
                date DATE NOT NULL UNIQUE,
                total_requests INTEGER DEFAULT 0,
                total_input_tokens INTEGER DEFAULT 0,
                total_output_tokens INTEGER DEFAULT 0,
                total_tokens INTEGER DEFAULT 0,
                created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create plans table for plan-mode persistence
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS plans (
                id TEXT PRIMARY KEY,
                conversation_id TEXT,
                title TEXT,
                user_request TEXT NOT NULL,
                plan_markdown TEXT NOT NULL,
                created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (conversation_id) REFERENCES conversations(id) ON DELETE SET NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create indexes for better performance
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_messages_conversation_id ON messages(conversation_id)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_messages_created_at ON messages(created_at)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_context_files_conversation_id ON context_files(conversation_id)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_tool_calls_conversation_id ON tool_calls(conversation_id)")
            .execute(&self.pool)
            .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_usage_stats_date ON usage_stats(date)")
            .execute(&self.pool)
            .await?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_plans_conversation_id ON plans(conversation_id)",
        )
        .execute(&self.pool)
        .await?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_plans_created_at ON plans(created_at)")
            .execute(&self.pool)
            .await?;

        debug!("Database migrations completed successfully");
        Ok(())
    }

    /// Get the database connection pool
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// Get the database path
    pub fn path(&self) -> &PathBuf {
        &self.db_path
    }

    /// Close the database connection pool
    pub async fn close(&self) {
        self.pool.close().await;
        info!("Database connection closed");
    }
}

/// Create a slug from a directory path
pub fn create_slug_from_path(path: &str) -> String {
    use regex::Regex;

    // Normalize the path by replacing backslashes with forward slashes
    let normalized_path = path.replace('\\', "/");

    // Extract the last few directory names to create a reasonable slug
    let path_parts: Vec<&str> = normalized_path
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    // Take the last 2-3 parts to create the slug
    let relevant_parts: Vec<&str> = if path_parts.len() > 3 {
        path_parts[path_parts.len() - 3..].to_vec()
    } else {
        path_parts
    };

    let base_slug = relevant_parts.join("_");

    // Clean up the slug:
    // 1. Replace invalid characters with underscores
    // 2. Remove consecutive underscores
    // 3. Convert to lowercase
    // 4. Limit length

    let re = Regex::new(r"[^a-zA-Z0-9_]").unwrap();
    let cleaned = re.replace_all(&base_slug, "_");

    let re_consecutive = Regex::new(r"_+").unwrap();
    let deduped = re_consecutive.replace_all(&cleaned, "_");

    let mut slug = deduped.to_lowercase();

    // Remove leading and trailing underscores
    slug = slug.trim_matches('_').to_string();

    // Limit length to 100 characters
    if slug.len() > 100 {
        slug.truncate(100);
    }

    // Ensure slug is not empty
    if slug.is_empty() {
        slug = "default".to_string();
    }

    slug
}

/// Represents a conversation in the database
#[derive(Debug, Clone)]
pub struct Conversation {
    pub id: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub system_prompt: Option<String>,
    pub model: String,
    pub subagent: Option<String>,
    pub total_tokens: i32,
    pub request_count: i32,
}

/// Represents a message in the database
#[derive(Debug, Clone)]
pub struct Message {
    pub id: String,
    pub conversation_id: String,
    pub role: String,
    pub content: String,
    pub model: String,
    pub tokens: i32,
    pub created_at: DateTime<Utc>,
}

/// Represents a tool call tied to a conversation
#[derive(Debug, Clone)]
pub struct ToolCallRecord {
    pub id: String,
    pub conversation_id: String,
    pub message_id: Option<String>,
    pub tool_name: String,
    pub tool_arguments: String,
    pub result_content: Option<String>,
    pub is_error: bool,
    pub created_at: DateTime<Utc>,
}

/// Represents a saved plan
#[derive(Debug, Clone)]
pub struct Plan {
    pub id: String,
    pub conversation_id: Option<String>,
    pub title: Option<String>,
    pub user_request: String,
    pub plan_markdown: String,
    pub created_at: DateTime<Utc>,
}

/// Usage statistics for a specific date
#[derive(Debug, Serialize)]
pub struct UsageStats {
    pub date: String,
    pub total_requests: i32,
    pub total_input_tokens: i32,
    pub total_output_tokens: i32,
    pub total_tokens: i32,
}

/// Statistics grouped by model
#[derive(Debug, Serialize)]
pub struct ModelStats {
    pub model: String,
    pub total_conversations: i32,
    pub total_tokens: i32,
    pub request_count: i32,
}

/// Overview of all statistics
#[derive(Debug, Serialize)]
pub struct StatsOverview {
    pub total_conversations: i32,
    pub total_messages: i32,
    pub total_tokens: i64,
    pub total_requests: i32,
}

impl DatabaseManager {
    /// Create a new conversation in the database
    pub async fn create_conversation(
        &self,
        system_prompt: Option<String>,
        model: &str,
        subagent: Option<&str>,
    ) -> Result<String> {
        let conversation_id = Uuid::new_v4().to_string();
        let now = Utc::now();

        debug!(
            "Creating new conversation: {} with model: {} and subagent: {:?}",
            conversation_id, model, subagent
        );

        sqlx::query(
            r#"
            INSERT INTO conversations (id, created_at, updated_at, system_prompt, model, subagent, total_tokens, request_count)
            VALUES (?, ?, ?, ?, ?, ?, 0, 0)
            "#
        )
        .bind(&conversation_id)
        .bind(now)
        .bind(now)
        .bind(&system_prompt)
        .bind(model)
        .bind(subagent)
        .execute(&self.pool)
        .await?;

        info!("Created new conversation: {}", conversation_id);
        Ok(conversation_id)
    }

    /// Add a message to a conversation
    pub async fn add_message(
        &self,
        conversation_id: &str,
        role: &str,
        content: &str,
        model: &str,
        tokens: i32,
    ) -> Result<String> {
        let message_id = Uuid::new_v4().to_string();
        let now = Utc::now();

        debug!(
            "Adding message {} to conversation {}: {} ({} tokens)",
            message_id, conversation_id, role, tokens
        );

        // Begin transaction
        let mut tx = self.pool.begin().await?;

        // Insert the message
        sqlx::query(
            r#"
            INSERT INTO messages (id, conversation_id, role, content, model, tokens, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&message_id)
        .bind(conversation_id)
        .bind(role)
        .bind(content)
        .bind(model)
        .bind(tokens)
        .bind(now)
        .execute(&mut *tx)
        .await?;

        // Update conversation stats
        sqlx::query(
            r#"
            UPDATE conversations
            SET total_tokens = total_tokens + ?,
                request_count = request_count + ?,
                updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(tokens)
        .bind(if role == "user" { 1 } else { 0 })
        .bind(now)
        .bind(conversation_id)
        .execute(&mut *tx)
        .await?;

        // Commit transaction
        tx.commit().await?;

        Ok(message_id)
    }

    /// Save a plan to the database
    pub async fn create_plan(
        &self,
        conversation_id: Option<&str>,
        title: Option<&str>,
        user_request: &str,
        plan_markdown: &str,
    ) -> Result<String> {
        let plan_id = Uuid::new_v4().to_string();
        let now = Utc::now();

        sqlx::query(
            r#"
            INSERT INTO plans (id, conversation_id, title, user_request, plan_markdown, created_at)
            VALUES (?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(&plan_id)
        .bind(conversation_id)
        .bind(title)
        .bind(user_request)
        .bind(plan_markdown)
        .bind(now)
        .execute(&self.pool)
        .await?;

        Ok(plan_id)
    }

    /// Fetch a plan by ID
    pub async fn get_plan(&self, plan_id: &str) -> Result<Option<Plan>> {
        let row = sqlx::query(
            r#"
            SELECT id, conversation_id, title, user_request, plan_markdown, created_at
            FROM plans
            WHERE id = ?
            "#,
        )
        .bind(plan_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(row.map(|row| Plan {
            id: row.get("id"),
            conversation_id: row.get::<Option<String>, _>("conversation_id"),
            title: row.get::<Option<String>, _>("title"),
            user_request: row.get("user_request"),
            plan_markdown: row.get("plan_markdown"),
            created_at: row.get("created_at"),
        }))
    }

    /// List plans ordered by creation time (descending)
    pub async fn list_plans(&self, limit: Option<i64>) -> Result<Vec<Plan>> {
        let mut query = String::from(
            r#"
            SELECT id, conversation_id, title, user_request, plan_markdown, created_at
            FROM plans
            ORDER BY created_at DESC
            "#,
        );

        if limit.is_some() {
            query.push_str(" LIMIT ?");
        }

        let mut sql = sqlx::query(&query);
        if let Some(limit) = limit {
            sql = sql.bind(limit);
        }

        let rows = sql.fetch_all(&self.pool).await?;

        let plans = rows
            .into_iter()
            .map(|row| Plan {
                id: row.get("id"),
                conversation_id: row.get::<Option<String>, _>("conversation_id"),
                title: row.get::<Option<String>, _>("title"),
                user_request: row.get("user_request"),
                plan_markdown: row.get("plan_markdown"),
                created_at: row.get("created_at"),
            })
            .collect();

        Ok(plans)
    }

    /// Update a plan with new metadata/content
    pub async fn update_plan(
        &self,
        plan_id: &str,
        title: Option<String>,
        user_request: Option<String>,
        plan_markdown: Option<String>,
    ) -> Result<Plan> {
        let existing = self
            .get_plan(plan_id)
            .await?
            .ok_or_else(|| anyhow!("Plan {} not found", plan_id))?;

        let new_title = title.or(existing.title);
        let new_user_request = user_request.unwrap_or(existing.user_request);
        let new_plan_markdown = plan_markdown.unwrap_or(existing.plan_markdown);

        sqlx::query(
            r#"
            UPDATE plans
            SET title = ?, user_request = ?, plan_markdown = ?
            WHERE id = ?
            "#,
        )
        .bind(&new_title)
        .bind(&new_user_request)
        .bind(&new_plan_markdown)
        .bind(plan_id)
        .execute(&self.pool)
        .await?;

        self.get_plan(plan_id)
            .await?
            .ok_or_else(|| anyhow!("Plan {} not found after update", plan_id))
    }

    /// Delete a plan
    pub async fn delete_plan(&self, plan_id: &str) -> Result<()> {
        sqlx::query("DELETE FROM plans WHERE id = ?")
            .bind(plan_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Get a conversation by ID
    pub async fn get_conversation(&self, conversation_id: &str) -> Result<Option<Conversation>> {
        let row = sqlx::query(
            r#"
            SELECT id, created_at, updated_at, system_prompt, model, subagent, total_tokens, request_count
            FROM conversations
            WHERE id = ?
            "#,
        )
        .bind(conversation_id)
        .fetch_optional(&self.pool)
        .await?;

        if let Some(row) = row {
            Ok(Some(Conversation {
                id: row.get("id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                system_prompt: row.get("system_prompt"),
                model: row.get("model"),
                subagent: row.get("subagent"),
                total_tokens: row.get("total_tokens"),
                request_count: row.get("request_count"),
            }))
        } else {
            Ok(None)
        }
    }

    /// Get all messages for a conversation
    pub async fn get_conversation_messages(&self, conversation_id: &str) -> Result<Vec<Message>> {
        let rows = sqlx::query(
            r#"
            SELECT id, conversation_id, role, content, model, tokens, created_at
            FROM messages
            WHERE conversation_id = ?
            ORDER BY created_at ASC
            "#,
        )
        .bind(conversation_id)
        .fetch_all(&self.pool)
        .await?;

        let messages = rows
            .into_iter()
            .map(|row| Message {
                id: row.get("id"),
                conversation_id: row.get("conversation_id"),
                role: row.get("role"),
                content: row.get("content"),
                model: row.get("model"),
                tokens: row.get("tokens"),
                created_at: row.get("created_at"),
            })
            .collect();

        Ok(messages)
    }

    /// Save a tool call for a conversation
    pub async fn add_tool_call(
        &self,
        conversation_id: &str,
        message_id: Option<&str>,
        tool_use_id: &str,
        tool_name: &str,
        tool_arguments: &str,
    ) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO tool_calls (id, conversation_id, message_id, tool_name, tool_arguments, is_error)
            VALUES (?, ?, ?, ?, ?, false)
            "#,
        )
        .bind(tool_use_id)
        .bind(conversation_id)
        .bind(message_id)
        .bind(tool_name)
        .bind(tool_arguments)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Update a tool call with its result content
    pub async fn complete_tool_call(
        &self,
        tool_use_id: &str,
        result_content: &str,
        is_error: bool,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE tool_calls
            SET result_content = ?, is_error = ?, created_at = created_at
            WHERE id = ?
            "#,
        )
        .bind(result_content)
        .bind(is_error)
        .bind(tool_use_id)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Fetch all tool calls for a conversation
    pub async fn get_conversation_tool_calls(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<ToolCallRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT id, conversation_id, message_id, tool_name, tool_arguments, result_content, is_error, created_at
            FROM tool_calls
            WHERE conversation_id = ?
            ORDER BY created_at ASC
            "#,
        )
        .bind(conversation_id)
        .fetch_all(&self.pool)
        .await?;

        let calls = rows
            .into_iter()
            .map(|row| ToolCallRecord {
                id: row.get("id"),
                conversation_id: row.get("conversation_id"),
                message_id: row.get("message_id"),
                tool_name: row.get("tool_name"),
                tool_arguments: row.get("tool_arguments"),
                result_content: row.get("result_content"),
                is_error: row.get("is_error"),
                created_at: row.get("created_at"),
            })
            .collect();

        Ok(calls)
    }

    /// Get recent conversations, optionally filtered by message content
    pub async fn get_recent_conversations(
        &self,
        limit: i64,
        search_filter: Option<&str>,
    ) -> Result<Vec<Conversation>> {
        // Base query is shared with /resume; optional filter narrows by message content
        let mut query = String::from(
            r#"
            SELECT id, created_at, updated_at, system_prompt, model, subagent, total_tokens, request_count
            FROM conversations c
            WHERE EXISTS (
                SELECT 1 FROM messages m WHERE m.conversation_id = c.id
            "#,
        );

        if search_filter.is_some() {
            query.push_str(" AND LOWER(m.content) LIKE LOWER(?) ");
        }

        query.push_str(
            r#"
            )
            ORDER BY updated_at DESC
            LIMIT ?
            "#,
        );

        let mut sql = sqlx::query(&query);

        if let Some(filter) = search_filter {
            let pattern = format!("%{}%", filter);
            sql = sql.bind(pattern);
        }

        sql = sql.bind(limit);

        let rows = sql.fetch_all(&self.pool).await?;

        let conversations = rows
            .into_iter()
            .map(|row| Conversation {
                id: row.get("id"),
                created_at: row.get("created_at"),
                updated_at: row.get("updated_at"),
                system_prompt: row.get("system_prompt"),
                model: row.get("model"),
                subagent: row.get("subagent"),
                total_tokens: row.get("total_tokens"),
                request_count: row.get("request_count"),
            })
            .collect();

        Ok(conversations)
    }

    /// Update the model for a conversation
    pub async fn update_conversation_model(
        &self,
        conversation_id: &str,
        model: &str,
    ) -> Result<()> {
        let now = Utc::now();
        sqlx::query(
            r#"
            UPDATE conversations
            SET model = ?, updated_at = ?
            WHERE id = ?
            "#,
        )
        .bind(model)
        .bind(now)
        .bind(conversation_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Update daily usage statistics
    pub async fn update_usage_stats(&self, input_tokens: i32, output_tokens: i32) -> Result<()> {
        let today = Utc::now().date_naive();
        let usage_id = Uuid::new_v4().to_string();
        let now = Utc::now();

        debug!(
            "Updating usage stats for {} - input: {}, output: {} tokens",
            today, input_tokens, output_tokens
        );

        // Try to update existing record first, then insert if it doesn't exist
        let result = sqlx::query(
            r#"
            UPDATE usage_stats
            SET total_requests = total_requests + 1,
                total_input_tokens = total_input_tokens + ?,
                total_output_tokens = total_output_tokens + ?,
                total_tokens = total_tokens + ? + ?,
                updated_at = ?
            WHERE date = ?
            "#,
        )
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(input_tokens)
        .bind(output_tokens)
        .bind(now)
        .bind(today)
        .execute(&self.pool)
        .await?;

        // If no rows were affected, insert a new record
        if result.rows_affected() == 0 {
            sqlx::query(
                r#"
                INSERT INTO usage_stats (id, date, total_requests, total_input_tokens, total_output_tokens, total_tokens, created_at, updated_at)
                VALUES (?, ?, 1, ?, ?, ?, ?, ?)
                "#
            )
            .bind(&usage_id)
            .bind(today)
            .bind(input_tokens)
            .bind(output_tokens)
            .bind(input_tokens + output_tokens)
            .bind(now)
            .bind(now)
            .execute(&self.pool)
            .await?;
        }

        Ok(())
    }

    /// Get usage statistics within a date range
    pub async fn get_usage_stats_range(
        &self,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
    ) -> Result<Vec<UsageStats>> {
        let start = start_date
            .unwrap_or_else(|| Utc::now().naive_utc().date() - chrono::Duration::days(30));
        let end = end_date.unwrap_or_else(|| Utc::now().naive_utc().date());

        let rows = sqlx::query(
            r#"
            SELECT date, total_requests, total_input_tokens, total_output_tokens, total_tokens
            FROM usage_stats
            WHERE date BETWEEN ? AND ?
            ORDER BY date
            "#,
        )
        .bind(start.to_string())
        .bind(end.to_string())
        .fetch_all(&self.pool)
        .await?;

        let stats = rows
            .iter()
            .map(|row| UsageStats {
                date: row.get("date"),
                total_requests: row.get("total_requests"),
                total_input_tokens: row.get("total_input_tokens"),
                total_output_tokens: row.get("total_output_tokens"),
                total_tokens: row.get("total_tokens"),
            })
            .collect();

        Ok(stats)
    }

    /// Get statistics grouped by model
    pub async fn get_stats_by_model(
        &self,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
    ) -> Result<Vec<ModelStats>> {
        let start = start_date
            .unwrap_or_else(|| Utc::now().naive_utc().date() - chrono::Duration::days(30));
        let end = end_date.unwrap_or_else(|| Utc::now().naive_utc().date());

        let rows = sqlx::query(
            r#"
            SELECT
                model,
                COUNT(DISTINCT conversation_id) as total_conversations,
                COALESCE(SUM(tokens), 0) as total_tokens,
                COALESCE(SUM(CASE WHEN role = 'user' THEN 1 ELSE 0 END), 0) as request_count
            FROM messages
            WHERE DATE(created_at) BETWEEN ? AND ?
            GROUP BY model
            ORDER BY total_tokens DESC
            "#,
        )
        .bind(start.to_string())
        .bind(end.to_string())
        .fetch_all(&self.pool)
        .await?;

        let stats = rows
            .iter()
            .map(|row| ModelStats {
                model: row.get("model"),
                total_conversations: row.get("total_conversations"),
                total_tokens: row.get::<i64, _>("total_tokens") as i32,
                request_count: row.get::<i64, _>("request_count") as i32,
            })
            .collect();

        Ok(stats)
    }

    /// Get overall statistics overview
    pub async fn get_stats_overview(&self) -> Result<StatsOverview> {
        let total_conversations: i32 = sqlx::query_scalar("SELECT COUNT(*) FROM conversations")
            .fetch_one(&self.pool)
            .await?;

        let total_messages: i32 = sqlx::query_scalar("SELECT COUNT(*) FROM messages")
            .fetch_one(&self.pool)
            .await?;

        let total_tokens: i64 =
            sqlx::query_scalar("SELECT COALESCE(SUM(total_tokens), 0) FROM usage_stats")
                .fetch_one(&self.pool)
                .await?;

        let total_requests: i32 =
            sqlx::query_scalar("SELECT COALESCE(SUM(total_requests), 0) FROM usage_stats")
                .fetch_one(&self.pool)
                .await?;

        Ok(StatsOverview {
            total_conversations,
            total_messages,
            total_tokens,
            total_requests,
        })
    }

    /// Get conversation counts grouped by date
    pub async fn get_conversation_counts_by_date(
        &self,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
    ) -> Result<Vec<(String, i32)>> {
        let start = start_date
            .unwrap_or_else(|| Utc::now().naive_utc().date() - chrono::Duration::days(30));
        let end = end_date.unwrap_or_else(|| Utc::now().naive_utc().date());

        let rows = sqlx::query(
            r#"
            SELECT DATE(created_at) as date, COUNT(*) as count
            FROM conversations
            WHERE DATE(created_at) BETWEEN ? AND ?
            GROUP BY DATE(created_at)
            ORDER BY date
            "#,
        )
        .bind(start.to_string())
        .bind(end.to_string())
        .fetch_all(&self.pool)
        .await?;

        let counts = rows
            .iter()
            .map(|row| (row.get::<String, _>("date"), row.get::<i32, _>("count")))
            .collect();

        Ok(counts)
    }

    /// Get conversation counts grouped by date and model
    pub async fn get_conversation_counts_by_date_and_model(
        &self,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
    ) -> Result<Vec<(String, String, i32)>> {
        let start = start_date
            .unwrap_or_else(|| Utc::now().naive_utc().date() - chrono::Duration::days(30));
        let end = end_date.unwrap_or_else(|| Utc::now().naive_utc().date());

        let rows = sqlx::query(
            r#"
            SELECT DATE(created_at) as date, model, COUNT(*) as count
            FROM conversations
            WHERE DATE(created_at) BETWEEN ? AND ?
            GROUP BY DATE(created_at), model
            ORDER BY date, model
            "#,
        )
        .bind(start.to_string())
        .bind(end.to_string())
        .fetch_all(&self.pool)
        .await?;

        let counts = rows
            .iter()
            .map(|row| {
                (
                    row.get::<String, _>("date"),
                    row.get::<String, _>("model"),
                    row.get::<i32, _>("count"),
                )
            })
            .collect();

        Ok(counts)
    }

    /// Get conversation counts grouped by date and subagent
    pub async fn get_conversation_counts_by_date_and_subagent(
        &self,
        start_date: Option<NaiveDate>,
        end_date: Option<NaiveDate>,
    ) -> Result<Vec<(String, String, i32)>> {
        let start = start_date
            .unwrap_or_else(|| Utc::now().naive_utc().date() - chrono::Duration::days(30));
        let end = end_date.unwrap_or_else(|| Utc::now().naive_utc().date());

        let rows = sqlx::query(
            r#"
            SELECT DATE(created_at) as date,
                   COALESCE(NULLIF(subagent, ''), 'default') as subagent,
                   COUNT(*) as count
            FROM conversations
            WHERE DATE(created_at) BETWEEN ? AND ?
            GROUP BY DATE(created_at), COALESCE(NULLIF(subagent, ''), 'default')
            ORDER BY date, subagent
            "#,
        )
        .bind(start.to_string())
        .bind(end.to_string())
        .fetch_all(&self.pool)
        .await?;

        let counts = rows
            .iter()
            .map(|row| {
                (
                    row.get::<String, _>("date"),
                    row.get::<String, _>("subagent"),
                    row.get::<i32, _>("count"),
                )
            })
            .collect();

        Ok(counts)
    }
}

/// Get the database path for the current directory
pub fn get_database_path() -> Result<PathBuf> {
    // Get current directory
    let current_dir = std::env::current_dir()?;
    let current_dir_str = current_dir.to_string_lossy();

    // Create slug from current directory path
    let slug = create_slug_from_path(&current_dir_str);

    // Get home directory
    let home_dir =
        dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not determine home directory"))?;

    // Create .aixplosion directory path
    let aixplosion_dir = home_dir.join(".aixplosion");

    // Create database file path
    let db_path = aixplosion_dir.join(format!("{}.db", slug));

    debug!(
        "Database path for directory '{}': {}",
        current_dir_str,
        db_path.display()
    );

    Ok(db_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_slug_from_path() {
        // Test basic path
        assert_eq!(
            create_slug_from_path("/home/user/projects/myapp"),
            "user_projects_myapp"
        );

        // Test Windows path
        assert_eq!(
            create_slug_from_path("C:\\Users\\User\\Documents\\project"),
            "user_documents_project"
        );

        // Test short path
        assert_eq!(create_slug_from_path("myproject"), "myproject");

        // Test path with special characters
        assert_eq!(
            create_slug_from_path("/path/with-special@chars#123"),
            "path_with_special_chars_123"
        );

        // Test empty path
        assert_eq!(create_slug_from_path(""), "default");

        // Test very long path
        let long_path = "/".to_string() + &"a".repeat(200);
        let slug = create_slug_from_path(&long_path);
        assert!(slug.len() <= 100);
    }
}
