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

#[derive(Debug, Clone, Copy)]
pub enum HookEvent {
    PreMessage,
    PostMessage,
    PreTool,
    PostTool,
}

impl HookEvent {
    fn primary_name(self) -> &'static str {
        match self {
            HookEvent::PreMessage => "pre_message",
            HookEvent::PostMessage => "post_message",
            HookEvent::PreTool => "pre_tool",
            HookEvent::PostTool => "post_tool",
        }
    }

    fn aliases(self) -> &'static [&'static str] {
        match self {
            HookEvent::PreMessage => &[
                "pre_message",
                "before_message",
                "before_user_message",
                "user_message",
                "user-message",
                "prompt_before",
                "pre_prompt",
                "before_prompt",
            ],
            HookEvent::PostMessage => &[
                "post_message",
                "after_message",
                "after_assistant_message",
                "assistant_message",
                "assistant-message",
                "response",
                "after_response",
            ],
            HookEvent::PreTool => &[
                "pre_tool",
                "before_tool",
                "tool_call",
                "tool-call",
                "before_tool_call",
                "tool_before",
            ],
            HookEvent::PostTool => &[
                "post_tool",
                "after_tool",
                "tool_result",
                "tool-result",
                "after_tool_call",
                "tool_after",
            ],
        }
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
        let payload = serde_json::json!({
            "event": HookEvent::PreMessage.primary_name(),
            "timestamp": Utc::now().to_rfc3339(),
            "project_root": self.project_root.display().to_string(),
            "cwd": std::env::current_dir()?.display().to_string(),
            "conversation_id": conversation_id,
            "model": model,
            "message": original_message,
            "cleaned_message": cleaned_message,
            "context_files": context_files,
        });
        self.run_event(HookEvent::PreMessage, payload).await
    }

    pub async fn run_post_message(
        &self,
        final_response: &str,
        conversation_id: Option<&str>,
        model: &str,
    ) -> Result<HookDecision> {
        let payload = serde_json::json!({
            "event": HookEvent::PostMessage.primary_name(),
            "timestamp": Utc::now().to_rfc3339(),
            "project_root": self.project_root.display().to_string(),
            "cwd": std::env::current_dir()?.display().to_string(),
            "conversation_id": conversation_id,
            "model": model,
            "response": final_response,
        });
        self.run_event(HookEvent::PostMessage, payload).await
    }

    pub async fn run_pre_tool(
        &self,
        tool_use_id: &str,
        tool_name: &str,
        arguments: &Value,
        conversation_id: Option<&str>,
        model: &str,
    ) -> Result<HookDecision> {
        let payload = serde_json::json!({
            "event": HookEvent::PreTool.primary_name(),
            "timestamp": Utc::now().to_rfc3339(),
            "project_root": self.project_root.display().to_string(),
            "cwd": std::env::current_dir()?.display().to_string(),
            "conversation_id": conversation_id,
            "model": model,
            "tool": {
                "id": tool_use_id,
                "name": tool_name,
                "arguments": arguments,
            }
        });
        self.run_event(HookEvent::PreTool, payload).await
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
        let payload = serde_json::json!({
            "event": HookEvent::PostTool.primary_name(),
            "timestamp": Utc::now().to_rfc3339(),
            "project_root": self.project_root.display().to_string(),
            "cwd": std::env::current_dir()?.display().to_string(),
            "conversation_id": conversation_id,
            "model": model,
            "tool": {
                "id": tool_use_id,
                "name": tool_name,
                "arguments": arguments,
                "result": result,
                "is_error": is_error,
            }
        });
        self.run_event(HookEvent::PostTool, payload).await
    }

    async fn run_event(&self, event: HookEvent, payload: Value) -> Result<HookDecision> {
        let mut decision = HookDecision {
            action: HookAction::Continue,
            message: None,
            updated_message: None,
            updated_arguments: None,
        };

        let mut commands = Vec::new();
        for alias in event.aliases() {
            if let Some(hooks) = self.hooks.get(*alias) {
                commands.extend(hooks.iter().cloned());
            }
        }

        if commands.is_empty() {
            return Ok(decision);
        }

        for command in commands {
            let output = self.execute_hook_command(&command, event, &payload).await;
            match output {
                Ok(response) => {
                    if let Some(response) = response {
                        if response.abort.unwrap_or(false)
                            || response
                                .action
                                .as_deref()
                                .map(|value| value.eq_ignore_ascii_case("abort"))
                                .unwrap_or(false)
                        {
                            decision.action = HookAction::Abort;
                            decision.message = response.message.or(response.user_message);
                            break;
                        }
                        if let Some(message) = response.user_message.or(response.message.clone()) {
                            decision.updated_message = Some(message);
                        }
                        if let Some(arguments) = response.tool_arguments.or(response.arguments) {
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
    }

    async fn execute_hook_command(
        &self,
        command: &HookCommand,
        event: HookEvent,
        payload: &Value,
    ) -> Result<Option<HookResponse>> {
        let mut cmd = if command.use_shell {
            let shell = command.shell.clone().unwrap_or_else(|| {
                if cfg!(target_os = "windows") {
                    "powershell".to_string()
                } else {
                    "bash".to_string()
                }
            });
            let mut cmd = Command::new(shell);
            if cfg!(target_os = "windows") {
                cmd.args(["-Command", &command.command]);
            } else {
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
        if let Some(mut stdin) = child.stdin.take() {
            stdin.write_all(payload_string.as_bytes()).await?;
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

    if let Ok(hooks) = parsed {
        if !hooks.is_empty() || content.contains("hooks") {
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
}
