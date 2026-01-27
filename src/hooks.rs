use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use log::{debug, warn};
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookEvent {
    // User interaction events
    UserPromptSubmit,
    // Tool events
    PreToolUse,
    PostToolUse,
    // Agent lifecycle events
    Stop,
    SubagentStop,
    SessionStart,
    // System events
    PreCompact,
    Notification,
    PermissionRequest,
}

impl HookEvent {
    pub fn primary_name(self) -> &'static str {
        match self {
            HookEvent::UserPromptSubmit => "UserPromptSubmit",
            HookEvent::PreToolUse => "PreToolUse",
            HookEvent::PostToolUse => "PostToolUse",
            HookEvent::Stop => "Stop",
            HookEvent::SubagentStop => "SubagentStop",
            HookEvent::SessionStart => "SessionStart",
            HookEvent::PreCompact => "PreCompact",
            HookEvent::Notification => "Notification",
            HookEvent::PermissionRequest => "PermissionRequest",
        }
    }

    /// Returns all possible hook events
    pub fn all_events() -> &'static [HookEvent] {
        &[
            HookEvent::UserPromptSubmit,
            HookEvent::PreToolUse,
            HookEvent::PostToolUse,
            HookEvent::Stop,
            HookEvent::SubagentStop,
            HookEvent::SessionStart,
            HookEvent::PreCompact,
            HookEvent::Notification,
            HookEvent::PermissionRequest,
        ]
    }
}

#[derive(Debug, Clone)]
pub struct HookManager {
    hooks: HashMap<String, Vec<HookCommand>>,
    project_root: PathBuf,
}

/// Claude Code settings.json format - with "hooks" wrapper
#[derive(Debug, Clone, Deserialize)]
struct ClaudeSettingsWrapper {
    hooks: HashMap<String, Vec<ClaudeHookEntry>>,
}

/// Claude Code settings.json format - without wrapper (legacy)
#[derive(Debug, Clone, Deserialize)]
struct ClaudeSettingsFlat {
    #[serde(flatten)]
    hooks: HashMap<String, Vec<ClaudeHookEntry>>,
}

#[derive(Debug, Clone, Deserialize)]
struct ClaudeHookEntry {
    /// Optional matcher to filter by tool name (e.g., "Bash", "Read")
    #[serde(default)]
    matcher: Option<String>,
    #[serde(default)]
    hooks: Vec<ClaudeHookDef>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ClaudeHookDef {
    Command {
        #[serde(rename = "type")]
        hook_type: String,
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
        #[serde(default, rename = "workingDirectory")]
        working_directory: Option<String>,
        #[serde(default, rename = "timeoutMs")]
        timeout_ms: Option<u64>,
        #[serde(default, rename = "continueOnError")]
        continue_on_error: bool,
    },
}

#[derive(Debug, Clone)]
struct HookCommand {
    command: String,
    args: Vec<String>,
    env: HashMap<String, String>,
    working_dir: Option<PathBuf>,
    timeout_ms: Option<u64>,
    continue_on_error: bool,
    use_shell: bool,
    source: String,
    /// Optional matcher to filter by tool name
    matcher: Option<String>,
}

#[derive(Debug, Clone)]
pub struct HookDecision {
    pub action: HookAction,
    pub message: Option<String>,
    pub updated_message: Option<String>,
    pub updated_arguments: Option<Value>,
}

/// Information about a configured hook for display
#[derive(Debug, Clone)]
pub struct HookInfo {
    pub event: String,
    pub command: String,
    pub args: Vec<String>,
    pub source: String,
    pub continue_on_error: bool,
    pub timeout_ms: Option<u64>,
    pub matcher: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookAction {
    Continue,
    Abort,
}

#[derive(Debug, Deserialize)]
struct HookResponse {
    #[serde(default)]
    decision: Option<String>, // "approve" or "block"
    #[serde(default)]
    reason: Option<String>,
    #[serde(default, rename = "continue")]
    continue_: Option<bool>,
    #[serde(default, rename = "stopReason")]
    stop_reason: Option<String>,
}

impl HookResponse {
    /// Checks if this response indicates abortion/blocking
    fn is_abort(&self) -> bool {
        // decision == "block"
        if self.decision.as_deref() == Some("block") {
            return true;
        }

        // continue == false
        if self.continue_ == Some(false) {
            return true;
        }

        false
    }

