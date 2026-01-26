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
    fn primary_name(self) -> &'static str {
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

    fn aliases(self) -> &'static [&'static str] {
        match self {
            HookEvent::UserPromptSubmit => &[
                // Claude Code official name
                "UserPromptSubmit",
                // Flexorama legacy names
                "pre_message",
                "before_message",
                "before_user_message",
                "user_message",
                "user-message",
                "prompt_before",
                "pre_prompt",
                "before_prompt",
            ],
            HookEvent::PreToolUse => &[
                // Claude Code official name
                "PreToolUse",
                // Flexorama legacy names
                "pre_tool",
                "before_tool",
                "tool_call",
                "tool-call",
                "before_tool_call",
                "tool_before",
            ],
            HookEvent::PostToolUse => &[
                // Claude Code official name
                "PostToolUse",
                // Flexorama legacy names
                "post_tool",
                "after_tool",
                "tool_result",
                "tool-result",
                "after_tool_call",
                "tool_after",
            ],
            HookEvent::Stop => &[
                // Claude Code official name
                "Stop",
                // Flexorama legacy names
                "post_message",
                "after_message",
                "after_assistant_message",
                "assistant_message",
                "assistant-message",
                "response",
                "after_response",
            ],
            HookEvent::SubagentStop => &[
                "SubagentStop",
                "subagent_stop",
                "after_subagent",
            ],
            HookEvent::SessionStart => &[
                "SessionStart",
                "session_start",
                "on_start",
                "init",
            ],
            HookEvent::PreCompact => &[
                "PreCompact",
                "pre_compact",
                "before_compact",
            ],
            HookEvent::Notification => &[
                "Notification",
                "notification",
                "on_notification",
            ],
            HookEvent::PermissionRequest => &[
                "PermissionRequest",
                "permission_request",
                "on_permission",
            ],
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

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum HookCommandDef {
    String(String),
    Detailed {
        command: String,
        #[serde(default)]
        args: Vec<String>,
        #[serde(default)]
        env: HashMap<String, String>,
        #[serde(default)]
        working_dir: Option<String>,
        #[serde(default)]
        timeout_ms: Option<u64>,
        #[serde(default)]
        shell: Option<String>,
        #[serde(default)]
        continue_on_error: bool,
    },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum HookCommandList {
    Single(HookCommandDef),
    Multiple(Vec<HookCommandDef>),
}

impl HookCommandList {
    fn into_vec(self) -> Vec<HookCommandDef> {
        match self {
            HookCommandList::Single(command) => vec![command],
            HookCommandList::Multiple(commands) => commands,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
struct HookConfigFile {
    #[serde(default)]
    hooks: HashMap<String, HookCommandList>,
}

/// Claude Code settings.json format
#[derive(Debug, Clone, Deserialize)]
struct ClaudeSettings {
    #[serde(flatten)]
    hooks: HashMap<String, Vec<ClaudeHookEntry>>,
}

#[derive(Debug, Clone, Deserialize)]
struct ClaudeHookEntry {
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
    shell: Option<String>,
    continue_on_error: bool,
    use_shell: bool,
    source: String,
}

#[derive(Debug, Clone)]
pub struct HookDecision {
    pub action: HookAction,
    pub message: Option<String>,
    pub updated_message: Option<String>,
    pub updated_arguments: Option<Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookAction {
    Continue,
    Abort,
}

#[derive(Debug, Deserialize)]
struct HookResponse {
    // Claude Code format
    #[serde(default)]
    decision: Option<String>, // "approve" or "block"
    #[serde(default)]
    reason: Option<String>,
    #[serde(default, rename = "continue")]
    continue_: Option<bool>,
    #[serde(default, rename = "stopReason")]
    stop_reason: Option<String>,
    #[serde(default, rename = "suppressOutput")]
    suppress_output: Option<bool>,

    // Flexorama legacy format
    #[serde(default)]
    action: Option<String>,
    #[serde(default)]
    abort: Option<bool>,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    user_message: Option<String>,
    #[serde(default)]
    tool_arguments: Option<Value>,
    #[serde(default)]
    arguments: Option<Value>,
}

impl HookResponse {
    /// Checks if this response indicates abortion/blocking
    fn is_abort(&self) -> bool {
        // Claude Code format: decision == "block"
        if self.decision.as_deref() == Some("block") {
            return true;
        }

        // Claude Code format: continue == false
        if self.continue_ == Some(false) {
            return true;
        }

        // Flexorama format: abort == true
        if self.abort == Some(true) {
            return true;
        }

        // Flexorama format: action == "abort"
        if self.action.as_deref().map(|a| a.eq_ignore_ascii_case("abort")).unwrap_or(false) {
            return true;
        }

        false
    }

    /// Gets the reason/message for abortion or modification
    fn get_message(&self) -> Option<String> {
        self.reason.clone()
            .or_else(|| self.stop_reason.clone())
            .or_else(|| self.message.clone())
            .or_else(|| self.user_message.clone())
    }

    /// Gets updated message content
    fn get_updated_message(&self) -> Option<String> {
        self.user_message.clone()
            .or_else(|| self.message.clone())
    }

    /// Gets updated tool arguments
    fn get_updated_arguments(&self) -> Option<Value> {
        self.tool_arguments.clone()
            .or_else(|| self.arguments.clone())
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
            // Claude Code format
            "session_id": session_id,
            "prompt": original_message,
            "timestamp": Utc::now().to_rfc3339(),
            // Flexorama extensions
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
            // Claude Code format
            "session_id": session_id,
            "timestamp": Utc::now().to_rfc3339(),
            // Flexorama extensions
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
            // Claude Code format
            "session_id": session_id,
            "tool_name": tool_name,
            "tool_input": arguments,
            "timestamp": Utc::now().to_rfc3339(),
            // Flexorama extensions
            "event": HookEvent::PreToolUse.primary_name(),
            "project_root": self.project_root.display().to_string(),
            "cwd": std::env::current_dir()?.display().to_string(),
            "model": model,
            "tool_use_id": tool_use_id,
        });
        self.run_event(HookEvent::PreToolUse, payload).await
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
            // Claude Code format
            "session_id": session_id,
            "tool_name": tool_name,
            "tool_input": arguments,
            "tool_output": result,
            "timestamp": Utc::now().to_rfc3339(),
            // Flexorama extensions
            "event": HookEvent::PostToolUse.primary_name(),
            "project_root": self.project_root.display().to_string(),
            "cwd": std::env::current_dir()?.display().to_string(),
            "model": model,
            "tool_use_id": tool_use_id,
            "is_error": is_error,
        });
        self.run_event(HookEvent::PostToolUse, payload).await
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
        let mut decision = HookDecision {
            action: HookAction::Continue,
            message: None,
            updated_message: None,
            updated_arguments: None,
        };

        // Collect unique commands (avoid executing duplicate hooks from aliases)
        let mut seen_commands = std::collections::HashSet::new();
        let mut commands = Vec::new();

        for alias in event.aliases() {
            if let Some(hooks) = self.hooks.get(*alias) {
                for hook in hooks {
                    // Create a unique key for this hook to prevent duplicates
                    let key = format!("{}:{:?}:{}", hook.command, hook.args, hook.source);
                    if seen_commands.insert(key) {
                        commands.push(hook.clone());
                    }
                }
            }
        }

        if commands.is_empty() {
            return Ok(decision);
        }

        // Overall timeout for all hooks (30 seconds)
        let overall_timeout = std::time::Duration::from_secs(30);
        let run_hooks_future = async {
            for command in commands {
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

                            // Update message if provided
                            if let Some(message) = response.get_updated_message() {
                                decision.updated_message = Some(message);
                            }

                            // Update arguments if provided
                            if let Some(arguments) = response.get_updated_arguments() {
                                decision.updated_arguments = Some(arguments);
                            }
                        }
                    }
                    Err(err) => {
                        if command.continue_on_error {
                            warn!(
                                "Hook '{}' failed but continue_on_error is set: {}",
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
            Err(_) => Err(anyhow!("Hook execution exceeded overall timeout of {:?}", overall_timeout)),
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
            let shell = command.shell.clone().unwrap_or_else(|| {
                Self::detect_shell()
            });
            let mut cmd = Command::new(&shell);

            // Determine shell arguments based on detected shell
            if shell.contains("pwsh") || shell.contains("powershell") {
                cmd.args(["-Command", &command.command]);
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
        cmd.env("FLEXORAMA_HOOK_EVENT", event_name);
        cmd.env(
            "CLAUDE_CODE_PROJECT_ROOT",
            self.project_root.display().to_string(),
        );
        cmd.env(
            "FLEXORAMA_PROJECT_ROOT",
            self.project_root.display().to_string(),
        );
        cmd.env("CLAUDE_CODE_HOOK_SOURCE", &command.source);
        cmd.env("FLEXORAMA_HOOK_SOURCE", &command.source);

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

        let mut loaded = false;

        // Try loading Claude Code style settings.json first
        let settings_json = flexorama_dir.join("settings.json");
        if settings_json.exists() && settings_json.is_file() {
            if let Ok(()) = self.load_claude_settings_file(&settings_json, source) {
                loaded = true;
            }
        }

        // Then try Flexorama hook config files
        let config_files = [
            flexorama_dir.join("hooks.json"),
            flexorama_dir.join("hooks.yaml"),
            flexorama_dir.join("hooks.yml"),
            flexorama_dir.join("hooks.toml"),
            flexorama_dir.join("hooks"),
        ];

        for config_path in config_files {
            if config_path.exists() && config_path.is_file() {
                self.load_hook_file(&config_path, source)?;
                loaded = true;
            }
        }

        // Load hook scripts from hooks/ directory
        let hooks_dir = flexorama_dir.join("hooks");
        if hooks_dir.exists() && hooks_dir.is_dir() {
            self.load_hook_directory(&hooks_dir, source)?;
            loaded = true;
        }

        Ok(loaded)
    }

    fn load_hook_directory(&mut self, hooks_dir: &Path, source: &str) -> Result<()> {
        for entry in fs::read_dir(hooks_dir)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            // Check if file is executable (Unix) or has valid extension (Windows)
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let metadata = fs::metadata(&path)?;
                let permissions = metadata.permissions();
                if permissions.mode() & 0o111 == 0 {
                    warn!(
                        "Skipping non-executable hook file: {} (use chmod +x to make it executable)",
                        path.display()
                    );
                    continue;
                }
            }

            #[cfg(windows)]
            {
                // On Windows, check for common executable extensions
                let is_executable = path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(|ext| {
                        matches!(
                            ext.to_lowercase().as_str(),
                            "exe" | "bat" | "cmd" | "ps1" | "py" | "rb" | "js" | "sh"
                        )
                    })
                    .unwrap_or(false);

                if !is_executable {
                    warn!(
                        "Skipping file with non-executable extension: {} (expected .exe, .bat, .cmd, .ps1, .py, .rb, .js, or .sh)",
                        path.display()
                    );
                    continue;
                }
            }

            let file_name = match path.file_stem().and_then(|s| s.to_str()) {
                Some(name) => name.to_string(),
                None => continue,
            };

            let command = HookCommand {
                command: path.display().to_string(),
                args: Vec::new(),
                env: HashMap::new(),
                working_dir: None,
                timeout_ms: None,
                shell: None,
                continue_on_error: false,
                use_shell: false,
                source: source.to_string(),
            };
            self.hooks.entry(file_name).or_default().push(command);
        }

        Ok(())
    }

    fn load_hook_file(&mut self, path: &Path, source: &str) -> Result<()> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read hook config {}", path.display()))?;
        let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");

        let hook_map = parse_hook_config(&content, extension)
            .with_context(|| format!("Failed to parse hook config {}", path.display()))?;

        for (event, list) in hook_map {
            for command in list.into_vec() {
                let hook_command = self.build_command(command, source)?;
                self.hooks
                    .entry(event.clone())
                    .or_default()
                    .push(hook_command);
            }
        }
        Ok(())
    }

    /// Load Claude Code style settings.json
    fn load_claude_settings_file(&mut self, path: &Path, source: &str) -> Result<()> {
        let content = fs::read_to_string(path)
            .with_context(|| format!("Failed to read Claude settings {}", path.display()))?;

        let settings: ClaudeSettings = serde_json::from_str(&content)
            .with_context(|| format!("Failed to parse Claude settings {}", path.display()))?;

        for (event_name, entries) in settings.hooks {
            for entry in entries {
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
                                shell: None,
                                continue_on_error,
                                use_shell,
                                source: source.to_string(),
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

    fn build_command(&self, command: HookCommandDef, source: &str) -> Result<HookCommand> {
        match command {
            HookCommandDef::String(command) => Ok(HookCommand {
                command,
                args: Vec::new(),
                env: HashMap::new(),
                working_dir: None,
                timeout_ms: None,
                shell: None,
                continue_on_error: false,
                use_shell: true,
                source: source.to_string(),
            }),
            HookCommandDef::Detailed {
                command,
                args,
                env,
                working_dir,
                timeout_ms,
                shell,
                continue_on_error,
            } => {
                let use_shell = args.is_empty();
                Ok(HookCommand {
                    command,
                    args,
                    env,
                    working_dir: working_dir.map(PathBuf::from),
                    timeout_ms,
                    shell,
                    continue_on_error,
                    use_shell,
                    source: source.to_string(),
                })
            }
        }
    }
}

fn parse_hook_config(content: &str, extension: &str) -> Result<HashMap<String, HookCommandList>> {
    // Try parsing with "hooks" wrapper key first
    let parsed: Result<HashMap<String, HookCommandList>> = match extension {
        "toml" => toml::from_str::<HookConfigFile>(content)
            .map(|c| c.hooks)
            .map_err(Into::into),
        "yaml" | "yml" => serde_yaml::from_str::<HookConfigFile>(content)
            .map(|c| c.hooks)
            .map_err(Into::into),
        "json" => serde_json::from_str::<HookConfigFile>(content)
            .map(|c| c.hooks)
            .map_err(Into::into),
        _ => {
            if let Ok(config) = toml::from_str::<HookConfigFile>(content) {
                Ok(config.hooks)
            } else if let Ok(config) = serde_yaml::from_str::<HookConfigFile>(content) {
                Ok(config.hooks)
            } else {
                serde_json::from_str::<HookConfigFile>(content)
                    .map(|c| c.hooks)
                    .map_err(Into::into)
            }
        }
    };

    // If we successfully parsed and got hooks, return them
    if let Ok(hooks) = parsed {
        if !hooks.is_empty() {
            return Ok(hooks);
        }
    }

    let fallback: Result<HashMap<String, HookCommandList>> = match extension {
        "toml" => toml::from_str::<HashMap<String, HookCommandList>>(content).map_err(Into::into),
        "yaml" | "yml" => {
            serde_yaml::from_str::<HashMap<String, HookCommandList>>(content).map_err(Into::into)
        }
        "json" => {
            serde_json::from_str::<HashMap<String, HookCommandList>>(content).map_err(Into::into)
        }
        _ => {
            if let Ok(config) = toml::from_str::<HashMap<String, HookCommandList>>(content) {
                Ok(config)
            } else if let Ok(config) =
                serde_yaml::from_str::<HashMap<String, HookCommandList>>(content)
            {
                Ok(config)
            } else {
                serde_json::from_str::<HashMap<String, HookCommandList>>(content)
                    .map_err(Into::into)
            }
        }
    };

    fallback.map_err(|err| anyhow!(err))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(not(windows))]
    #[tokio::test]
    async fn run_pre_message_updates_message() {
        let mut manager = HookManager {
            hooks: HashMap::new(),
            project_root: PathBuf::from("."),
        };
        manager.hooks.insert(
            "pre_message".to_string(),
            vec![HookCommand {
                command: "printf '{\"user_message\":\"updated\"}'".to_string(),
                args: Vec::new(),
                env: HashMap::new(),
                working_dir: None,
                timeout_ms: None,
                shell: None,
                continue_on_error: false,
                use_shell: true,
                source: "test".to_string(),
            }],
        );

        let decision = manager
            .run_pre_message("clean", "orig", &[], None, "model")
            .await
            .expect("run hook");

        assert_eq!(decision.action, HookAction::Continue);
        assert_eq!(decision.updated_message.as_deref(), Some("updated"));
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn run_pre_tool_aborts_on_action() {
        let mut manager = HookManager {
            hooks: HashMap::new(),
            project_root: PathBuf::from("."),
        };
        manager.hooks.insert(
            "pre_tool".to_string(),
            vec![HookCommand {
                command: "printf '{\"action\":\"abort\",\"message\":\"nope\"}'".to_string(),
                args: Vec::new(),
                env: HashMap::new(),
                working_dir: None,
                timeout_ms: None,
                shell: None,
                continue_on_error: false,
                use_shell: true,
                source: "test".to_string(),
            }],
        );

        let decision = manager
            .run_pre_tool(
                "tool-1",
                "write_file",
                &serde_json::json!({}),
                None,
                "model",
            )
            .await
            .expect("run hook");

        assert_eq!(decision.action, HookAction::Abort);
        assert_eq!(decision.message.as_deref(), Some("nope"));
    }

    #[test]
    fn parse_hook_config_with_hooks_key() {
        let content = r#"
        {
          "hooks": {
            "pre_tool": [
              "echo hi",
              {"command": "echo", "args": ["ok"], "continue_on_error": true}
            ]
          }
        }
        "#;
        let hooks = parse_hook_config(content, "json").expect("parse hooks");
        let commands = hooks.get("pre_tool").expect("pre_tool hook");
        assert_eq!(commands.clone().into_vec().len(), 2);
    }

    #[test]
    fn parse_hook_config_without_hooks_key() {
        let content = r#"
        pre_message:
          - echo hi
          - command: echo
            args: ["ok"]
        "#;
        let hooks = parse_hook_config(content, "yaml").expect("parse hooks");
        assert!(hooks.contains_key("pre_message"));
        let commands = hooks.get("pre_message").expect("pre_message hook");
        assert_eq!(commands.clone().into_vec().len(), 2);
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn claude_code_event_names_work() {
        let mut manager = HookManager {
            hooks: HashMap::new(),
            project_root: PathBuf::from("."),
        };
        // Use Claude Code event name
        manager.hooks.insert(
            "UserPromptSubmit".to_string(),
            vec![HookCommand {
                command: "printf '{\"user_message\":\"modified\"}'".to_string(),
                args: Vec::new(),
                env: HashMap::new(),
                working_dir: None,
                timeout_ms: None,
                shell: None,
                continue_on_error: false,
                use_shell: true,
                source: "test".to_string(),
            }],
        );

        let decision = manager
            .run_pre_message("clean", "orig", &[], None, "model")
            .await
            .expect("run hook");

        assert_eq!(decision.action, HookAction::Continue);
        assert_eq!(decision.updated_message.as_deref(), Some("modified"));
    }

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
                shell: None,
                continue_on_error: false,
                use_shell: true,
                source: "test".to_string(),
            }],
        );

        let decision = manager
            .run_pre_tool("tool-1", "bash", &serde_json::json!({}), None, "model")
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
                shell: None,
                continue_on_error: false,
                use_shell: true,
                source: "test".to_string(),
            }],
        );

        let decision = manager
            .run_post_message("response", None, "model")
            .await
            .expect("run hook");

        assert_eq!(decision.action, HookAction::Abort);
        assert_eq!(decision.message.as_deref(), Some("Not done yet"));
    }

    #[cfg(not(windows))]
    #[tokio::test]
    async fn hook_alias_deduplication() {
        let mut manager = HookManager {
            hooks: HashMap::new(),
            project_root: PathBuf::from("."),
        };

        let hook_cmd = HookCommand {
            command: "printf '{\"user_message\":\"test\"}'".to_string(),
            args: Vec::new(),
            env: HashMap::new(),
            working_dir: None,
            timeout_ms: None,
            shell: None,
            continue_on_error: false,
            use_shell: true,
            source: "test".to_string(),
        };

        // Add same hook under both primary name and alias
        manager.hooks.insert("UserPromptSubmit".to_string(), vec![hook_cmd.clone()]);
        manager.hooks.insert("pre_message".to_string(), vec![hook_cmd.clone()]);

        let decision = manager
            .run_pre_message("clean", "orig", &[], None, "model")
            .await
            .expect("run hook");

        // Should only execute once due to deduplication
        assert_eq!(decision.action, HookAction::Continue);
        assert_eq!(decision.updated_message.as_deref(), Some("test"));
    }

    #[test]
    fn parse_claude_settings_format() {
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

        let settings: ClaudeSettings = serde_json::from_str(content).expect("parse settings");
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
            "pre_message".to_string(),
            vec![HookCommand {
                command: "sleep 2".to_string(),
                args: Vec::new(),
                env: HashMap::new(),
                working_dir: None,
                timeout_ms: Some(100), // 100ms timeout
                shell: None,
                continue_on_error: false,
                use_shell: true,
                source: "test".to_string(),
            }],
        );

        let result = manager.run_pre_message("clean", "orig", &[], None, "model").await;
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
            "pre_message".to_string(),
            vec![
                HookCommand {
                    command: "exit 1".to_string(),
                    args: Vec::new(),
                    env: HashMap::new(),
                    working_dir: None,
                    timeout_ms: None,
                    shell: None,
                    continue_on_error: true, // Allow failure
                    use_shell: true,
                    source: "test".to_string(),
                },
                HookCommand {
                    command: "printf '{\"user_message\":\"still ran\"}'".to_string(),
                    args: Vec::new(),
                    env: HashMap::new(),
                    working_dir: None,
                    timeout_ms: None,
                    shell: None,
                    continue_on_error: false,
                    use_shell: true,
                    source: "test".to_string(),
                },
            ],
        );

        let decision = manager
            .run_pre_message("clean", "orig", &[], None, "model")
            .await
            .expect("should succeed despite first hook failing");

        assert_eq!(decision.updated_message.as_deref(), Some("still ran"));
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
