use anyhow::{anyhow, Result};
use regex::Regex;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use tokio::fs;

#[derive(Debug, Clone)]
pub struct CustomCommand {
    pub name: String,
    pub description: Option<String>,
    pub argument_hint: Option<String>,
    pub allowed_tools: Vec<String>,
    pub model: Option<String>,
    pub content: String,
}

#[derive(Debug)]
pub struct RenderedCustomCommand {
    pub command: CustomCommand,
    pub message: String,
}

#[derive(Debug, Deserialize, Default)]
struct CustomCommandFrontmatter {
    #[serde(rename = "allowed-tools", default, alias = "allowed_tools")]
    allowed_tools: Option<AllowedToolsDef>,
    #[serde(rename = "argument-hint", default, alias = "argument_hint")]
    argument_hint: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum AllowedToolsDef {
    List(Vec<String>),
    String(String),
}

impl CustomCommand {
    pub fn from_markdown(name: String, content: &str) -> Result<Self> {
        let (frontmatter, body) = split_frontmatter(content)?;
        let mut command = CustomCommand {
            name,
            description: None,
            argument_hint: None,
            allowed_tools: Vec::new(),
            model: None,
            content: body.trim().to_string(),
        };

        if let Some(frontmatter) = frontmatter {
            if !frontmatter.trim().is_empty() {
                match serde_yaml::from_str::<CustomCommandFrontmatter>(&frontmatter) {
                    Ok(parsed) => {
                        command.description = parsed.description;
                        command.argument_hint = parsed.argument_hint;
                        command.allowed_tools = normalize_allowed_tools(parsed.allowed_tools);
                        command.model = parsed.model;
                    }
                    Err(_) => {
                        let parsed = parse_frontmatter_loose(&frontmatter);
                        command.description = parsed.description;
                        command.argument_hint = parsed.argument_hint;
                        command.allowed_tools = normalize_allowed_tools(parsed.allowed_tools);
                        command.model = parsed.model;
                    }
                }
            }
        }

        Ok(command)
    }

    pub fn to_markdown(&self) -> String {
        let mut lines = Vec::new();
        if !self.allowed_tools.is_empty() {
            lines.push(format!("allowed-tools: {}", self.allowed_tools.join(", ")));
        }
        if let Some(argument_hint) = &self.argument_hint {
            if !argument_hint.trim().is_empty() {
                lines.push(format!("argument-hint: {}", argument_hint.trim()));
            }
        }
        if let Some(description) = &self.description {
            if !description.trim().is_empty() {
                lines.push(format!("description: {}", description.trim()));
            }
        }
        if let Some(model) = &self.model {
            if !model.trim().is_empty() {
                lines.push(format!("model: {}", model.trim()));
            }
        }

        let frontmatter = lines.join("\n");
        let body = self.content.trim_end();
        format!("---\n{}\n---\n\n{}", frontmatter, body)
    }

    pub fn render(&self, args_raw: &str) -> String {
        let mut rendered = self.content.clone();
        let arguments = args_raw.trim();
        if rendered.contains("$ARGUMENTS") {
            rendered = rendered.replace("$ARGUMENTS", arguments);
        }

        let args: Vec<&str> = if arguments.is_empty() {
            Vec::new()
        } else {
            arguments.split_whitespace().collect()
        };

        let re = Regex::new(r"\$(\d+)").expect("valid regex");
        re.replace_all(&rendered, |caps: &regex::Captures| {
            let idx = caps[1].parse::<usize>().unwrap_or(0);
            if idx == 0 {
                ""
            } else {
                args.get(idx - 1).copied().unwrap_or("")
            }
        })
        .to_string()
    }
}

pub async fn render_custom_command_input(input: &str) -> Result<Option<RenderedCustomCommand>> {
    let trimmed = input.trim();
    if !trimmed.starts_with('/') {
        return Ok(None);
    }

    let mut iter = trimmed
        .trim_start_matches('/')
        .splitn(2, |c: char| c.is_whitespace());
    let name = iter.next().unwrap_or("").trim();
    if name.is_empty() {
        return Ok(None);
    }
    let args_raw = iter.next().unwrap_or("").trim();

    let command = match load_custom_command(name).await? {
        Some(command) => command,
        None => return Ok(None),
    };

    let args_count = if args_raw.is_empty() {
        0
    } else {
        args_raw.split_whitespace().count()
    };
    if let Some(required) = required_arg_count(&command) {
        if args_count < required {
            let message = missing_args_message(&command);
            return Err(anyhow!(message));
        }
    }

    let message = command.render(args_raw);
    Ok(Some(RenderedCustomCommand { command, message }))
}

pub async fn list_custom_commands() -> Result<Vec<CustomCommand>> {
    let dir = commands_dir()?;
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut entries = fs::read_dir(&dir).await?;
    let mut commands = Vec::new();

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("md") {
            continue;
        }
        let name = match path.file_stem().and_then(|s| s.to_str()) {
            Some(name) if !name.trim().is_empty() => name.to_string(),
            _ => continue,
        };
        match load_command_from_path(&path, name).await {
            Ok(command) => commands.push(command),
            Err(e) => eprintln!("Failed to load command {:?}: {}", path, e),
        }
    }