    /// Gets the reason/message for abortion
    fn get_message(&self) -> Option<String> {
        self.reason.clone().or_else(|| self.stop_reason.clone())
    }
}

impl HookManager {
    pub fn load() -> Result<Option<Self>> {
        let project_root = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let mut manager = HookManager {
            hooks: HashMap::new(),
            project_root: project_root.clone(),
        };

        let mut loaded = false;

        if let Some(home_dir) = dirs::home_dir() {
            let home_flexorama = home_dir.join(".flexorama");
            if manager.load_from_flexorama_dir(&home_flexorama, "home")? {
                loaded = true;
            }
        }

        let project_flexorama = project_root.join(".flexorama");
        if manager.load_from_flexorama_dir(&project_flexorama, "project")? {
            loaded = true;
        }

        if loaded {
            Ok(Some(manager))
        } else {
            Ok(None)
        }
    }

    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Returns an iterator over all configured hooks with their event names
    pub fn list_hooks(&self) -> Vec<HookInfo> {
        let mut result = Vec::new();
        for (event_name, commands) in &self.hooks {
            for cmd in commands {
                result.push(HookInfo {
                    event: event_name.clone(),
                    command: cmd.command.clone(),
                    args: cmd.args.clone(),
                    source: cmd.source.clone(),
                    continue_on_error: cmd.continue_on_error,
                    timeout_ms: cmd.timeout_ms,
                    matcher: cmd.matcher.clone(),
                });
            }
        }
        result.sort_by(|a, b| a.event.cmp(&b.event));
        result
    }

    /// Returns the paths where hooks can be configured
    pub fn get_config_paths() -> Vec<(String, std::path::PathBuf, bool)> {
        let mut paths = Vec::new();

        // Global hooks
        if let Some(home_dir) = dirs::home_dir() {
            let home_flexorama = home_dir.join(".flexorama");
            let settings_json = home_flexorama.join("settings.json");
            paths.push((
                "Global settings.json".to_string(),
                settings_json.clone(),
                settings_json.exists(),
            ));
        }

        // Project hooks
        if let Ok(project_root) = std::env::current_dir() {
            let project_flexorama = project_root.join(".flexorama");
            let settings_json = project_flexorama.join("settings.json");
            paths.push((
                "Project settings.json".to_string(),
                settings_json.clone(),
                settings_json.exists(),
            ));
        }

        paths
    }

    pub async fn run_pre_message(
        &self,
        cleaned_message: &str,
        original_message: &str,
        context_files: &[String],
        conversation_id: Option<&str>,
        model: &str,
    ) -> Result<HookDecision> {
        let session_id = conversation_id.unwrap_or("unknown");
        let payload = serde_json::json!({
            "session_id": session_id,
            "prompt": original_message,
            "timestamp": Utc::now().to_rfc3339(),
            "event": HookEvent::UserPromptSubmit.primary_name(),
            "project_root": self.project_root.display().to_string(),
            "cwd": std::env::current_dir()?.display().to_string(),
            "model": model,
            "cleaned_message": cleaned_message,
            "context_files": context_files,
        });
        self.run_event(HookEvent::UserPromptSubmit, payload).await
    }

