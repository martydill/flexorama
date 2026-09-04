use anyhow::Result;
use colored::Colorize;
use dialoguer::Select;
use futures_util::future::BoxFuture;
use glob::Pattern;
use log::{debug, error, info, warn};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;

use crate::hooks::{HookAction, HookManager};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BashSecurity {
    /// List of allowed command patterns (supports wildcards)
    pub allowed_commands: HashSet<String>,
    /// List of explicitly denied command patterns (supports wildcards)
    pub denied_commands: HashSet<String>,
    /// Whether to ask for permission for unknown commands
    pub ask_for_permission: bool,
    /// Whether to enable security mode at all
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileSecurity {
    /// Whether to ask for permission for file create/edit operations
    pub ask_for_permission: bool,
    /// Whether to enable file security mode at all
    pub enabled: bool,
    /// Whether to allow all file operations this session
    pub allow_all_session: bool,
}

impl Default for BashSecurity {
    fn default() -> Self {
        Self {
            // Default safe commands
            allowed_commands: HashSet::from([
                "ls".to_string(),
                "pwd".to_string(),
                "cd".to_string(),
                "cat".to_string(),
                "head".to_string(),
                "tail".to_string(),
                "grep".to_string(),
                "find".to_string(),
                "which".to_string(),
                "whereis".to_string(),
                "echo".to_string(),
                "date".to_string(),
                "whoami".to_string(),
                "id".to_string(),
                "uname".to_string(),
                "df".to_string(),
                "du".to_string(),
                "wc".to_string(),
                "sort".to_string(),
                "uniq".to_string(),
                "cut".to_string(),
                "awk".to_string(),
                "sed".to_string(),
                "git status".to_string(),
                "git log".to_string(),
                "git diff".to_string(),
                "git show".to_string(),
                "git branch".to_string(),
                "git tag".to_string(),
                "cargo check".to_string(),
                "cargo test".to_string(),
                "cargo build".to_string(),
                "cargo clippy".to_string(),
                "rustc --version".to_string(),
                "node --version".to_string(),
                "npm --version".to_string(),
                "python --version".to_string(),
                "python3 --version".to_string(),
                "pip --version".to_string(),
                "pip3 --version".to_string(),
            ]),
            denied_commands: HashSet::from([
                "rm *".to_string(),
                "sudo rm *".to_string(),
                "format".to_string(),
                "fdisk".to_string(),
                "mkfs".to_string(),
                "dd".to_string(),
                "shutdown".to_string(),
                "reboot".to_string(),
                "halt".to_string(),
                "poweroff".to_string(),
                "passwd".to_string(),
                "su".to_string(),
                "sudo su".to_string(),
                "chmod 777 *".to_string(),
                "chown *".to_string(),
                "mv *".to_string(),
                "cp *".to_string(),
            ]),
            ask_for_permission: true,
            enabled: true,
        }
    }
}

impl Default for FileSecurity {
    fn default() -> Self {
        Self {
            ask_for_permission: true,
            enabled: true,
            allow_all_session: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum PermissionResult {
    Allowed,
    Denied,
    RequiresPermission,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FilePermissionResult {
    Allowed,
    Denied,
    RequiresPermission,
}

pub struct BashSecurityManager {
    security: BashSecurity,
    permission_handler: Option<PermissionHandler>,
    hook_manager: Option<Arc<HookManager>>,
    conversation_id: Option<String>,
    model: String,
}

pub struct FileSecurityManager {
    security: FileSecurity,
    permission_handler: Option<PermissionHandler>,
    hook_manager: Option<Arc<HookManager>>,
    conversation_id: Option<String>,
    model: String,
}

#[derive(Debug, Clone)]
pub enum PermissionKind {
    Bash,
    File,
}

#[derive(Debug, Clone)]
pub struct PermissionPrompt {
    pub kind: PermissionKind,
    pub summary: String,
    pub detail: String,
    pub options: Vec<String>,
}

pub type PermissionHandler =
    Arc<dyn Fn(PermissionPrompt) -> BoxFuture<'static, Option<usize>> + Send + Sync>;

impl FileSecurityManager {
    pub fn new(security: FileSecurity) -> Self {
        Self {
            security,
            permission_handler: None,
            hook_manager: None,
            conversation_id: None,
            model: String::new(),
        }
    }

    /// Set the hook manager for permission request hooks
    pub fn set_hook_manager(
        &mut self,
        hook_manager: Arc<HookManager>,
        conversation_id: Option<String>,
        model: String,
    ) {
        self.hook_manager = Some(hook_manager);
        self.conversation_id = conversation_id;
        self.model = model;
    }

    /// Update the conversation ID (e.g., when switching conversations)
    pub fn set_conversation_id(&mut self, conversation_id: Option<String>) {
        self.conversation_id = conversation_id;
    }

    /// Check if a file operation is allowed
    pub fn check_file_permission(&mut self, operation: &str, path: &str) -> FilePermissionResult {
        if !self.security.enabled {
            debug!(
                "File security is disabled, allowing operation: {} on {}",
                operation, path
            );
            return FilePermissionResult::Allowed;
        }

        // If allow all session is enabled, allow all file operations
        if self.security.allow_all_session {
            debug!(
                "Allow all session is enabled, allowing operation: {} on {}",
                operation, path
            );
            return FilePermissionResult::Allowed;
        }

        // If ask_for_permission is enabled, require permission for all file operations
        if self.security.ask_for_permission {
            info!(
                "File operation '{}' on '{}' requires user permission",
                operation, path
            );
            FilePermissionResult::RequiresPermission
        } else {
            debug!(
                "File operation '{}' on '{}' allowed (ask_for_permission is false)",
                operation, path
            );
            FilePermissionResult::Allowed
        }
    }

    /// Ask user for permission to perform a file operation
    pub async fn ask_file_permission(
        &mut self,
        operation: &str,
        path: &str,
    ) -> Result<Option<bool>> {
        if !self.security.ask_for_permission {
            return Ok(Some(true));
        }

        // Check PermissionRequest hook first
        if let Some(hook_manager) = &self.hook_manager {
            let detail = format!("Operation: {}\nPath: {}", operation, path);
            let hook_decision = hook_manager
                .run_permission_request(
                    "file",
                    operation,
                    &detail,
                    self.conversation_id.as_deref(),
                    &self.model,
                )
                .await?;

            // Only act on explicit decisions from hooks
            if hook_decision.explicit_decision {
                match hook_decision.action {
                    HookAction::Abort => {
                        // Hook explicitly denied the permission
                        info!(
                            "PermissionRequest hook denied file operation: {} on {}",
                            operation, path
                        );
                        return Ok(None);
                    }
                    HookAction::Continue => {
                        // Hook explicitly approved the permission
                        info!(
                            "PermissionRequest hook approved file operation: {} on {}",
                            operation, path
                        );
                        return Ok(Some(false)); // Allowed this time only
                    }
                }
            }
            // If no explicit decision, fall through to user prompt
        }

        let options = vec![
            "Allow this operation only".to_string(),
            "Allow all file operations this session".to_string(),
            "Deny this operation".to_string(),
        ];

        if let Some(handler) = &self.permission_handler {
            let prompt = PermissionPrompt {
                kind: PermissionKind::File,
                summary: "File operation requires permission".to_string(),
                detail: format!(
                    "Operation: {}
Path: {}",
                    operation, path
                ),
                options,
            };
            let handler = handler.clone();
            let selection = (handler)(prompt).await;

            return match selection {
                Some(idx) => {
                    self.handle_file_permission_selection(idx, operation, path)
                        .await
                }
                None => Ok(None),
            };
        }

        app_println!();
        app_println!("{}", "🛡️ File Operation Security Check".yellow().bold());
        app_println!("The following file operation requires permission:");
        app_println!("  Operation: {}", operation.cyan());
        app_println!("  Path: {}", path.cyan());
        app_println!();

        // Use tokio::task::spawn_blocking without timeout to wait indefinitely for user input
        let options_clone = options.clone();
        let result = tokio::task::spawn_blocking(move || {
            Select::new()
                .with_prompt("Select an option")
                .items(&options_clone)
                .default(0) // Default to "Allow this operation only"
                .interact()
        })
        .await;

        match result {
            Ok(Ok(selection)) => {
                self.handle_file_permission_selection(selection, operation, path)
                    .await
            }
            Ok(Err(e)) => {
                error!("Failed to get user input: {}", e);
                app_println!(
                    "{} Failed to get user input, denying file operation for safety",
                    "🛡️".yellow()
                );
                Ok(None) // Deny for safety
            }
            Err(e) => {
                error!("Task join error: {}", e);
                app_println!(
                    "{} Failed to get user input, denying file operation for safety",
                    "🛡️".yellow()
                );
                Ok(None) // Deny for safety
            }
        }
    }

    /// Handle the user's file permission selection
    async fn handle_file_permission_selection(
        &mut self,
        selection: usize,
        _operation: &str,
        _path: &str,
    ) -> Result<Option<bool>> {
        match selection {
            0 => {
                // Allow this operation only
                app_println!("{} File operation allowed for this time only", "✅".green());
                Ok(Some(false)) // Allow but don't change session settings
            }
            1 => {
                // Allow all file operations this session
                app_println!(
                    "{} All file operations allowed for this session",
                    "✅".green()
                );
                self.security.allow_all_session = true;
                Ok(Some(true)) // Allow and set session flag
            }
            2 => {
                // Deny this operation
                app_println!("{} File operation denied", "❌".red());
                Ok(None) // Deny
            }
            _ => {
                app_println!(
                    "{} Invalid selection, denying file operation for safety",
                    "⚠️".yellow()
                );
                Ok(None) // Deny for safety
            }
        }
    }

    /// Get current file security settings
    pub fn get_file_security(&self) -> &FileSecurity {
        &self.security
    }

    /// Update file security settings
    pub fn update_file_security(&mut self, security: FileSecurity) {
        self.security = security;
    }

    pub fn set_permission_handler(&mut self, handler: Option<PermissionHandler>) {
        self.permission_handler = handler;
    }

    /// Reset allow all session flag
    pub fn reset_session_permissions(&mut self) {
        self.security.allow_all_session = false;
    }

    /// Display current file security settings
    pub fn display_file_permissions(&self) {
        app_println!();
        app_println!("{}", "🔒 File Security Settings".cyan().bold());
        app_println!();

        app_println!("{}", "Security Status:".green().bold());
        let status = if self.security.enabled {
            "✅ Enabled".green().to_string()
        } else {
            "❌ Disabled".red().to_string()
        };
        app_println!("  File Security: {}", status);

        let ask_status = if self.security.ask_for_permission {
            "✅ Enabled".green().to_string()
        } else {
            "❌ Disabled".red().to_string()
        };
        app_println!("  Ask for permission: {}", ask_status);

        let session_status = if self.security.allow_all_session {
            "✅ Enabled".green().to_string()
        } else {
            "❌ Disabled".red().to_string()
        };
        app_println!("  Allow all this session: {}", session_status);
        app_println!();

        app_println!("{}", "File Security Tips:".yellow().bold());
        app_println!("  • Enable 'ask for permission' for better security");
        app_println!("  • Use 'Allow this operation only' for one-off edits");
        app_println!("  • Use 'Allow all file operations this session' for trusted sessions");
        app_println!(
            "  • File operations include: write_file, edit_file, create_directory, delete_file"
        );
        app_println!("  • Read operations (read_file, list_directory) are always allowed");
        app_println!();
    }
}

impl BashSecurityManager {
    pub fn new(security: BashSecurity) -> Self {
        Self {
            security,
            permission_handler: None,
            hook_manager: None,
            conversation_id: None,
            model: String::new(),
        }
    }

    /// Set the hook manager for permission request hooks
    pub fn set_hook_manager(
        &mut self,
        hook_manager: Arc<HookManager>,
        conversation_id: Option<String>,
        model: String,
    ) {
        self.hook_manager = Some(hook_manager);
        self.conversation_id = conversation_id;
        self.model = model;
    }

    /// Update the conversation ID (e.g., when switching conversations)
    pub fn set_conversation_id(&mut self, conversation_id: Option<String>) {
        self.conversation_id = conversation_id;
    }

    /// Check if a command is allowed to execute
    pub fn check_command_permission(&self, command: &str) -> PermissionResult {
        if !self.security.enabled {
            debug!("Security is disabled, allowing command: {}", command);
            return PermissionResult::Allowed;
        }

        // Extract the base command (first word) and full command for checking
        let base_command = command.split_whitespace().next().unwrap_or("").trim();

        debug!("Checking permission for command: {}", command);
        debug!("Base command: {}", base_command);

        // Check denied patterns first (more restrictive)
        for denied_pattern in &self.security.denied_commands {
            if self.matches_pattern(command, denied_pattern)
                || self.matches_pattern(base_command, denied_pattern)
            {
                warn!(
                    "Command '{}' matches denied pattern: {}",
                    command, denied_pattern
                );
                return PermissionResult::Denied;
            }
        }

        // Check allowed patterns
        for allowed_pattern in &self.security.allowed_commands {
            if self.matches_pattern(command, allowed_pattern)
                || self.matches_pattern(base_command, allowed_pattern)
            {
                debug!(
                    "Command '{}' matches allowed pattern: {}",
                    command, allowed_pattern
                );
                return PermissionResult::Allowed;
            }
        }

        // If not explicitly allowed or denied, decide based on ask_for_permission setting
        if self.security.ask_for_permission {
            info!("Command '{}' requires user permission", command);
            PermissionResult::RequiresPermission
        } else {
            warn!(
                "Command '{}' not in allowlist and ask_for_permission is false",
                command
            );
            PermissionResult::Denied
        }
    }

    /// Ask user for permission to execute a command
    pub async fn ask_permission(&mut self, command: &str) -> Result<Option<bool>> {
        if !self.security.ask_for_permission {
            return Ok(None);
        }

        // Check PermissionRequest hook first
        if let Some(hook_manager) = &self.hook_manager {
            let hook_decision = hook_manager
                .run_permission_request(
                    "bash",
                    "bash",
                    command,
                    self.conversation_id.as_deref(),
                    &self.model,
                )
                .await?;

            // Only act on explicit decisions from hooks
            if hook_decision.explicit_decision {
                match hook_decision.action {
                    HookAction::Abort => {
                        // Hook explicitly denied the permission
                        info!("PermissionRequest hook denied bash command: {}", command);
                        return Ok(None);
                    }
                    HookAction::Continue => {
                        // Hook explicitly approved the permission
                        info!("PermissionRequest hook approved bash command: {}", command);
                        return Ok(Some(false)); // Allowed this time only
                    }
                }
            }
            // If no explicit decision, fall through to user prompt
        }

        let options = self.generate_permission_options(command);
        if let Some(handler) = &self.permission_handler {
            let prompt = PermissionPrompt {
                kind: PermissionKind::Bash,
                summary: "Command requires permission".to_string(),
                detail: command.to_string(),
                options,
            };
            let handler = handler.clone();
            let selection = (handler)(prompt).await;

            return match selection {
                Some(idx) => self.handle_permission_selection(idx, command).await,
                None => Ok(None),
            };
        }

        app_println!();
        app_println!("{}", "🛡️ Security Check".yellow().bold());
        app_println!("The following command is not in the allowlist:");
        app_println!("  {}", command.cyan());
        app_println!();

        // Use tokio::task::spawn_blocking without timeout to wait indefinitely for user input
        let options_clone = options.clone();
        let result = tokio::task::spawn_blocking(move || {
            Select::new()
                .with_prompt("Select an option")
                .items(&options_clone)
                .default(0) // Default to "Allow this time only"
                .interact()
        })
        .await;

        match result {
            Ok(Ok(selection)) => self.handle_permission_selection(selection, command).await,
            Ok(Err(e)) => {
                error!("Failed to get user input: {}", e);
                app_println!(
                    "{} Failed to get user input, denying command for safety",
                    "🛡️".yellow()
                );
                Ok(None) // Deny for safety
            }
            Err(e) => {
                error!("Task join error: {}", e);
                app_println!(
                    "{} Failed to get user input, denying command for safety",
                    "🛡️".yellow()
                );
                Ok(None) // Deny for safety
            }
        }
    }

    /// Generate permission options based on the command structure
    fn generate_permission_options(&self, command: &str) -> Vec<String> {
        let mut options = vec![
            "Allow this time only (don't add to allowlist)".to_string(),
            "Allow and add to allowlist".to_string(),
        ];

        // Add wildcard option if command has parameters
        if self.has_parameters(command) {
            let wildcard_pattern = self.generate_wildcard_pattern(command);
            options.push(format!(
                "Allow and add to allowlist with wildcard: '{}'",
                wildcard_pattern
            ));
        }

        options.push("Deny this command".to_string());
        options
    }

    /// Check if a command has parameters (arguments beyond the base command)
    fn has_parameters(&self, command: &str) -> bool {
        command.split_whitespace().count() > 1
    }

    /// Generate a wildcard pattern for the command
    fn generate_wildcard_pattern(&self, command: &str) -> String {
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return command.to_string();
        }

        // Replace all parameters after the base command with *
        format!("{} *", parts[0])
    }

    /// Handle the user's permission selection
    async fn handle_permission_selection(
        &mut self,
        selection: usize,
        command: &str,
    ) -> Result<Option<bool>> {
        match selection {
            0 => {
                // Allow this time only
                app_println!("{} Command allowed for this time only", "✅".green());
                Ok(Some(false)) // Allow but don't add to allowlist
            }
            1 => {
                // Allow and add to allowlist
                app_println!("{} Command allowed and added to allowlist", "✅".green());
                self.add_to_allowlist(command.to_string());
                Ok(Some(true)) // Allow and add to allowlist
            }
            2 => {
                if self.has_parameters(command) {
                    // Allowlist with wildcard
                    let wildcard_pattern = self.generate_wildcard_pattern(command);
                    app_println!(
                        "{} Command wildcard pattern added to allowlist: '{}'",
                        "✅".green(),
                        wildcard_pattern.cyan()
                    );
                    self.add_to_allowlist(wildcard_pattern);
                    Ok(Some(true)) // Allow and add wildcard to allowlist
                } else {
                    // No wildcard option, this is the deny option
                    app_println!("{} Command denied", "❌".red());
                    Ok(None) // Deny
                }
            }
            3 => {
                // Deny this command (only present when there are parameters)
                app_println!("{} Command denied", "❌".red());
                Ok(None) // Deny
            }
            _ => {
                app_println!(
                    "{} Invalid selection, denying command for safety",
                    "⚠️".yellow()
                );
                Ok(None) // Deny for safety
            }
        }
    }

    /// Add a command to the allowlist
    pub fn add_to_allowlist(&mut self, command: String) {
        self.security.allowed_commands.insert(command);
    }

    /// Add a command to the denylist
    pub fn add_to_denylist(&mut self, command: String) {
        self.security.denied_commands.insert(command);
    }

    /// Remove a command from the allowlist
    pub fn remove_from_allowlist(&mut self, command: &str) -> bool {
        self.security.allowed_commands.remove(command)
    }

    /// Remove a command from the denylist
    pub fn remove_from_denylist(&mut self, command: &str) -> bool {
        self.security.denied_commands.remove(command)
    }

    /// Get current security settings
    pub fn get_security(&self) -> &BashSecurity {
        &self.security
    }

    /// Update security settings
    pub fn update_security(&mut self, security: BashSecurity) {
        self.security = security;
    }

    pub fn set_permission_handler(&mut self, handler: Option<PermissionHandler>) {
        self.permission_handler = handler;
    }

    /// Check if a command matches a pattern (supports wildcards)
    fn matches_pattern(&self, command: &str, pattern: &str) -> bool {
        // Handle exact match
        if command == pattern {
            return true;
        }

        // Handle wildcard patterns
        if pattern.contains('*') || pattern.contains('?') {
            match Pattern::new(pattern) {
                Ok(glob_pattern) => {
                    if glob_pattern.matches(command) {
                        return true;
                    }
                }
                Err(e) => {
                    debug!("Invalid glob pattern '{}': {}", pattern, e);
                }
            }
        }

        // Handle prefix match (e.g., "git" matches "git status")
        if command.starts_with(&format!("{} ", pattern)) || command == pattern {
            return true;
        }

        false
    }

    /// Display current permissions
    pub fn display_permissions(&self) {
        app_println!();
        app_println!("{}", "🔒 Bash Security Settings".cyan().bold());
        app_println!();

        app_println!("{}", "Security Status:".green().bold());
        let status = if self.security.enabled {
            "✅ Enabled".green().to_string()
        } else {
            "❌ Disabled".red().to_string()
        };
        app_println!("  Security: {}", status);

        let ask_status = if self.security.ask_for_permission {
            "✅ Enabled".green().to_string()
        } else {
            "❌ Disabled".red().to_string()
        };
        app_println!("  Ask for permission: {}", ask_status);
        app_println!();

        app_println!(
            "{} Allowed Commands ({}):",
            "Allowed Commands".green().bold(),
            self.security.allowed_commands.len()
        );
        if self.security.allowed_commands.is_empty() {
            app_println!("  {}", "<No commands allowed>".dimmed());
        } else {
            let mut sorted_commands: Vec<_> = self.security.allowed_commands.iter().collect();
            sorted_commands.sort();
            for command in sorted_commands {
                app_println!("  ✅ {}", command.green());
            }
        }
        app_println!();

        app_println!(
            "{} Denied Commands ({}):",
            "Denied Commands".red().bold(),
            self.security.denied_commands.len()
        );
        if self.security.denied_commands.is_empty() {
            app_println!("  {}", "<No commands denied>".dimmed());
        } else {
            let mut sorted_commands: Vec<_> = self.security.denied_commands.iter().collect();
            sorted_commands.sort();
            for command in sorted_commands {
                app_println!("  ❌ {}", command.red());
            }
        }
        app_println!();

        app_println!("{}", "Security Tips:".yellow().bold());
        app_println!("  • Use wildcards: 'git *' allows all git commands");
        app_println!("  • Be specific: 'cargo test' is safer than 'cargo *'");
        app_println!("  • Review denied commands regularly");
        app_println!("  • Enable 'ask for permission' for unknown commands");
        app_println!("  • Choose 'Allow this time only' for one-off commands");
        app_println!("  • Choose 'Allow and add to allowlist' for trusted commands");
        app_println!("  • Choose 'Allowlist with wildcard' for commands with parameters");
        app_println!(
            "  • Wildcard patterns replace parameters with * (e.g., 'curl example.com' → 'curl *')"
        );
        app_println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn security_manager_with_lists(
        allowed: &[&str],
        denied: &[&str],
        ask_for_permission: bool,
    ) -> BashSecurityManager {
        let security = BashSecurity {
            allowed_commands: allowed.iter().map(|value| (*value).to_string()).collect(),
            denied_commands: denied.iter().map(|value| (*value).to_string()).collect(),
            ask_for_permission,
            enabled: true,
        };
        BashSecurityManager::new(security)
    }

    #[test]
    fn check_command_permission_denied_takes_precedence() {
        let manager = security_manager_with_lists(&["git *"], &["git push"], true);

        assert_eq!(
            manager.check_command_permission("git push"),
            PermissionResult::Denied
        );
        assert_eq!(
            manager.check_command_permission("git status"),
            PermissionResult::Allowed
        );
    }

    #[test]
    fn check_command_permission_base_command_match_allows_subcommands() {
        let manager = security_manager_with_lists(&["git"], &[], true);

        assert_eq!(
            manager.check_command_permission("git status"),
            PermissionResult::Allowed
        );
        assert_eq!(
            manager.check_command_permission("git"),
            PermissionResult::Allowed
        );
    }

    #[test]
    fn check_command_permission_wildcard_and_invalid_glob_patterns() {
        let manager = security_manager_with_lists(&["cargo *", "git ["], &[], true);

        assert_eq!(
            manager.check_command_permission("cargo build"),
            PermissionResult::Allowed
        );
        assert_eq!(
            manager.check_command_permission("git status"),
            PermissionResult::RequiresPermission
        );
    }

    #[test]
    fn generate_permission_options_varies_with_parameters() {
        let manager = security_manager_with_lists(&[], &[], true);

        let with_params = manager.generate_permission_options("git status");
        assert!(with_params.iter().any(|option| option.contains("wildcard")));
        assert_eq!(with_params.len(), 4);

        let without_params = manager.generate_permission_options("git");
        assert!(!without_params
            .iter()
            .any(|option| option.contains("wildcard")));
        assert_eq!(without_params.len(), 3);
    }

    #[test]
    fn generate_wildcard_pattern_uses_base_command() {
        let manager = security_manager_with_lists(&[], &[], true);

        assert_eq!(
            manager.generate_wildcard_pattern("cargo build --release"),
            "cargo *"
        );
    }

    use serial_test::serial;
    use std::sync::{Arc, Mutex};

    fn bash_manager(
        allowed: &[&str],
        denied: &[&str],
        ask_for_permission: bool,
    ) -> BashSecurityManager {
        security_manager_with_lists(allowed, denied, ask_for_permission)
    }

    fn bash_handler(selection: Option<usize>) -> PermissionHandler {
        Arc::new(move |_prompt| {
            let selection = selection;
            Box::pin(async move { selection })
        })
    }

    #[derive(Default)]
    struct RecordingSink {
        lines: Mutex<Vec<(String, bool)>>,
    }

    impl crate::output::OutputSink for RecordingSink {
        fn write(&self, text: &str, is_err: bool) {
            self.lines
                .lock()
                .expect("lines lock")
                .push((text.to_string(), is_err));
        }

        fn flush(&self) {}
    }

    /// Run `f` with the global output sink capturing, returning stdout text.
    fn capture_stdout<F: FnOnce()>(f: F) -> String {
        colored::control::set_override(false);
        let sink = Arc::new(RecordingSink::default());
        crate::output::set_output_sink(sink.clone());
        f();
        crate::output::clear_output_sink();
        colored::control::unset_override();
        let guard = sink.lines.lock().unwrap();
        let text = guard
            .iter()
            .filter(|(_, is_err)| !*is_err)
            .map(|(text, _)| text.clone())
            .collect::<Vec<_>>()
            .join("");
        drop(guard);
        text
    }

    #[tokio::test]
    async fn disabled_security_allows_everything() {
        let security = BashSecurity {
            allowed_commands: Default::default(),
            denied_commands: ["rm -rf /".to_string()].into_iter().collect(),
            ask_for_permission: true,
            enabled: false,
        };
        let manager = BashSecurityManager::new(security);

        assert_eq!(
            manager.check_command_permission("rm -rf /"),
            PermissionResult::Allowed
        );
    }

    #[tokio::test]
    async fn unmatched_command_denied_when_not_asking_permission() {
        let manager = bash_manager(&[], &[], false);

        assert_eq!(
            manager.check_command_permission("curl example.com"),
            PermissionResult::Denied
        );
    }

    #[tokio::test]
    async fn question_mark_wildcard_matches_single_character() {
        let manager = bash_manager(&["git status?"], &[], true);

        assert_eq!(
            manager.check_command_permission("git statuss"),
            PermissionResult::Allowed
        );
        assert_eq!(
            manager.check_command_permission("git status"),
            PermissionResult::RequiresPermission
        );
    }

    #[tokio::test]
    async fn prefix_match_requires_word_boundary() {
        let manager = bash_manager(&["git"], &[], true);

        assert_eq!(
            manager.check_command_permission("gitx status"),
            PermissionResult::RequiresPermission
        );
    }

    #[tokio::test]
    async fn ask_permission_short_circuits_when_not_asking() {
        let mut manager = bash_manager(&[], &[], false);
        manager.set_permission_handler(Some(bash_handler(Some(1))));

        let result = manager.ask_permission("git status").await.unwrap();

        assert_eq!(result, None);
        assert!(
            !manager.get_security().allowed_commands.contains("git status"),
            "handler must not run when ask_for_permission is false"
        );
    }

    #[tokio::test]
    async fn ask_permission_allow_this_time_only() {
        let mut manager = bash_manager(&[], &[], true);
        manager.set_permission_handler(Some(bash_handler(Some(0))));

        let result = manager.ask_permission("git status").await.unwrap();

        assert_eq!(result, Some(false));
        assert!(
            !manager.get_security().allowed_commands.contains("git status"),
            "this-time-only grants must not persist"
        );
    }

    #[tokio::test]
    async fn ask_permission_allow_and_persist_to_allowlist() {
        let mut manager = bash_manager(&[], &[], true);
        manager.set_permission_handler(Some(bash_handler(Some(1))));

        let result = manager.ask_permission("git status").await.unwrap();

        assert_eq!(result, Some(true));
        assert_eq!(
            manager.check_command_permission("git status"),
            PermissionResult::Allowed,
            "allowlist grant must persist"
        );
    }

    #[tokio::test]
    async fn ask_permission_wildcard_option_adds_pattern() {
        let mut manager = bash_manager(&[], &[], true);
        manager.set_permission_handler(Some(bash_handler(Some(2))));

        let result = manager.ask_permission("cargo build --release").await.unwrap();

        assert_eq!(result, Some(true));
        assert_eq!(
            manager.check_command_permission("cargo build"),
            PermissionResult::Allowed
        );
        assert!(
            manager.get_security().allowed_commands.contains("cargo *"),
            "wildcard pattern should be stored, got {:?}",
            manager.get_security().allowed_commands
        );
    }

    #[tokio::test]
    async fn ask_permission_deny_selections() {
        // Selection 2 is "deny" for parameter-less commands
        let mut manager = bash_manager(&[], &[], true);
        manager.set_permission_handler(Some(bash_handler(Some(2))));
        assert_eq!(manager.ask_permission("git").await.unwrap(), None);

        // Selection 3 is "deny" when a wildcard option exists
        let mut manager = bash_manager(&[], &[], true);
        manager.set_permission_handler(Some(bash_handler(Some(3))));
        assert_eq!(manager.ask_permission("git status").await.unwrap(), None);
        assert_eq!(
            manager.check_command_permission("git status"),
            PermissionResult::RequiresPermission
        );
    }

    #[tokio::test]
    async fn ask_permission_invalid_selection_denies_for_safety() {
        let mut manager = bash_manager(&[], &[], true);
        manager.set_permission_handler(Some(bash_handler(Some(99))));

        assert_eq!(manager.ask_permission("git").await.unwrap(), None);
    }

    #[tokio::test]
    async fn ask_permission_dismissed_prompt_denies() {
        let mut manager = bash_manager(&[], &[], true);
        manager.set_permission_handler(Some(bash_handler(None)));

        assert_eq!(manager.ask_permission("git").await.unwrap(), None);
    }

    #[tokio::test]
    async fn allowlist_and_denylist_membership_updates() {
        let mut manager = bash_manager(&[], &[], true);

        manager.add_to_allowlist("cargo build".to_string());
        assert_eq!(
            manager.check_command_permission("cargo build"),
            PermissionResult::Allowed
        );
        assert!(manager.remove_from_allowlist("cargo build"));
        assert!(!manager.remove_from_allowlist("cargo build"));
        assert_eq!(
            manager.check_command_permission("cargo build"),
            PermissionResult::RequiresPermission
        );

        manager.add_to_denylist("rm *".to_string());
        assert_eq!(
            manager.check_command_permission("rm -rf build"),
            PermissionResult::Denied
        );
        assert!(manager.remove_from_denylist("rm *"));
        assert!(!manager.remove_from_denylist("rm *"));
    }

    #[test]
    fn update_security_replaces_settings() {
        let mut manager = bash_manager(&["git"], &[], true);
        let replacement = BashSecurity {
            allowed_commands: ["npm".to_string()].into_iter().collect(),
            denied_commands: Default::default(),
            ask_for_permission: false,
            enabled: true,
        };

        manager.update_security(replacement);

        assert_eq!(
            manager.check_command_permission("npm install"),
            PermissionResult::Allowed
        );
        assert_eq!(
            manager.check_command_permission("git status"),
            PermissionResult::Denied
        );
    }

    #[test]
    #[serial]
    fn display_permissions_prints_settings() {
        let manager = bash_manager(&["git"], &["rm *"], true);

        let out = capture_stdout(|| manager.display_permissions());

        assert!(out.contains("Bash Security Settings"), "output: {}", out);
        assert!(out.contains("git"), "allowlist should be shown: {}", out);
        assert!(out.contains("rm *"), "denylist should be shown: {}", out);
    }

    #[test]
    #[serial]
    fn display_file_permissions_prints_settings() {
        let manager = FileSecurityManager::new(FileSecurity {
            ask_for_permission: true,
            enabled: true,
            allow_all_session: false,
        });

        let out = capture_stdout(|| manager.display_file_permissions());

        assert!(out.contains("File Security Settings"), "output: {}", out);
        assert!(out.contains("Ask for permission"), "output: {}", out);
    }

    #[test]
    fn file_security_session_grant_can_be_reset() {
        let mut manager = FileSecurityManager::new(FileSecurity {
            ask_for_permission: false,
            enabled: true,
            allow_all_session: false,
        });

        manager.update_file_security(FileSecurity {
            ask_for_permission: false,
            enabled: true,
            allow_all_session: true,
        });
        assert_eq!(
            manager.check_file_permission("Write", "/tmp/x"),
            FilePermissionResult::Allowed
        );
        assert!(manager.get_file_security().allow_all_session);

        manager.reset_session_permissions();

        assert!(!manager.get_file_security().allow_all_session);
        // With ask_for_permission=false the operation is still policy-allowed
        assert_eq!(
            manager.check_file_permission("Write", "/tmp/x"),
            FilePermissionResult::Allowed
        );
    }

    #[test]
    fn file_security_update_replaces_settings() {
        let mut manager = FileSecurityManager::new(FileSecurity {
            ask_for_permission: false,
            enabled: true,
            allow_all_session: false,
        });

        manager.update_file_security(FileSecurity {
            ask_for_permission: true,
            enabled: true,
            allow_all_session: false,
        });

        assert_eq!(
            manager.check_file_permission("Write", "/tmp/x"),
            FilePermissionResult::RequiresPermission
        );
    }
}