    Ok(commands)
}

pub async fn load_custom_command(name: &str) -> Result<Option<CustomCommand>> {
    let normalized = normalize_command_name(name)?;
    let path = command_file_path(&normalized)?;
    if !path.exists() {
        return Ok(None);
    }
    let command = load_command_from_path(&path, normalized).await?;
    Ok(Some(command))
}

pub async fn save_custom_command(command: &CustomCommand) -> Result<()> {
    let normalized = normalize_command_name(&command.name)?;
    let dir = commands_dir()?;
    fs::create_dir_all(&dir).await?;
    let path = command_file_path(&normalized)?;
    let content = CustomCommand {
        name: normalized,
        ..command.clone()
    }
    .to_markdown();
    fs::write(path, content).await?;
    Ok(())
}

pub async fn delete_custom_command(name: &str) -> Result<()> {
    let normalized = normalize_command_name(name)?;
    let path = command_file_path(&normalized)?;
    if path.exists() {
        fs::remove_file(path).await?;
    }
    Ok(())
}

fn split_frontmatter(content: &str) -> Result<(Option<String>, String)> {
    let mut lines = content.lines();
    let first = lines.next().unwrap_or("");
    if first.trim() != "---" {
        return Ok((None, content.to_string()));
    }

    let mut frontmatter_lines = Vec::new();
    let mut found_end = false;
    for line in lines.by_ref() {
        if line.trim() == "---" {
            found_end = true;
            break;
        }
        frontmatter_lines.push(line);
    }

    if !found_end {
        return Err(anyhow!(
            "Custom command frontmatter is missing a closing ---"
        ));
    }

    let frontmatter = frontmatter_lines.join("\n");
    let body = lines.collect::<Vec<_>>().join("\n");
    Ok((Some(frontmatter), body))
}

fn normalize_allowed_tools(input: Option<AllowedToolsDef>) -> Vec<String> {
    match input {
        Some(AllowedToolsDef::List(list)) => list
            .into_iter()
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect(),
        Some(AllowedToolsDef::String(value)) => value
            .split(',')
            .map(|item| item.trim().to_string())
            .filter(|item| !item.is_empty())
            .collect(),
        None => Vec::new(),
    }
}

fn parse_frontmatter_loose(frontmatter: &str) -> CustomCommandFrontmatter {
    let mut parsed = CustomCommandFrontmatter::default();
    let mut allowed_list: Vec<String> = Vec::new();
    let mut reading_allowed_list = false;
    for line in frontmatter.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if reading_allowed_list {
            if let Some(item) = trimmed.strip_prefix("- ") {
                let item = strip_quotes(item).trim();
                if !item.is_empty() {
                    allowed_list.push(item.to_string());
                }
                continue;
            } else {
                reading_allowed_list = false;
            }
        }

        let mut parts = trimmed.splitn(2, ':');
        let key = parts.next().unwrap_or("").trim();
        let value = parts.next().unwrap_or("").trim();
        if key.is_empty() {
            continue;
        }
        match key {
            "allowed-tools" | "allowed_tools" => {
                if value.is_empty() {
                    reading_allowed_list = true;
                    allowed_list = Vec::new();
                } else {
                    let cleaned = strip_quotes(value);
                    parsed.allowed_tools = Some(AllowedToolsDef::String(cleaned.to_string()));
                }
            }
            "argument-hint" | "argument_hint" => {
                if !value.is_empty() {
                    parsed.argument_hint = Some(strip_quotes(value).to_string());
                }
            }
            "description" => {
                if !value.is_empty() {
                    parsed.description = Some(strip_quotes(value).to_string());
                }
            }
            "model" => {
                if !value.is_empty() {
                    parsed.model = Some(strip_quotes(value).to_string());
                }
            }
            _ => {}
        }
    }

    if !allowed_list.is_empty() {
        parsed.allowed_tools = Some(AllowedToolsDef::List(allowed_list));
    }
    parsed
}