    pub async fn run_post_message(
        &self,
        final_response: &str,
        conversation_id: Option<&str>,
        model: &str,
    ) -> Result<HookDecision> {
        let session_id = conversation_id.unwrap_or("unknown");
        let payload = serde_json::json!({
            "session_id": session_id,
            "timestamp": Utc::now().to_rfc3339(),
            "event": HookEvent::Stop.primary_name(),
            "project_root": self.project_root.display().to_string(),
            "cwd": std::env::current_dir()?.display().to_string(),
            "model": model,
            "response": final_response,
        });
        self.run_event(HookEvent::Stop, payload).await
    }

    pub async fn run_pre_tool(
        &self,
        tool_use_id: &str,
        tool_name: &str,
        arguments: &Value,
        conversation_id: Option<&str>,
        model: &str,
    ) -> Result<HookDecision> {
        let session_id = conversation_id.unwrap_or("unknown");
        let payload = serde_json::json!({
            "session_id": session_id,
            "tool_name": tool_name,
            "tool_input": arguments,
            "tool_use_id": tool_use_id,
            "hook_event_name": HookEvent::PreToolUse.primary_name(),
            "cwd": std::env::current_dir()?.display().to_string(),
            "project_root": self.project_root.display().to_string(),
            "model": model,
            "timestamp": Utc::now().to_rfc3339(),
        });
        self.run_event_with_matcher(HookEvent::PreToolUse, payload, Some(tool_name))
            .await
    }

    pub async fn run_post_tool(
        &self,
        tool_use_id: &str,
        tool_name: &str,
        arguments: &Value,
        result: &str,
        is_error: bool,
        conversation_id: Option<&str>,
        model: &str,
    ) -> Result<HookDecision> {
        let session_id = conversation_id.unwrap_or("unknown");
        let payload = serde_json::json!({
            "session_id": session_id,
            "tool_name": tool_name,
            "tool_input": arguments,
            "tool_result": result,
            "tool_use_id": tool_use_id,
            "is_error": is_error,
            "hook_event_name": HookEvent::PostToolUse.primary_name(),
            "cwd": std::env::current_dir()?.display().to_string(),
            "project_root": self.project_root.display().to_string(),
            "model": model,
            "timestamp": Utc::now().to_rfc3339(),
        });
        self.run_event_with_matcher(HookEvent::PostToolUse, payload, Some(tool_name))
            .await
    }

    /// Run session start hook
    pub async fn run_session_start(
        &self,
        conversation_id: Option<&str>,
        model: &str,
    ) -> Result<HookDecision> {
        let session_id = conversation_id.unwrap_or("unknown");
        let payload = serde_json::json!({
            "session_id": session_id,
            "timestamp": Utc::now().to_rfc3339(),
            "event": HookEvent::SessionStart.primary_name(),
            "project_root": self.project_root.display().to_string(),
            "cwd": std::env::current_dir()?.display().to_string(),
            "model": model,
        });
        self.run_event(HookEvent::SessionStart, payload).await
    }

    /// Run subagent stop hook
    pub async fn run_subagent_stop(
        &self,
        conversation_id: Option<&str>,
        model: &str,
    ) -> Result<HookDecision> {
        let session_id = conversation_id.unwrap_or("unknown");
        let payload = serde_json::json!({
            "session_id": session_id,
            "timestamp": Utc::now().to_rfc3339(),
            "event": HookEvent::SubagentStop.primary_name(),
            "project_root": self.project_root.display().to_string(),
            "cwd": std::env::current_dir()?.display().to_string(),
            "model": model,
        });
        self.run_event(HookEvent::SubagentStop, payload).await
    }

    async fn run_event(&self, event: HookEvent, payload: Value) -> Result<HookDecision> {
        self.run_event_with_matcher(event, payload, None).await
    }

