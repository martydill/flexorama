use chrono::{DateTime, Duration, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// CSRF token with expiration time
#[derive(Debug, Clone)]
struct CsrfToken {
    token: String,
    expires_at: DateTime<Utc>,
}

/// Manages CSRF tokens for the web application
#[derive(Clone)]
pub struct CsrfManager {
    tokens: Arc<RwLock<HashMap<String, CsrfToken>>>,
    token_lifetime: Duration,
}

impl CsrfManager {
    /// Creates a new CSRF manager with default token lifetime of 1 hour
    pub fn new() -> Self {
        Self {
            tokens: Arc::new(RwLock::new(HashMap::new())),
            token_lifetime: Duration::hours(1),
        }
    }

    /// Creates a new CSRF manager with custom token lifetime
    pub fn with_lifetime(lifetime: Duration) -> Self {
        Self {
            tokens: Arc::new(RwLock::new(HashMap::new())),
            token_lifetime: lifetime,
        }
    }

    /// Generates a new CSRF token
    pub async fn generate_token(&self) -> String {
        let token = Uuid::new_v4().to_string();
        let expires_at = Utc::now() + self.token_lifetime;

        let csrf_token = CsrfToken {
            token: token.clone(),
            expires_at,
        };

        let mut tokens = self.tokens.write().await;
        tokens.insert(token.clone(), csrf_token);

        // Clean up expired tokens (simple cleanup on every generation)
        self.cleanup_expired_tokens(&mut tokens).await;

        token
    }

    /// Validates a CSRF token
    pub async fn validate_token(&self, token: &str) -> bool {
        let mut tokens = self.tokens.write().await;

        // Check if token exists and is not expired
        if let Some(csrf_token) = tokens.get(token) {
            if csrf_token.expires_at > Utc::now() {
                return true;
            }
            // Token expired, remove it
            tokens.remove(token);
        }

        false
    }

    /// Consumes (validates and removes) a CSRF token
    /// This is the recommended approach for state-changing operations
    pub async fn consume_token(&self, token: &str) -> bool {
        let mut tokens = self.tokens.write().await;

        // Check if token exists and is not expired
        if let Some(csrf_token) = tokens.remove(token) {
            if csrf_token.expires_at > Utc::now() {
                return true;
            }
        }

        false
    }

    /// Removes expired tokens from the store
    async fn cleanup_expired_tokens(&self, tokens: &mut HashMap<String, CsrfToken>) {
        let now = Utc::now();
        tokens.retain(|_, token| token.expires_at > now);
    }

    /// Returns the number of active tokens (for debugging/monitoring)
    pub async fn token_count(&self) -> usize {
        self.tokens.read().await.len()
    }
}

impl Default for CsrfManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_generate_and_validate_token() {
        let manager = CsrfManager::new();
        let token = manager.generate_token().await;

        assert!(manager.validate_token(&token).await);
    }

    #[tokio::test]
    async fn test_invalid_token() {
        let manager = CsrfManager::new();
        assert!(!manager.validate_token("invalid-token").await);
    }

    #[tokio::test]
    async fn test_consume_token() {
        let manager = CsrfManager::new();
        let token = manager.generate_token().await;

        // First consumption should succeed
        assert!(manager.consume_token(&token).await);

        // Second consumption should fail (token already consumed)
        assert!(!manager.consume_token(&token).await);
    }

    #[tokio::test]
    async fn test_expired_token() {
        let manager = CsrfManager::with_lifetime(Duration::milliseconds(-1));
        let token = manager.generate_token().await;

        // Wait a bit to ensure token is expired
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        assert!(!manager.validate_token(&token).await);
    }

    #[tokio::test]
    async fn test_token_count() {
        let manager = CsrfManager::new();

        assert_eq!(manager.token_count().await, 0);

        let _token1 = manager.generate_token().await;
        assert_eq!(manager.token_count().await, 1);

        let token2 = manager.generate_token().await;
        assert_eq!(manager.token_count().await, 2);

        manager.consume_token(&token2).await;
        assert_eq!(manager.token_count().await, 1);
    }
}