fn strip_quotes(value: &str) -> &str {
    let trimmed = value.trim();
    if trimmed.len() >= 2 {
        let bytes = trimmed.as_bytes();
        if (bytes[0] == b'"' && bytes[trimmed.len() - 1] == b'"')
            || (bytes[0] == b'\'' && bytes[trimmed.len() - 1] == b'\'')
        {
            return &trimmed[1..trimmed.len() - 1];
        }
    }
    trimmed
}

fn required_arg_count(command: &CustomCommand) -> Option<usize> {
    let mut required = 0usize;
    if command.content.contains("$ARGUMENTS") {
        required = 1;
    }

    let re = Regex::new(r"\$(\d+)").expect("valid regex");
    for caps in re.captures_iter(&command.content) {
        if let Ok(idx) = caps[1].parse::<usize>() {
            if idx > required {
                required = idx;
            }
        }
    }

    if required > 0 {
        Some(required)
    } else {
        None
    }
}

fn missing_args_message(command: &CustomCommand) -> String {
    let base = format!("Command '/{}' requires arguments.", command.name);
    if let Some(hint) = command
        .argument_hint
        .as_ref()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
    {
        format!("{} Usage: /{} {}", base, command.name, hint)
    } else {
        base
    }
}

fn normalize_command_name(name: &str) -> Result<String> {
    let trimmed = name.trim().trim_start_matches('/');
    let trimmed = trimmed.strip_suffix(".md").unwrap_or(trimmed);
    if trimmed.is_empty() {
        return Err(anyhow!("Command name is required"));
    }
    if trimmed.contains('/') || trimmed.contains('\\') {
        return Err(anyhow!("Command name cannot contain path separators"));
    }
    Ok(trimmed.to_string())
}

fn commands_dir() -> Result<PathBuf> {
    if let Ok(dir) = std::env::var("FLEXORAMA_COMMANDS_DIR") {
        if !dir.trim().is_empty() {
            return Ok(PathBuf::from(dir));
        }
    }
    let dir = dirs::home_dir()
        .ok_or_else(|| anyhow!("Home directory not found"))?
        .join(".flexorama")
        .join("commands");
    Ok(dir)
}

fn command_file_path(name: &str) -> Result<PathBuf> {
    Ok(commands_dir()?.join(format!("{}.md", name)))
}