    async fn run_event_with_matcher(
        &self,
        event: HookEvent,
        payload: Value,
        tool_name: Option<&str>,
    ) -> Result<HookDecision> {
        let mut decision = HookDecision {
            action: HookAction::Continue,
            message: None,
            updated_message: None,
            updated_arguments: None,
        };

        let event_name = event.primary_name();
        let commands = match self.hooks.get(event_name) {
            Some(cmds) => cmds.clone(),
            None => return Ok(decision),
        };

        if commands.is_empty() {
            return Ok(decision);
        }

        // Filter commands by matcher if tool_name is provided
        let filtered_commands: Vec<_> = commands
            .into_iter()
            .filter(|cmd| {
                match (&cmd.matcher, tool_name) {
                    // No matcher means run for all tools
                    (None, _) => true,
                    // Matcher specified but no tool name - skip
                    (Some(_), None) => false,
                    // Both specified - check if they match (case-insensitive)
                    (Some(matcher), Some(name)) => matcher.eq_ignore_ascii_case(name),
                }
            })
            .collect();

        if filtered_commands.is_empty() {
            return Ok(decision);
        }

        // Overall timeout for all hooks (30 seconds)
        let overall_timeout = std::time::Duration::from_secs(30);
        let run_hooks_future = async {
            for command in filtered_commands {
                let output = self.execute_hook_command(&command, event, &payload).await;
                match output {
                    Ok(response) => {
                        if let Some(response) = response {
                            // Check if hook wants to abort/block
                            if response.is_abort() {
                                decision.action = HookAction::Abort;
                                decision.message = response.get_message();
                                break;
                            }
                        }
                    }
                    Err(err) => {
                        if command.continue_on_error {
                            warn!(
                                "Hook '{}' failed but continueOnError is set: {}",
                                command.command, err
                            );
                        } else {
                            return Err(err);
                        }
                    }
                }
            }
            Ok(decision)
        };

        // Apply overall timeout
        match tokio::time::timeout(overall_timeout, run_hooks_future).await {
            Ok(result) => result,
            Err(_) => Err(anyhow!(
                "Hook execution exceeded overall timeout of {:?}",
                overall_timeout
            )),
        }
    }

    /// Detect the best available shell for the current platform
    fn detect_shell() -> String {
        if cfg!(target_os = "windows") {
            // Try PowerShell Core first (cross-platform)
            if which::which("pwsh").is_ok() {
                return "pwsh".to_string();
            }
            // Fall back to Windows PowerShell
            if which::which("powershell").is_ok() {
                return "powershell".to_string();
            }
            // Last resort: cmd.exe
            "cmd".to_string()
        } else {
            // For Unix-like systems, prefer bash
            if which::which("bash").is_ok() {
                return "bash".to_string();
            }
            // Fall back to sh
            "sh".to_string()
        }
    }

