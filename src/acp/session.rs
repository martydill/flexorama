use crate::agent::Agent;
use crate::config::Config;
use agent_client_protocol_schema::SessionId;
use anyhow::Result;
use log::{info, warn};
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Represents an active ACP session with its own agent and conversation
pub struct Session {
    /// Unique session identifier
    pub session_id: SessionId,

    /// The agent for this session (each session has its own agent instance)
    pub agent: Arc<Mutex<Agent>>,

    /// Cancellation flag for this session
    pub cancellation_flag: Arc<AtomicBool>,

    /// The conversation ID in the database for this session
    pub conversation_id: Option<String>,
}

impl Session {
    /// Create a new session with its own agent
    pub async fn new(
        session_id: SessionId,
        config: &Config,
        model: &str,
        yolo_mode: bool,
        plan_mode: bool,
    ) -> Result<Self> {
        info!("Creating new session: {}", session_id);

        // Create a new agent for this session
        let mut agent = Agent::new(config.clone(), model.to_string(), yolo_mode, plan_mode);

        // Suppress output in ACP mode
        agent.set_suppress_output(true);

        // Start a new conversation in the database
        let conversation_id = agent.start_new_conversation().await.ok();

        if let Some(ref conv_id) = conversation_id {
            info!(
                "Session {} created with conversation ID: {}",
                session_id, conv_id
            );
        } else {
            info!(
                "Session {} created without database conversation",
                session_id
            );
        }

        Ok(Self {
            session_id,
            agent: Arc::new(Mutex::new(agent)),
            cancellation_flag: Arc::new(AtomicBool::new(false)),
            conversation_id,
        })
    }

    /// Cancel this session
    pub fn cancel(&self) {
        info!("Cancelling session: {}", self.session_id);
        self.cancellation_flag
            .store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Manages all active ACP sessions
pub struct SessionManager {
    /// Map of session IDs to sessions
    sessions: Arc<Mutex<HashMap<String, Arc<Session>>>>,

    /// Configuration for creating new sessions
    config: Config,

    /// Default model
    model: String,

    /// Yolo mode flag
    yolo_mode: bool,

    /// Plan mode flag
    plan_mode: bool,
}

impl SessionManager {
    /// Create a new session manager
    pub fn new(config: Config, model: String, yolo_mode: bool, plan_mode: bool) -> Self {
        Self {
            sessions: Arc::new(Mutex::new(HashMap::new())),
            config,
            model,
            yolo_mode,
            plan_mode,
        }
    }

    /// Create a new session
    pub async fn create_session(&self, session_id: SessionId) -> Result<Arc<Session>> {
        let session = Arc::new(
            Session::new(
                session_id.clone(),
                &self.config,
                &self.model,
                self.yolo_mode,
                self.plan_mode,
            )
            .await?,
        );

        let mut sessions = self.sessions.lock().await;
        sessions.insert(session_id.to_string(), session.clone());

        info!("Session {} registered in manager", session_id);
        Ok(session)
    }

    /// Get an existing session by ID
    pub async fn get_session(&self, session_id: &str) -> Option<Arc<Session>> {
        let sessions = self.sessions.lock().await;
        sessions.get(session_id).cloned()
    }

    /// Remove a session
    pub async fn remove_session(&self, session_id: &str) -> Option<Arc<Session>> {
        let mut sessions = self.sessions.lock().await;
        let removed = sessions.remove(session_id);

        if removed.is_some() {
            info!("Session {} removed from manager", session_id);
        } else {
            warn!("Attempted to remove non-existent session: {}", session_id);
        }

        removed
    }

    /// Get the number of active sessions
    pub async fn session_count(&self) -> usize {
        let sessions = self.sessions.lock().await;
        sessions.len()
    }

    /// Clean up all sessions
    pub async fn clear_all_sessions(&self) {
        let mut sessions = self.sessions.lock().await;
        let count = sessions.len();
        sessions.clear();
        info!("Cleared all {} sessions", count);
    }

    /// List all active session IDs
    pub async fn list_session_ids(&self) -> Vec<String> {
        let sessions = self.sessions.lock().await;
        sessions.keys().cloned().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Provider;

    fn create_test_config() -> Config {
        Config {
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
        }
    }

    #[tokio::test]
    async fn test_session_manager_create() {
        let config = create_test_config();
        let manager = SessionManager::new(config, "test-model".to_string(), false, false);

        let session_id = SessionId::from("test-session-1".to_string());
        let result = manager.create_session(session_id.clone()).await;

        assert!(result.is_ok());
        assert_eq!(manager.session_count().await, 1);
    }

    #[tokio::test]
    async fn test_session_manager_get() {
        let config = create_test_config();
        let manager = SessionManager::new(config, "test-model".to_string(), false, false);

        let session_id = SessionId::from("test-session-2".to_string());
        let created = manager.create_session(session_id.clone()).await.unwrap();

        let retrieved = manager.get_session(&session_id.to_string()).await;
        assert!(retrieved.is_some());

        let retrieved_session = retrieved.unwrap();
        assert_eq!(retrieved_session.session_id, created.session_id);
    }

    #[tokio::test]
    async fn test_session_manager_remove() {
        let config = create_test_config();
        let manager = SessionManager::new(config, "test-model".to_string(), false, false);

        let session_id = SessionId::from("test-session-3".to_string());
        manager.create_session(session_id.clone()).await.unwrap();

        assert_eq!(manager.session_count().await, 1);

        let removed = manager.remove_session(&session_id.to_string()).await;
        assert!(removed.is_some());
        assert_eq!(manager.session_count().await, 0);
    }

    #[tokio::test]
    async fn test_session_manager_clear_all() {
        let config = create_test_config();
        let manager = SessionManager::new(config, "test-model".to_string(), false, false);

        for i in 1..=3 {
            let session_id = SessionId::from(format!("test-session-{}", i));
            manager.create_session(session_id).await.unwrap();
        }

        assert_eq!(manager.session_count().await, 3);

        manager.clear_all_sessions().await;
        assert_eq!(manager.session_count().await, 0);
    }

    #[tokio::test]
    async fn test_session_manager_list_session_ids() {
        let config = create_test_config();
        let manager = SessionManager::new(config, "test-model".to_string(), false, false);

        let session_ids = vec!["session-1", "session-2", "session-3"];

        for id in &session_ids {
            let session_id = SessionId::from(id.to_string());
            manager.create_session(session_id).await.unwrap();
        }

        let listed = manager.list_session_ids().await;
        assert_eq!(listed.len(), 3);

        for id in session_ids {
            assert!(listed.contains(&id.to_string()));
        }
    }

    #[tokio::test]
    async fn test_session_cancel() {
        let config = create_test_config();
        let session_id = SessionId::from("test-session-cancel".to_string());

        let session = Session::new(session_id, &config, "test-model", false, false)
            .await
            .unwrap();

        assert!(!session
            .cancellation_flag
            .load(std::sync::atomic::Ordering::SeqCst));

        session.cancel();

        assert!(session
            .cancellation_flag
            .load(std::sync::atomic::Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_session_manager_get_nonexistent() {
        let config = create_test_config();
        let manager = SessionManager::new(config, "test-model".to_string(), false, false);

        let result = manager.get_session("nonexistent-session").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_session_manager_remove_nonexistent() {
        let config = create_test_config();
        let manager = SessionManager::new(config, "test-model".to_string(), false, false);

        let result = manager.remove_session("nonexistent-session").await;
        assert!(result.is_none());
    }
}