async fn load_command_from_path(path: &Path, name: String) -> Result<CustomCommand> {
    let content = fs::read_to_string(path).await?;
    CustomCommand::from_markdown(name, &content)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    struct EnvGuard {
        home: Option<std::ffi::OsString>,
        userprofile: Option<std::ffi::OsString>,
        commands_dir: Option<std::ffi::OsString>,
    }

    impl EnvGuard {
        fn set_commands_dir(path: &Path) -> Self {
            let home = std::env::var_os("HOME");
            let userprofile = std::env::var_os("USERPROFILE");
            let commands_dir = std::env::var_os("FLEXORAMA_COMMANDS_DIR");
            std::env::set_var("FLEXORAMA_COMMANDS_DIR", path);
            Self {
                home,
                userprofile,
                commands_dir,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = self.home.take() {
                std::env::set_var("HOME", value);
            } else {
                std::env::remove_var("HOME");
            }
            if let Some(value) = self.userprofile.take() {
                std::env::set_var("USERPROFILE", value);
            } else {
                std::env::remove_var("USERPROFILE");
            }
            if let Some(value) = self.commands_dir.take() {
                std::env::set_var("FLEXORAMA_COMMANDS_DIR", value);
            } else {
                std::env::remove_var("FLEXORAMA_COMMANDS_DIR");
            }
        }
    }

    #[test]
    fn test_from_markdown_parses_yaml_frontmatter() {
        let input = r#"---
allowed-tools:
  - Read
  - Write
argument-hint: [path]
description: Read and write
model: test-model
---

Do something with $1"#;
        let cmd = CustomCommand::from_markdown("demo".to_string(), input).expect("parse");
        assert_eq!(cmd.name, "demo");
        assert_eq!(cmd.argument_hint.as_deref(), Some("[path]"));
        assert_eq!(cmd.description.as_deref(), Some("Read and write"));
        assert_eq!(cmd.model.as_deref(), Some("test-model"));
        assert_eq!(
            cmd.allowed_tools,
            vec!["Read".to_string(), "Write".to_string()]
        );
        assert_eq!(cmd.content, "Do something with $1");
    }

    #[test]
    fn test_from_markdown_parses_loose_frontmatter() {
        let input = r#"---
allowed-tools: Bash(git add:*), Bash(git status:*)
argument-hint: [issue]
---

Fix $ARGUMENTS"#;
        let cmd = CustomCommand::from_markdown("fix".to_string(), input).expect("parse");
        assert_eq!(cmd.allowed_tools.len(), 2);
        assert_eq!(
            cmd.allowed_tools,
            vec![
                "Bash(git add:*)".to_string(),
                "Bash(git status:*)".to_string()
            ]
        );
        assert_eq!(cmd.argument_hint.as_deref(), Some("[issue]"));
        assert_eq!(cmd.content, "Fix $ARGUMENTS");
    }

    #[test]
    fn test_render_substitutes_args() {
        let cmd = CustomCommand {
            name: "demo".to_string(),
            description: None,
            argument_hint: None,
            allowed_tools: Vec::new(),
            model: None,
            content: "One=$1 Two=$2 Rest=$ARGUMENTS".to_string(),
        };
        let rendered = cmd.render("alpha beta gamma");
        assert_eq!(rendered, "One=alpha Two=beta Rest=alpha beta gamma");
    }

    #[tokio::test]
    #[serial]
    async fn test_render_custom_command_requires_args() {
        let temp = tempdir().expect("tempdir");
        let _guard = EnvGuard::set_commands_dir(temp.path());
        let cmd = CustomCommand {
            name: "issue".to_string(),
            description: None,
            argument_hint: Some("[issue-number]".to_string()),
            allowed_tools: Vec::new(),
            model: None,
            content: "Fix $ARGUMENTS".to_string(),
        };
        save_custom_command(&cmd).await.expect("save");
        let err = render_custom_command_input("/issue")
            .await
            .expect_err("expected error");
        assert!(err.to_string().contains("requires arguments"));
        assert!(err.to_string().contains("Usage: /issue [issue-number]"));
    }

    #[tokio::test]
    #[serial]
    async fn test_render_custom_command_success() {
        let temp = tempdir().expect("tempdir");
        let _guard = EnvGuard::set_commands_dir(temp.path());
        let cmd = CustomCommand {
            name: "review".to_string(),
            description: None,
            argument_hint: None,
            allowed_tools: Vec::new(),
            model: None,
            content: "Review $1".to_string(),
        };
        save_custom_command(&cmd).await.expect("save");
        let rendered = render_custom_command_input("/review src/main.rs")
            .await
            .expect("render")
            .expect("command");
        assert_eq!(rendered.message, "Review src/main.rs");
    }

    #[tokio::test]
    #[serial]
    async fn test_list_and_delete_commands() {
        let temp = tempdir().expect("tempdir");
        let _guard = EnvGuard::set_commands_dir(temp.path());
        let cmd_a = CustomCommand {
            name: "one".to_string(),
            description: None,
            argument_hint: None,
            allowed_tools: Vec::new(),
            model: None,
            content: "A".to_string(),
        };
        let cmd_b = CustomCommand {
            name: "two".to_string(),
            description: None,
            argument_hint: None,
            allowed_tools: Vec::new(),
            model: None,
            content: "B".to_string(),
        };
        save_custom_command(&cmd_a).await.expect("save a");
        save_custom_command(&cmd_b).await.expect("save b");
        let mut names = list_custom_commands()
            .await
            .expect("list")
            .into_iter()
            .map(|c| c.name)
            .collect::<Vec<_>>();
        names.sort();
        assert_eq!(names, vec!["one".to_string(), "two".to_string()]);
        delete_custom_command("one").await.expect("delete");
        let names = list_custom_commands()
            .await
            .expect("list")
            .into_iter()
            .map(|c| c.name)
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["two".to_string()]);
    }
}