    async fn execute_hook_command(
        &self,
        command: &HookCommand,
        event: HookEvent,
        payload: &Value,
    ) -> Result<Option<HookResponse>> {
        let mut cmd = if command.use_shell {
            let shell = Self::detect_shell();
            let mut cmd = Command::new(&shell);

            // Determine shell arguments based on detected shell
            if shell.contains("pwsh") || shell.contains("powershell") {
                // -NoProfile: Don't load profile scripts (faster startup)
                // -NonInteractive: Don't prompt for input (prevents "supply values for parameters" prompts)
                // -Command: Execute the command
                cmd.args(["-NoProfile", "-NonInteractive", "-Command", &command.command]);
            } else if shell.contains("cmd") {
                cmd.args(["/C", &command.command]);
            } else {
                // Assume Unix-like shell (bash, sh, zsh, etc.)
                cmd.args(["-c", &command.command]);
            }
            cmd
        } else {
            let mut cmd = Command::new(&command.command);
            cmd.args(&command.args);
            cmd
        };

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if let Some(working_dir) = &command.working_dir {
            cmd.current_dir(working_dir);
        }

        for (key, value) in &command.env {
            cmd.env(key, value);
        }

        let event_name = event.primary_name();
        cmd.env("CLAUDE_CODE_HOOK_EVENT", event_name);
        cmd.env(
            "CLAUDE_CODE_PROJECT_ROOT",
            self.project_root.display().to_string(),
        );
        cmd.env("CLAUDE_CODE_HOOK_SOURCE", &command.source);

        let payload_string = serde_json::to_string(payload)?;
        let mut child = cmd.spawn().context("Failed to spawn hook command")?;

        // Write to stdin and close it. Ignore broken pipe errors (child may exit quickly).
        if let Some(mut stdin) = child.stdin.take() {
            let write_result = stdin.write_all(payload_string.as_bytes()).await;
            // Explicitly drop stdin to close the pipe
            drop(stdin);

            // Ignore broken pipe errors - if the child process exits before reading all input,
            // that's okay (e.g., simple hooks that don't read stdin)
            if let Err(e) = write_result {
                if e.kind() != std::io::ErrorKind::BrokenPipe {
                    return Err(e.into());
                }
            }
        }

        let output = if let Some(timeout) = command.timeout_ms {
            tokio::time::timeout(
                std::time::Duration::from_millis(timeout),
                child.wait_with_output(),
            )
            .await
            .context("Hook command timed out")??
        } else {
            child.wait_with_output().await?
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(anyhow!(
                "Hook command '{}' failed with status {}: {}",
                command.command,
                output.status,
                stderr
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if stdout.is_empty() {
            return Ok(None);
        }

        match serde_json::from_str::<HookResponse>(&stdout) {
            Ok(response) => Ok(Some(response)),
            Err(err) => {
                debug!(
                    "Hook '{}' returned non-JSON output: {} ({})",
                    command.command, stdout, err
                );
                Ok(None)
            }
        }
    }

    fn load_from_flexorama_dir(&mut self, flexorama_dir: &Path, source: &str) -> Result<bool> {
        if !flexorama_dir.exists() {
            return Ok(false);
        }

        // Load Claude Code style settings.json
        let settings_json = flexorama_dir.join("settings.json");
        if settings_json.exists() && settings_json.is_file() {
            self.load_settings_file(&settings_json, source)?;
            return Ok(true);
        }

        Ok(false)
    }

    /// Load Claude Code style settings.json
    fn load_settings_file(&mut self, path: &Path, source: &str) -> Result<()> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read settings {}", path.display()))?;

        // Try parsing with "hooks" wrapper first (standard Claude Code format)
        let hooks_map = if let Ok(wrapper) = serde_json::from_str::<ClaudeSettingsWrapper>(&content)
        {
            wrapper.hooks
        } else if let Ok(flat) = serde_json::from_str::<ClaudeSettingsFlat>(&content) {
            // Fall back to flat format (events at root level)
            flat.hooks
        } else {
            return Err(anyhow!(
                "Failed to parse settings {}: invalid format",
                path.display()
            ));
        };

        for (event_name, entries) in hooks_map {
            for entry in entries {
                let matcher = entry.matcher.clone();
                for hook_def in entry.hooks {
                    match hook_def {
                        ClaudeHookDef::Command {
                            hook_type,
                            command,
                            args,
                            env,
                            working_directory,
                            timeout_ms,
                            continue_on_error,
                        } => {
                            // Only support "command" type hooks
                            if hook_type != "command" {
                                warn!("Unsupported hook type '{}', skipping", hook_type);
                                continue;
                            }

                            let use_shell = args.is_empty();
                            let hook_command = HookCommand {
                                command,
                                args,
                                env,
                                working_dir: working_directory.map(PathBuf::from),
                                timeout_ms,
                                continue_on_error,
                                use_shell,
                                source: source.to_string(),
                                matcher: matcher.clone(),
                            };

                            self.hooks
                                .entry(event_name.clone())
                                .or_default()
                                .push(hook_command);
                        }
                    }
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(windows))]
    #[tokio::test]
    async fn claude_code_response_format_blocks() {
        let mut manager = HookManager {
            hooks: HashMap::new(),
            project_root: PathBuf::from("."),
        };
        manager.hooks.insert(
            "PreToolUse".to_string(),
            vec![HookCommand {
                command: "printf '{\"decision\":\"block\",\"reason\":\"Not allowed\"}'".to_string(),
                args: Vec::new(),
                env: HashMap::new(),
                working_dir: None,
                timeout_ms: None,
                continue_on_error: false,
                use_shell: true,
                source: "test".to_string(),
                matcher: None,
            }],
        );

        let decision = manager
            .run_pre_tool("tool-1", "Bash", &serde_json::json!({}), None, "model")
            .await
            .expect("run hook");

        assert_eq!(decision.action, HookAction::Abort);
        assert_eq!(decision.message.as_deref(), Some("Not allowed"));
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn claude_code_continue_false_blocks() {
        let mut manager = HookManager {
            hooks: HashMap::new(),
            project_root: PathBuf::from("."),
        };
        manager.hooks.insert(
            "Stop".to_string(),
            vec![HookCommand {
                command: "printf '{\"continue\":false,\"stopReason\":\"Not done yet\"}'".to_string(),
                args: Vec::new(),
                env: HashMap::new(),
                working_dir: None,
                timeout_ms: None,
                continue_on_error: false,
                use_shell: true,
                source: "test".to_string(),
                matcher: None,
            }],
        );

        let decision = manager
            .run_post_message("response", None, "model")
            .await
            .expect("run hook");

        assert_eq!(decision.action, HookAction::Abort);
        assert_eq!(decision.message.as_deref(), Some("Not done yet"));
    }

    #[test]
    fn parse_claude_settings_with_hooks_wrapper() {
        let content = r#"
        {
          "hooks": {
            "PreToolUse": [
              {
                "matcher": "Bash",
                "hooks": [
                  {
                    "type": "command",
                    "command": "echo test"
                  }
                ]
              }
            ]
          }
        }
        "#;

        let settings: ClaudeSettingsWrapper =
            serde_json::from_str(content).expect("parse settings with wrapper");
        assert!(settings.hooks.contains_key("PreToolUse"));
        let entries = settings.hooks.get("PreToolUse").unwrap();
        assert_eq!(entries[0].matcher, Some("Bash".to_string()));
    }

    #[test]
    fn parse_claude_settings_flat_format() {
        let content = r#"
        {
          "UserPromptSubmit": [
            {
              "hooks": [
                {
                  "type": "command",
                  "command": "python script.py",
                  "continueOnError": true
                }
              ]
            }
          ],
          "PreToolUse": [
            {
              "hooks": [
                {
                  "type": "command",
                  "command": "check-tool",
                  "args": ["--strict"],
                  "timeoutMs": 5000
                }
              ]
            }
          ]
        }
        "#;

        let settings: ClaudeSettingsFlat =
            serde_json::from_str(content).expect("parse flat settings");
        assert!(settings.hooks.contains_key("UserPromptSubmit"));
        assert!(settings.hooks.contains_key("PreToolUse"));
    }

    #[test]
    fn detect_shell_returns_valid_shell() {
        let shell = HookManager::detect_shell();
        // Should return a non-empty shell name
        assert!(!shell.is_empty());

        if cfg!(target_os = "windows") {
            assert!(
                shell.contains("pwsh") || shell.contains("powershell") || shell.contains("cmd"),
                "Expected Windows shell, got: {}",
                shell
            );
        } else {
            assert!(
                shell.contains("bash") || shell.contains("sh"),
                "Expected Unix shell, got: {}",
                shell
            );
        }
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn hook_timeout_is_enforced() {
        let mut manager = HookManager {
            hooks: HashMap::new(),
            project_root: PathBuf::from("."),
        };
        manager.hooks.insert(
            "UserPromptSubmit".to_string(),
            vec![HookCommand {
                command: "sleep 2".to_string(),
                args: Vec::new(),
                env: HashMap::new(),
                working_dir: None,
                timeout_ms: Some(100), // 100ms timeout
                continue_on_error: false,
                use_shell: true,
                source: "test".to_string(),
                matcher: None,
            }],
        );

        let result = manager
            .run_pre_message("clean", "orig", &[], None, "model")
            .await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("timed out"));
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn continue_on_error_allows_hook_to_fail() {
        let mut manager = HookManager {
            hooks: HashMap::new(),
            project_root: PathBuf::from("."),
        };
        manager.hooks.insert(
            "UserPromptSubmit".to_string(),
            vec![
                HookCommand {
                    command: "exit 1".to_string(),
                    args: Vec::new(),
                    env: HashMap::new(),
                    working_dir: None,
                    timeout_ms: None,
                    continue_on_error: true, // Allow failure
                    use_shell: true,
                    source: "test".to_string(),
                    matcher: None,
                },
                HookCommand {
                    command: "printf '{\"decision\":\"approve\"}'".to_string(),
                    args: Vec::new(),
                    env: HashMap::new(),
                    working_dir: None,
                    timeout_ms: None,
                    continue_on_error: false,
                    use_shell: true,
                    source: "test".to_string(),
                    matcher: None,
                },
            ],
        );

        let decision = manager
            .run_pre_message("clean", "orig", &[], None, "model")
            .await
            .expect("should succeed despite first hook failing");

        assert_eq!(decision.action, HookAction::Continue);
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn matcher_filters_hooks_by_tool_name() {
        let mut manager = HookManager {
            hooks: HashMap::new(),
            project_root: PathBuf::from("."),
        };
        manager.hooks.insert(
            "PreToolUse".to_string(),
            vec![
                HookCommand {
                    command: "printf '{\"decision\":\"block\",\"reason\":\"Bash blocked\"}'".to_string(),
                    args: Vec::new(),
                    env: HashMap::new(),
                    working_dir: None,
                    timeout_ms: None,
                    continue_on_error: false,
                    use_shell: true,
                    source: "test".to_string(),
                    matcher: Some("Bash".to_string()),
                },
                HookCommand {
                    command: "printf '{\"decision\":\"approve\"}'".to_string(),
                    args: Vec::new(),
                    env: HashMap::new(),
                    working_dir: None,
                    timeout_ms: None,
                    continue_on_error: false,
                    use_shell: true,
                    source: "test".to_string(),
                    matcher: Some("Read".to_string()),
                },
            ],
        );

        // Bash tool should be blocked
        let decision = manager
            .run_pre_tool("tool-1", "Bash", &serde_json::json!({}), None, "model")
            .await
            .expect("run hook");
        assert_eq!(decision.action, HookAction::Abort);
        assert_eq!(decision.message.as_deref(), Some("Bash blocked"));

        // Read tool should pass (different matcher)
        let decision = manager
            .run_pre_tool("tool-2", "Read", &serde_json::json!({}), None, "model")
            .await
            .expect("run hook");
        assert_eq!(decision.action, HookAction::Continue);

        // Write tool should have no hooks (no matcher matches)
        let decision = manager
            .run_pre_tool("tool-3", "Write", &serde_json::json!({}), None, "model")
            .await
            .expect("run hook");
        assert_eq!(decision.action, HookAction::Continue);
    }

    #[test]
    fn hook_event_all_events_contains_all_variants() {
        let all_events = HookEvent::all_events();
        assert_eq!(all_events.len(), 9);
        assert!(all_events.contains(&HookEvent::UserPromptSubmit));
        assert!(all_events.contains(&HookEvent::PreToolUse));
        assert!(all_events.contains(&HookEvent::PostToolUse));
        assert!(all_events.contains(&HookEvent::Stop));
        assert!(all_events.contains(&HookEvent::SubagentStop));
        assert!(all_events.contains(&HookEvent::SessionStart));
        assert!(all_events.contains(&HookEvent::PreCompact));
        assert!(all_events.contains(&HookEvent::Notification));
        assert!(all_events.contains(&HookEvent::PermissionRequest));
    }
}
