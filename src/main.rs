use anyhow::anyhow;
use anyhow::Result;
use chrono::Local;
use clap::Parser;
use colored::*;
use dialoguer::Select;
use std::collections::VecDeque;
use std::io::{self, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use tokio::sync::Mutex as AsyncMutex;

use indicatif::{ProgressBar, ProgressStyle};
use log::{debug, error, info, warn};

#[macro_use]
mod output;

mod agent;
mod anthropic;
mod autocomplete;
mod config;
mod conversation;
mod database;
mod formatter;
mod gemini;
mod help;
mod input;
mod logo;
mod mcp;
mod openai;
mod security;
mod subagent;
mod tui;
mod web;

mod llm;
mod tools;

#[cfg(test)]
mod formatter_tests;

use agent::Agent;
use config::{Config, Provider};
use database::{
    get_database_path, Conversation as StoredConversation, DatabaseManager,
    Message as StoredMessage,
};
use formatter::create_code_formatter;
use help::{
    display_mcp_yolo_warning, display_yolo_warning, print_agent_help, print_file_permissions_help,
    print_help, print_mcp_help, print_permissions_help,
};
use mcp::McpManager;

/// Create a streaming renderer
fn create_streaming_renderer(
    formatter: &formatter::CodeFormatter,
) -> (
    Arc<Mutex<formatter::StreamingResponseFormatter>>,
    Arc<dyn Fn(String) + Send + Sync>,
) {
    let state = Arc::new(Mutex::new(formatter::StreamingResponseFormatter::new(
        formatter.clone(),
    )));
    let callback_state = Arc::clone(&state);
    let callback: Arc<dyn Fn(String) + Send + Sync> = Arc::new(move |content: String| {
        if content.is_empty() {
            return;
        }
        if let Ok(mut renderer) = callback_state.lock() {
            if let Err(e) = renderer.handle_chunk(&content) {
                app_eprintln!("{} Streaming formatter error: {}", "Error".red(), e);
            }
        }
    });
    (state, callback)
}

/// Process input and handle streaming/non-streaming response
async fn process_input(
    input: &str,
    agent: &mut Agent,
    formatter: &formatter::CodeFormatter,
    stream: bool,
    cancellation_flag: Arc<AtomicBool>,
) {
    // Show spinner while processing (only for non-streaming)
    if stream {
        let (streaming_state, stream_callback) = create_streaming_renderer(formatter);
        let result = agent
            .process_message_with_stream(
                &input,
                Some(Arc::clone(&stream_callback)),
                None,
                cancellation_flag.clone(),
            )
            .await;

        if let Ok(mut renderer) = streaming_state.lock() {
            if let Err(e) = renderer.finish() {
                app_eprintln!("{} Streaming formatter error: {}", "Error".red(), e);
            }
        }

        match result {
            Ok(_response) => {
                app_println!();
            }
            Err(e) => {
                if e.to_string().contains("CANCELLED") {
                    // Cancellation handled silently
                } else {
                    app_eprintln!("{}: {}", "Error".red(), e);
                }
                app_println!();
            }
        }
    } else {
        let spinner = create_spinner();
        let result = agent
            .process_message(&input, cancellation_flag.clone())
            .await;
        spinner.finish_and_clear();

        match result {
            Ok(response) => {
                // Only print response if it's not empty (i.e., not just @file references)
                if !response.is_empty() {
                    if let Err(e) = formatter.print_formatted(&response) {
                        app_eprintln!("{} formatting response: {}", "Error".red(), e);
                    }
                }
                app_println!();
            }
            Err(e) => {
                if e.to_string().contains("CANCELLED") {
                    // Cancellation handled silently
                } else {
                    app_eprintln!("{}: {}", "Error".red(), e);
                }
                app_println!();
            }
        }
    }
}

async fn run_tui_interactive(
    tui: Arc<tui::Tui>,
    agent: &mut Agent,
    mcp_manager: &Arc<McpManager>,
    formatter: &formatter::CodeFormatter,
    stream: bool,
    plan_mode: bool,
) -> Result<()> {
    enum InputEvent {
        Queued,
        Exit,
    }

    let tui_for_permissions = Arc::clone(&tui);
    agent
        .set_permission_handler(Some(Arc::new(move |prompt| {
            let tui_for_permissions = Arc::clone(&tui_for_permissions);
            let prompt = prompt.clone();
            Box::pin(async move {
                tokio::task::spawn_blocking(move || tui_for_permissions.prompt_permission(&prompt))
                    .await
                    .ok()
                    .flatten()
            })
        })))
        .await;

    logo::display_logo();
    app_println!("{}", "?? AIxplosion - Interactive Mode".green().bold());
    if plan_mode {
        app_println!(
            "{}",
            "Plan mode enabled: generating read-only plans and saving them to the database."
                .yellow()
                .bold()
        );
    }
    app_println!("{}", "Type '/help' for commands.".dimmed());
    app_println!();

    let queued_inputs = Arc::new(Mutex::new(VecDeque::new()));
    tui.set_queue(&VecDeque::new())?;

    let current_cancel_flag = Arc::new(Mutex::new(None::<Arc<AtomicBool>>));
    let exit_requested = Arc::new(AtomicBool::new(false));
    let (input_tx, mut input_rx) = mpsc::unbounded_channel::<InputEvent>();

    let tui_for_input = Arc::clone(&tui);
    let queue_for_input = Arc::clone(&queued_inputs);
    let cancel_for_input = Arc::clone(&current_cancel_flag);
    let exit_for_input = Arc::clone(&exit_requested);
    let input_thread = tokio::task::spawn_blocking(move || loop {
        match tui_for_input.read_input() {
            Ok(tui::InputResult::Submitted(value)) => {
                if value.trim().is_empty() {
                    continue;
                }
                let snapshot = {
                    let mut guard = queue_for_input.lock().expect("queue lock");
                    guard.push_back(value);
                    guard.clone()
                };
                let _ = tui_for_input.set_queue(&snapshot);
                if input_tx.send(InputEvent::Queued).is_err() {
                    break;
                }
            }
            Ok(tui::InputResult::Cancelled) => {
                let cancel = { cancel_for_input.lock().expect("cancel lock").clone() };
                if let Some(flag) = cancel {
                    flag.store(true, Ordering::SeqCst);
                    app_println!("\n{} Cancelling AI conversation...", "??".yellow());
                }
            }
            Ok(tui::InputResult::Exit) => {
                exit_for_input.store(true, Ordering::SeqCst);
                let cancel = { cancel_for_input.lock().expect("cancel lock").clone() };
                if let Some(flag) = cancel {
                    flag.store(true, Ordering::SeqCst);
                    app_println!("\n{} Cancelling and exiting...", "??".yellow());
                }
                let _ = input_tx.send(InputEvent::Exit);
                break;
            }
            Err(_) => break,
        }
    });

    let clear_queue = |queue: &Arc<Mutex<VecDeque<String>>>| -> Result<()> {
        let snapshot = {
            let mut guard = queue.lock().expect("queue lock");
            guard.clear();
            guard.clone()
        };
        tui.set_queue(&snapshot)?;
        Ok(())
    };

    loop {
        if exit_requested.load(Ordering::SeqCst) {
            clear_queue(&queued_inputs)?;
            print_usage_stats(agent);
            app_println!("{}", "Goodbye! ??".green());
            break;
        }

        let next_input = {
            let mut guard = queued_inputs.lock().expect("queue lock");
            let next = guard.pop_front();
            let snapshot = guard.clone();
            drop(guard);
            if next.is_some() {
                let _ = tui.set_queue(&snapshot);
            }
            next
        };

        let input = if let Some(value) = next_input {
            value
        } else {
            match input_rx.recv().await {
                Some(InputEvent::Queued) => continue,
                Some(InputEvent::Exit) => continue,
                None => break,
            }
        };

        let highlighted = formatter.format_input_with_file_highlighting(&input);
        app_println!();
        app_println!("> {}", highlighted);

        let trimmed = input.trim();
        if trimmed.starts_with('/')
            || trimmed.starts_with('!')
            || trimmed == "exit"
            || trimmed == "quit"
        {
            if trimmed.starts_with('/') {
                if let Err(e) = handle_slash_command(
                    trimmed,
                    agent,
                    mcp_manager,
                    formatter,
                    stream,
                    Some(tui.as_ref()),
                )
                .await
                {
                    app_eprintln!("{} Error handling command: {}", "?".red(), e);
                }
                let mut guard = current_cancel_flag.lock().expect("cancel lock");
                *guard = None;
                continue;
            }

            if trimmed.starts_with('!') {
                if let Err(e) = handle_shell_command(trimmed, agent).await {
                    app_eprintln!("{} Error executing shell command: {}", "?".red(), e);
                }
                let mut guard = current_cancel_flag.lock().expect("cancel lock");
                *guard = None;
                continue;
            }

            if trimmed == "exit" || trimmed == "quit" {
                exit_requested.store(true, Ordering::SeqCst);
                clear_queue(&queued_inputs)?;
                let mut guard = current_cancel_flag.lock().expect("cancel lock");
                *guard = None;
                continue;
            }
        }

        let cancellation_flag_for_processing = Arc::new(AtomicBool::new(false));
        {
            let mut guard = current_cancel_flag.lock().expect("cancel lock");
            *guard = Some(cancellation_flag_for_processing.clone());
        }
        process_input(
            &input,
            agent,
            formatter,
            stream,
            cancellation_flag_for_processing.clone(),
        )
        .await;
        {
            let mut guard = current_cancel_flag.lock().expect("cancel lock");
            *guard = None;
        }
    }

    input_thread.abort();
    Ok(())
}

/// Check for and add context files
async fn add_context_files(agent: &mut Agent, context_files: &[String]) -> Result<()> {
    // Always add AGENTS.md from ~/.aixplosion/ if it exists (priority)
    let home_agents_md = get_home_agents_md_path();
    if home_agents_md.exists() {
        debug!("Auto-adding AGENTS.md from ~/.aixplosion/ as context");
        match agent
            .add_context_file(home_agents_md.to_str().unwrap())
            .await
        {
            Ok(_) => app_println!(
                "{} Added context file: {}",
                "✓".green(),
                home_agents_md.display()
            ),
            Err(e) => app_eprintln!(
                "{} Failed to add context file '{}': {}",
                "✗".red(),
                home_agents_md.display(),
                e
            ),
        }
    }

    // Also add AGENTS.md from current directory if it exists (in addition to home directory version)
    if Path::new("AGENTS.md").exists() {
        debug!("Auto-adding AGENTS.md from current directory as context");
        match agent.add_context_file("AGENTS.md").await {
            Ok(_) => app_println!("{} Added context file: {}", "✓".green(), "AGENTS.md"),
            Err(e) => app_eprintln!(
                "{} Failed to add context file 'AGENTS.md': {}",
                "✗".red(),
                e
            ),
        }
    }

    // Add any additional context files specified by the user
    for file_path in context_files {
        debug!("Adding context file: {}", file_path);
        match agent.add_context_file(file_path).await {
            Ok(_) => app_println!("{} Added context file: {}", "✓".green(), file_path),
            Err(e) => app_eprintln!(
                "{} Failed to add context file '{}': {}",
                "✗".red(),
                file_path,
                e
            ),
        }
    }

    Ok(())
}

/// Get the path to AGENTS.md in the user's home .aixplosion directory
fn get_home_agents_md_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".aixplosion")
        .join("AGENTS.md")
}

async fn handle_agent_command(
    args: &[&str],
    agent: &mut Agent,
    _formatter: &formatter::CodeFormatter,
    _stream: bool,
) -> Result<()> {
    let mut subagent_manager = subagent::SubagentManager::new()?;
    subagent_manager.load_all_subagents().await?;

    if args.is_empty() {
        // Show current subagent status
        if agent.is_subagent_mode() {
            app_println!("{}", "🤖 Current Subagent".cyan().bold());
            app_println!("  You are currently in a subagent session");
            app_println!("  Use '/agent exit' to return to default mode");
        } else {
            app_println!("{}", "🤖 Subagent Management".cyan().bold());
            app_println!("  No subagent currently active");
        }
        app_println!();
        print_agent_help();
        return Ok(());
    }

    match args[0] {
        "list" => {
            let subagents = subagent_manager.list_subagents();
            if subagents.is_empty() {
                app_println!(
                    "{}",
                    "No subagents configured. Use '/agent create' to create one.".yellow()
                );
                return Ok(());
            }

            app_println!("{}", "🤖 Available Subagents".cyan().bold());
            app_println!();
            for subagent in subagents {
                let status = if agent.is_subagent_mode()
                    && agent
                        .get_system_prompt()
                        .map_or(false, |p| p.contains(&subagent.system_prompt))
                {
                    "✅ Active".green().to_string()
                } else {
                    "⏸️ Inactive".yellow().to_string()
                };

                app_println!(
                    "  {} {} ({})",
                    "Agent:".bold(),
                    subagent.name.cyan(),
                    status
                );
                if !subagent.allowed_tools.is_empty() {
                    let allowed_tools: Vec<&str> =
                        subagent.allowed_tools.iter().map(|s| s.as_str()).collect();
                    app_println!("  Allowed tools: {}", allowed_tools.join(", "));
                }
                if !subagent.denied_tools.is_empty() {
                    let denied_tools: Vec<&str> =
                        subagent.denied_tools.iter().map(|s| s.as_str()).collect();
                    app_println!("  Denied tools: {}", denied_tools.join(", "));
                }
                app_println!();
            }
        }
        "create" => {
            if args.len() < 3 {
                app_println!(
                    "{} Usage: /agent create <name> <system_prompt>",
                    "⚠️".yellow()
                );
                app_println!(
                    "{} Example: /agent create rust-expert \"You are a Rust expert...\"",
                    "💡".blue()
                );
                return Ok(());
            }

            let name = args[1];
            let system_prompt = args[2..].join(" ");

            // Default tool set for new subagent - use readonly tools from metadata
            let registry = agent.tool_registry.read().await;
            let allowed_tools: Vec<String> = registry
                .get_all_tools()
                .filter(|metadata| metadata.readonly)
                .map(|metadata| metadata.name.clone())
                .collect();

            match subagent_manager
                .create_subagent(name, &system_prompt, allowed_tools, vec![])
                .await
            {
                Ok(config) => {
                    app_println!("{} Created subagent: {}", "✅".green(), name.cyan());
                    app_println!("  Config file: ~/.aixplosion/agents/{}.md", name);
                }
                Err(e) => {
                    app_eprintln!("{} Failed to create subagent: {}", "✗".red(), e);
                }
            }
        }
        "use" | "switch" => {
            if args.len() < 2 {
                app_println!("{} Usage: /agent use <name>", "⚠️".yellow());
                return Ok(());
            }

            let name = args[1];
            if let Some(config) = subagent_manager.get_subagent(name) {
                match agent.switch_to_subagent(config).await {
                    Ok(_) => {
                        // Clear conversation context when switching to subagent
                        match agent.clear_conversation_keep_agents_md().await {
                            Ok(_) => {
                                app_println!(
                                    "{} Switched to subagent: {}",
                                    "✅".green(),
                                    name.cyan()
                                );
                                app_println!("{} Conversation context cleared", "🗑️".blue());
                            }
                            Err(e) => {
                                app_println!(
                                    "{} Switched to subagent: {}",
                                    "✅".green(),
                                    name.cyan()
                                );
                                app_eprintln!(
                                    "{} Failed to clear conversation context: {}",
                                    "⚠️".yellow(),
                                    e
                                );
                            }
                        }
                    }
                    Err(e) => {
                        app_eprintln!("{} Failed to switch to subagent: {}", "✗".red(), e);
                    }
                }
            } else {
                app_eprintln!("{} Subagent '{}' not found", "✗".red(), name);
                app_println!(
                    "{} Available subagents: {}",
                    "💡".blue(),
                    subagent_manager
                        .list_subagents()
                        .iter()
                        .map(|s| s.name.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
        }
        "exit" => match agent.exit_subagent().await {
            Ok(_) => {
                app_println!("{} Exited subagent mode", "✅".green());
                app_println!("{} Previous conversation context restored", "🔄".blue());
            }
            Err(e) => {
                app_eprintln!("{} Failed to exit subagent mode: {}", "✗".red(), e);
            }
        },
        "delete" => {
            if args.len() < 2 {
                app_println!("{} Usage: /agent delete <name>", "⚠️".yellow());
                return Ok(());
            }

            let name = args[1];
            app_println!(
                "{} Are you sure you want to delete subagent '{}'?",
                "⚠️".yellow(),
                name
            );
            app_println!("  This action cannot be undone.");
            app_println!("  Use '/agent delete {} --confirm' to proceed", name);

            if args.len() > 2 && args[2] == "--confirm" {
                match subagent_manager.delete_subagent(name).await {
                    Ok(_) => {
                        app_println!("{} Deleted subagent: {}", "✅".green(), name);
                    }
                    Err(e) => {
                        app_eprintln!("{} Failed to delete subagent: {}", "✗".red(), e);
                    }
                }
            }
        }
        "edit" => {
            if args.len() < 2 {
                app_println!("{} Usage: /agent edit <name>", "⚠️".yellow());
                return Ok(());
            }

            let name = args[1];
            let file_path = dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(".aixplosion")
                .join("agents")
                .join(format!("{}.md", name));

            if file_path.exists() {
                app_println!(
                    "{} Opening subagent config for editing: {}",
                    "📝".blue(),
                    file_path.display()
                );

                // Try to open in default editor
                #[cfg(target_os = "windows")]
                let _ = std::process::Command::new("notepad")
                    .arg(&file_path)
                    .status();

                #[cfg(not(target_os = "windows"))]
                {
                    if let Ok(editor) = std::env::var("EDITOR") {
                        let _ = std::process::Command::new(editor).arg(&file_path).status();
                    } else {
                        let _ = std::process::Command::new("nano").arg(&file_path).status();
                    }
                }

                app_println!(
                    "{} After editing, use '/agent reload {}' to apply changes",
                    "💡".blue(),
                    name
                );
            } else {
                app_eprintln!("{} Subagent '{}' not found", "✗".red(), name);
            }
        }
        "reload" => {
            subagent_manager.load_all_subagents().await?;
            app_println!("{} Reloaded subagent configurations", "✅".green());
        }
        "help" => {
            print_agent_help();
        }
        _ => {
            app_println!("{} Unknown agent command: {}", "⚠️".yellow(), args[0]);
            print_agent_help();
        }
    }

    Ok(())
}

async fn handle_shell_command(command: &str, _agent: &mut Agent) -> Result<()> {
    // Extract the shell command by removing the '!' prefix
    let shell_command = command.trim_start_matches('!').trim();

    if shell_command.is_empty() {
        app_println!(
            "{} Usage: !<command> - Execute a shell command",
            "⚠️".yellow()
        );
        app_println!("{} Examples: !dir, !ls -la, !git status", "💡".blue());
        return Ok(());
    }

    app_println!("{} Executing: {}", "🔧".blue(), shell_command);

    // Create a tool call for the bash command
    let tool_call = tools::ToolCall {
        id: "shell_command".to_string(),
        name: "bash".to_string(),
        arguments: serde_json::json!({
            "command": shell_command
        }),
    };

    // Execute the bash command directly without permission checks
    // This bypasses the security manager for ! commands
    execute_bash_command_directly(&tool_call)
        .await
        .map(|result| {
            if result.is_error {
                app_println!("{} Command failed:", "❌".red());
                app_println!("{}", result.content.red());
            } else {
                app_println!("{}", result.content);
            }
        })
        .map_err(|e| {
            app_eprintln!("{} Error executing shell command: {}", "✗".red(), e);
            e
        })?;

    Ok(())
}

/// Execute a bash command directly without security checks (for ! commands)
async fn execute_bash_command_directly(tool_call: &tools::ToolCall) -> Result<tools::ToolResult> {
    let command = tool_call
        .arguments
        .get("command")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("Missing 'command' argument"))?
        .to_string();

    debug!("Direct shell command execution: {}", command);

    let tool_use_id = tool_call.id.clone();

    // Execute the command using tokio::task to spawn blocking operation
    let command_clone = command.clone();
    match tokio::task::spawn_blocking(move || {
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("cmd")
                .args(["/C", &command_clone])
                .output()
        }
        #[cfg(not(target_os = "windows"))]
        {
            std::process::Command::new("bash")
                .args(["-c", &command_clone])
                .output()
        }
    })
    .await
    {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);

            let content = if !stderr.is_empty() {
                format!(
                    "Exit code: {}\nStdout:\n{}\nStderr:\n{}",
                    output.status.code().unwrap_or(-1),
                    stdout,
                    stderr
                )
            } else {
                format!(
                    "Exit code: {}\nOutput:\n{}",
                    output.status.code().unwrap_or(-1),
                    stdout
                )
            };

            Ok(tools::ToolResult {
                tool_use_id,
                content,
                is_error: !output.status.success(),
            })
        }
        Ok(Err(e)) => Ok(tools::ToolResult {
            tool_use_id,
            content: format!("Error executing command '{}': {}", command, e),
            is_error: true,
        }),
        Err(e) => Ok(tools::ToolResult {
            tool_use_id,
            content: format!("Task join error: {}", e),
            is_error: true,
        }),
    }
}

fn truncate_line(line: &str, max_chars: usize) -> String {
    let truncated: String = line.chars().take(max_chars).collect();
    if line.chars().count() > max_chars {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

fn build_message_preview(messages: &[StoredMessage]) -> String {
    if messages.is_empty() {
        return "(no messages)".to_string();
    }

    // Use the first message only; keep it single line and short
    let first_message = messages
        .iter()
        .find(|m| !m.content.trim().is_empty())
        .unwrap_or(&messages[0]);

    let first_line = first_message.content.lines().next().unwrap_or("").trim();
    let single_line = first_line.split_whitespace().collect::<Vec<_>>().join(" ");

    truncate_line(&single_line, 50)
}

fn format_resume_option(conversation: &StoredConversation, preview: &str) -> String {
    let updated_local = conversation
        .updated_at
        .with_timezone(&Local)
        .format("%Y-%m-%d %H:%M")
        .to_string();

    let short_id: String = conversation.id.chars().take(8).collect();
    let short_id = if conversation.id.len() > 8 {
        format!("{}…", short_id)
    } else {
        short_id
    };

    let meta = format!(
        "{} | Updated {} | Model {} | Requests {} | Tokens {}",
        short_id,
        updated_local,
        conversation.model,
        conversation.request_count,
        conversation.total_tokens
    );

    // Put preview on its own line and add a trailing newline to create spacing between items
    format!("{}\n  Preview: {}\n", meta, preview)
}

async fn build_conversation_previews(
    agent: &Agent,
    conversations: &[StoredConversation],
) -> Result<Vec<(StoredConversation, String)>> {
    let database_manager = agent
        .database_manager()
        .ok_or_else(|| anyhow!("Database is not configured"))?;

    let mut conversations_with_previews: Vec<(StoredConversation, String)> = Vec::new();

    for conversation in conversations {
        let messages = database_manager
            .get_conversation_messages(&conversation.id)
            .await?;
        if messages.is_empty() {
            continue; // Skip conversations with no messages
        }
        let preview = build_message_preview(&messages);
        conversations_with_previews.push((conversation.clone(), preview));
    }

    Ok(conversations_with_previews)
}

async fn select_conversation_index(
    prompt: &str,
    options: Vec<String>,
    cancel_message: &str,
    tui: Option<&tui::Tui>,
) -> Option<usize> {
    if let Some(tui) = tui {
        return select_index_with_tui(tui, prompt, &options, cancel_message).await;
    }

    let options_clone = options.clone();
    let prompt_text = prompt.to_string();
    let selection = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::task::spawn_blocking(move || {
            Select::new()
                .with_prompt(prompt_text)
                .items(&options_clone)
                .default(0) // Set first option as default
                .interact_opt()
        }),
    )
    .await;

    match selection {
        Ok(Ok(Ok(Some(index)))) => Some(index),
        Ok(Ok(Ok(None))) => {
            app_println!("{}", cancel_message.yellow());
            None
        }
        Ok(Ok(Err(e))) => {
            app_eprintln!("{} Failed to select conversation: {}", "?".red(), e);
            None
        }
        Ok(Err(e)) => {
            app_eprintln!("{} Failed to read selection: {}", "?".red(), e);
            None
        }
        Err(_) => {
            app_eprintln!(
                "{} Conversation selection timed out after 30 seconds.",
                "?".red()
            );
            None
        }
    }
}

async fn select_index_with_tui(
    tui: &tui::Tui,
    prompt: &str,
    options: &[String],
    cancel_message: &str,
) -> Option<usize> {
    app_println!("{}", prompt.cyan().bold());
    for (idx, option) in options.iter().enumerate() {
        app_println!("  {}. {}", idx + 1, option);
    }
    app_println!(
        "Enter a number (1-{}) and press Ctrl+Enter, or ESC to cancel.",
        options.len()
    );

    loop {
        match tui.read_input() {
            Ok(tui::InputResult::Submitted(value)) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(choice) = trimmed.parse::<usize>() {
                    if choice >= 1 && choice <= options.len() {
                        return Some(choice - 1);
                    }
                }
                app_println!(
                    "{} Invalid selection. Enter a number between 1 and {}.",
                    "??".yellow(),
                    options.len()
                );
            }
            Ok(tui::InputResult::Cancelled) => {
                app_println!("{}", cancel_message.yellow());
                return None;
            }
            Ok(tui::InputResult::Exit) => {
                app_println!("{}", cancel_message.yellow());
                return None;
            }
            Err(e) => {
                app_eprintln!("{} Failed to read selection: {}", "?".red(), e);
                return None;
            }
        }
    }
}

async fn handle_resume_command(agent: &mut Agent, tui: Option<&tui::Tui>) -> Result<()> {
    if agent.database_manager().is_none() {
        app_println!(
            "{} Database is not configured; cannot resume conversations.",
            "??".yellow()
        );
        return Ok(());
    }

    let current_id = agent.current_conversation_id();

    // Fetch more than 5 in case the current conversation is among the most recent
    let recent = agent.list_recent_conversations(15, None).await?;
    let available: Vec<StoredConversation> = recent
        .into_iter()
        .filter(|conv| Some(conv.id.as_str()) != current_id.as_deref())
        .take(5)
        .collect();

    if available.is_empty() {
        app_println!(
            "{} No other recent conversations found to resume.",
            "??".yellow()
        );
        return Ok(());
    }

    let conversations_with_previews = build_conversation_previews(agent, &available).await?;

    if conversations_with_previews.is_empty() {
        app_println!(
            "{} No recent conversations with messages found to resume.",
            "??".yellow()
        );
        return Ok(());
    }

    let options: Vec<String> = conversations_with_previews
        .iter()
        .map(|(conversation, preview)| format_resume_option(conversation, preview))
        .collect();

    let selected_index = select_conversation_index(
        "Select a conversation to resume",
        options,
        "Resume cancelled.",
        tui,
    )
    .await;

    if let Some(index) = selected_index {
        if let Some((conversation, _)) = conversations_with_previews.get(index) {
            agent.resume_conversation(&conversation.id).await?;
            app_println!(
                "{} Resumed conversation {} ({} messages loaded).",
                "û".green(),
                conversation.id,
                agent.conversation_len()
            );
        }
    }

    Ok(())
}

async fn handle_search_command(
    agent: &mut Agent,
    query: &str,
    tui: Option<&tui::Tui>,
) -> Result<()> {
    if agent.database_manager().is_none() {
        app_println!(
            "{} Database is not configured; cannot search conversations.",
            "??".yellow()
        );
        return Ok(());
    }

    let search_term = query.trim();
    if search_term.is_empty() {
        app_println!("{} Usage: /search <text>", "??".yellow());
        return Ok(());
    }

    let current_id = agent.current_conversation_id();
    let recent = agent
        .list_recent_conversations(30, Some(search_term))
        .await?;
    let available: Vec<StoredConversation> = recent
        .into_iter()
        .filter(|conv| Some(conv.id.as_str()) != current_id.as_deref())
        .collect();

    if available.is_empty() {
        app_println!(
            "{} No conversations matched '{}'.",
            "??".yellow(),
            search_term
        );
        return Ok(());
    }

    let conversations_with_previews = build_conversation_previews(agent, &available).await?;

    if conversations_with_previews.is_empty() {
        app_println!(
            "{} No matching conversations with messages found.",
            "??".yellow()
        );
        return Ok(());
    }

    let options: Vec<String> = conversations_with_previews
        .iter()
        .map(|(conversation, preview)| format_resume_option(conversation, preview))
        .collect();

    let prompt = format!("Select a conversation matching \"{}\"", search_term);
    let selected_index =
        select_conversation_index(&prompt, options, "Search cancelled.", tui).await;

    if let Some(index) = selected_index {
        if let Some((conversation, _)) = conversations_with_previews.get(index) {
            agent.resume_conversation(&conversation.id).await?;
            app_println!(
                "{} Resumed conversation {} ({} messages loaded).",
                "–".green(),
                conversation.id,
                agent.conversation_len()
            );
        }
    }

    Ok(())
}

async fn handle_slash_command(
    command: &str,
    agent: &mut Agent,
    mcp_manager: &McpManager,
    formatter: &formatter::CodeFormatter,
    stream: bool,
    tui: Option<&tui::Tui>,
) -> Result<bool> {
    let parts: Vec<&str> = command.trim().split(' ').collect();
    let cmd = parts[0];

    match cmd {
        "/help" => {
            print_help();
            Ok(true) // Command was handled
        }
        "/stats" | "/usage" => {
            print_usage_stats(agent);
            Ok(true) // Command was handled
        }
        "/context" => {
            agent.display_context();
            Ok(true) // Command was handled
        }
        "/provider" => {
            agent.display_provider();
            Ok(true)
        }
        "/model" => {
            let provider = agent.provider();
            let available = config::provider_models(provider);
            if parts.len() == 1 {
                app_println!("{}", "LLM Model".cyan().bold());
                app_println!("  Provider: {}", provider);
                app_println!("  Current: {}", agent.model());
                if !available.is_empty() {
                    app_println!("  Available:");
                    for model in available {
                        app_println!("    - {}", model);
                    }
                }
                app_println!("  Usage: /model <name> | /model list | /model pick");
                return Ok(true);
            }

            match parts[1] {
                "list" => {
                    app_println!("{}", "Available Models".cyan().bold());
                    app_println!("  Provider: {}", provider);
                    if available.is_empty() {
                        app_println!("  (no default models configured)");
                    } else {
                        for model in available {
                            app_println!("  - {}", model);
                        }
                    }
                }
                "pick" => {
                    if available.is_empty() {
                        app_println!(
                            "{} No default models configured for {}",
                            "??".yellow(),
                            provider
                        );
                        return Ok(true);
                    }
                    if let Some(tui) = tui {
                        let options: Vec<String> =
                            available.iter().map(|model| model.to_string()).collect();
                        if let Some(index) = select_index_with_tui(
                            tui,
                            "Select a model",
                            &options,
                            "Model pick cancelled.",
                        )
                        .await
                        {
                            let new_model = options[index].clone();
                            agent.set_model(new_model.clone()).await?;
                            app_println!("{} Active model set to {}", "??".green(), new_model);
                        }
                    } else {
                        let selected = Select::new()
                            .with_prompt("Select a model")
                            .items(available)
                            .default(0)
                            .interact_opt()?;
                        if let Some(index) = selected {
                            let new_model = available[index].to_string();
                            agent.set_model(new_model.clone()).await?;
                            app_println!("{} Active model set to {}", "??".green(), new_model);
                        }
                    }
                }
                _ => {
                    let new_model = parts[1..].join(" ");
                    if new_model.is_empty() {
                        app_println!("{} Usage: /model <name>", "??".yellow());
                        return Ok(true);
                    }
                    agent.set_model(new_model.clone()).await?;
                    app_println!("{} Active model set to {}", "??".green(), new_model);
                }
            }
            Ok(true)
        }
        "/search" => {
            let search_text = command.trim_start_matches("/search").trim();
            handle_search_command(agent, search_text, tui).await?;
            Ok(true)
        }
        "/resume" => {
            handle_resume_command(agent, tui).await?;
            Ok(true)
        }
        "/clear" => {
            match agent.clear_conversation_keep_agents_md().await {
                Ok(_) => {
                    app_println!(
                        "{}",
                        "🧹 Conversation context cleared! (AGENTS.md preserved if it existed)"
                            .green()
                    );
                }
                Err(e) => {
                    app_eprintln!("{} Failed to clear context: {}", "✗".red(), e);
                }
            }
            Ok(true) // Command was handled
        }
        "/reset-stats" => {
            agent.reset_token_usage();
            app_println!("{}", "📊 Token usage statistics reset!".green());
            Ok(true) // Command was handled
        }
        "/mcp" => {
            handle_mcp_command(&parts[1..], mcp_manager).await?;
            // Force refresh MCP tools after any MCP command
            if let Err(e) = agent.force_refresh_mcp_tools().await {
                warn!("Failed to refresh MCP tools: {}", e);
            }
            Ok(true) // Command was handled
        }
        "/permissions" => {
            handle_permissions_command(&parts[1..], agent).await?;
            Ok(true) // Command was handled
        }
        "/file-permissions" => {
            handle_file_permissions_command(&parts[1..], agent).await?;
            Ok(true) // Command was handled
        }
        "/plan" => {
            // Parse subcommand with splitn to preserve plan IDs containing whitespace
            let mut plan_parts = command.splitn(3, ' ');
            let _ = plan_parts.next(); // "/plan"
            let sub = plan_parts.next().unwrap_or("").trim();
            match sub {
                "on" => {
                    agent.set_plan_mode(true).await?;
                    app_println!(
                        "{} Plan mode enabled: generating read-only plans and saving them to the database.",
                        "✓".green()
                    );
                }
                "off" => {
                    agent.set_plan_mode(false).await?;
                    app_println!(
                        "{} Plan mode disabled: execution tools restored.",
                        "✓".green()
                    );
                }
                "run" => {
                    let plan_id_raw = plan_parts.next().unwrap_or("").trim();
                    if plan_id_raw.is_empty() {
                        app_println!("{} Usage: /plan run <plan_id>", "ℹ".yellow());
                        return Ok(true);
                    }
                    let plan_id = plan_id_raw;
                    app_println!("{} Loading plan {}...", "…".cyan(), plan_id);
                    let message = agent.load_plan_for_execution(plan_id).await?;
                    app_println!(
                        "{} Running saved plan {} (plan mode disabled for execution).",
                        "→".green(),
                        plan_id
                    );
                    let cancellation_flag = Arc::new(AtomicBool::new(false));
                    if stream {
                        let (streaming_state, stream_callback) =
                            create_streaming_renderer(formatter);
                        let response = agent
                            .process_message_with_stream(
                                &message,
                                Some(Arc::clone(&stream_callback)),
                                None,
                                cancellation_flag,
                            )
                            .await;
                        if let Ok(mut renderer) = streaming_state.lock() {
                            if let Err(e) = renderer.finish() {
                                app_eprintln!("{} Streaming formatter error: {}", "Error".red(), e);
                            }
                        }
                        response?;
                    } else {
                        let spinner = create_spinner();
                        let response = agent.process_message(&message, cancellation_flag).await?;
                        spinner.finish_and_clear();
                        formatter.print_formatted(&response)?;
                    }
                }
                _ => {
                    app_println!(
                        "{} Unknown /plan command. Use '/plan on', '/plan off', or '/plan run <id>'.",
                        "ℹ".yellow()
                    );
                }
            }
            Ok(true)
        }
        "/agent" => {
            handle_agent_command(&parts[1..], agent, formatter, stream).await?;
            Ok(true)
        }
        "/exit" | "/quit" => {
            // Print final stats before exiting
            print_usage_stats(agent);
            app_println!("{}", "Goodbye! 👋".green());
            std::process::exit(0);
        }
        _ => {
            app_println!(
                "{} Unknown command: {}. Type /help for available commands.",
                "⚠️".yellow(),
                cmd
            );
            Ok(true) // Command was handled (as unknown)
        }
    }
}

/// Handle MCP commands
async fn handle_mcp_command(args: &[&str], mcp_manager: &McpManager) -> Result<()> {
    if args.is_empty() {
        print_mcp_help();
        return Ok(());
    }

    match args[0] {
        "list" => match mcp_manager.list_servers().await {
            Ok(servers) => {
                app_println!("{}", "🔌 MCP Servers".cyan().bold());
                app_println!();
                if servers.is_empty() {
                    app_println!("{}", "No MCP servers configured.".yellow());
                    return Ok(());
                }

                for (name, config, connected) in servers {
                    let status = if connected {
                        "✅ Connected".green().to_string()
                    } else if config.enabled {
                        "❌ Disconnected".red().to_string()
                    } else {
                        "⏸️ Disabled".yellow().to_string()
                    };

                    app_println!("{} {} ({})", "Server:".bold(), name.cyan(), status);

                    if let Some(command) = &config.command {
                        app_println!("  Command: {}", command);
                    }
                    if let Some(args) = &config.args {
                        app_println!("  Args: {}", args.join(" "));
                    }
                    if let Some(url) = &config.url {
                        app_println!("  URL: {}", url);
                    }

                    if connected {
                        if let Ok(tools) = mcp_manager.get_all_tools().await {
                            let server_tools: Vec<_> = tools
                                .iter()
                                .filter(|(server_name, _)| server_name == &name)
                                .collect();
                            app_println!("  Tools: {} available", server_tools.len());
                        }
                    }
                    app_println!();
                }
            }
            Err(e) => {
                app_eprintln!("{} Failed to list MCP servers: {}", "✗".red(), e);
            }
        },
        "connect" => {
            if args.len() < 2 {
                app_println!("{} Usage: /mcp connect <server_name>", "⚠️".yellow());
                return Ok(());
            }

            app_println!(
                "{} Connecting to MCP server: {}",
                "🔌".blue(),
                args[1].cyan()
            );

            match mcp_manager.connect_server(args[1]).await {
                Ok(_) => {
                    app_println!(
                        "{} Successfully connected to MCP server: {}",
                        "✅".green(),
                        args[1].cyan()
                    );

                    // Try to list available tools
                    match mcp_manager.get_all_tools().await {
                        Ok(tools) => {
                            let server_tools: Vec<_> = tools
                                .iter()
                                .filter(|(server_name, _)| server_name == args[1])
                                .collect();
                            if !server_tools.is_empty() {
                                app_println!(
                                    "{} Available tools: {}",
                                    "🛠️".blue(),
                                    server_tools.len()
                                );
                                for (_, tool) in server_tools {
                                    app_println!(
                                        "  - {} {}",
                                        tool.name.bold(),
                                        tool.description
                                            .as_ref()
                                            .unwrap_or(&"".to_string())
                                            .dimmed()
                                    );
                                }
                            }
                        }
                        Err(_) => {
                            app_println!("{} Connected but failed to list tools", "⚠️".yellow());
                        }
                    }
                }
                Err(e) => {
                    app_eprintln!(
                        "{} Failed to connect to MCP server '{}': {}",
                        "✗".red(),
                        args[1],
                        e
                    );
                    app_println!("{} Troubleshooting:", "💡".yellow());
                    app_println!("  1. Check if the server is properly configured: /mcp list");
                    app_println!("  2. Verify the command/URL is correct");
                    app_println!("  3. Ensure all dependencies are installed");
                    app_println!("  4. Check network connectivity for WebSocket servers");
                    app_println!("  5. Try reconnecting: /mcp reconnect {}", args[1]);
                }
            }
        }
        "disconnect" => {
            if args.len() < 2 {
                app_println!("{} Usage: /mcp disconnect <server_name>", "⚠️".yellow());
                return Ok(());
            }

            match mcp_manager.disconnect_server(args[1]).await {
                Ok(_) => {
                    app_println!(
                        "{} Disconnected from MCP server: {}",
                        "🔌".yellow(),
                        args[1].cyan()
                    );
                }
                Err(e) => {
                    app_eprintln!(
                        "{} Failed to disconnect from MCP server '{}': {}",
                        "✗".red(),
                        args[1],
                        e
                    );
                }
            }
        }
        "reconnect" => {
            if args.len() < 2 {
                app_println!("{} Usage: /mcp reconnect <server_name>", "⚠️".yellow());
                return Ok(());
            }

            match mcp_manager.reconnect_server(args[1]).await {
                Ok(_) => {
                    app_println!(
                        "{} Reconnected to MCP server: {}",
                        "🔄".blue(),
                        args[1].cyan()
                    );
                }
                Err(e) => {
                    app_eprintln!(
                        "{} Failed to reconnect to MCP server '{}': {}",
                        "✗".red(),
                        args[1],
                        e
                    );
                }
            }
        }
        "tools" => match mcp_manager.get_all_tools().await {
            Ok(tools) => {
                app_println!("{}", "🛠️  MCP Tools".cyan().bold());
                app_println!();

                if tools.is_empty() {
                    app_println!(
                        "{}",
                        "No MCP tools available. Connect to a server first.".yellow()
                    );
                    return Ok(());
                }

                let mut by_server = std::collections::HashMap::new();
                for (server_name, tool) in tools {
                    by_server
                        .entry(server_name)
                        .or_insert_with(Vec::new)
                        .push(tool);
                }

                for (server_name, server_tools) in by_server {
                    app_println!("{} {}:", "Server:".bold(), server_name.cyan());
                    for tool in server_tools {
                        app_println!("  🛠️  {}", tool.name.bold());
                        if let Some(description) = &tool.description {
                            app_println!("     {}", description.dimmed());
                        }
                    }
                    app_println!();
                }
            }
            Err(e) => {
                app_eprintln!("{} Failed to list MCP tools: {}", "✗".red(), e);
            }
        },
        "add" => {
            if args.len() < 4 {
                app_println!(
                    "{} Usage: /mcp add <name> stdio <command> [args...]",
                    "⚠️".yellow()
                );
                app_println!("{} Usage: /mcp add <name> ws <url>", "⚠️".yellow());
                app_println!();
                app_println!("{}", "Examples:".green().bold());
                app_println!(
                    "  /mcp add myserver stdio npx -y @modelcontextprotocol/server-filesystem"
                );
                app_println!("  /mcp add websocket ws://localhost:8080");
                return Ok(());
            }

            let name = args[1];
            let connection_type = args[2];

            if connection_type == "stdio" {
                let command = args[3];
                let server_args: Vec<String> = args[4..].iter().map(|s| s.to_string()).collect();

                // Validate that we have a proper command
                if command.is_empty() {
                    app_println!("{} Command cannot be empty", "⚠️".yellow());
                    return Ok(());
                }

                let server_config = mcp::McpServerConfig {
                    name: name.to_string(),
                    command: Some(command.to_string()),
                    args: if server_args.is_empty() {
                        None
                    } else {
                        Some(server_args)
                    },
                    url: None,
                    env: None,
                    enabled: true,
                };

                app_println!("{} Adding MCP server: {}", "🔧".blue(), name.cyan());
                app_println!("  Command: {}", command);
                if !args[4..].is_empty() {
                    app_println!("  Args: {}", args[4..].join(" "));
                }

                match mcp_manager.add_server(name, server_config).await {
                    Ok(_) => {
                        app_println!(
                            "{} Successfully added MCP server: {}",
                            "✅".green(),
                            name.cyan()
                        );
                        app_println!(
                            "{} Use '/mcp connect {}' to connect to this server",
                            "💡".blue(),
                            name
                        );
                    }
                    Err(e) => {
                        app_eprintln!("{} Failed to add MCP server '{}': {}", "✗".red(), name, e);
                        app_println!("{} Common issues:", "💡".yellow());
                        app_println!("  - Command '{}' not found or not executable", command);
                        app_println!("  - Missing dependencies (e.g., Node.js, npm, npx)");
                        app_println!("  - Network connectivity issues");
                        app_println!("  - Insufficient permissions");
                    }
                }
            } else if connection_type == "ws" || connection_type == "websocket" {
                let url = args[3];

                // Basic URL validation
                if !url.starts_with("ws://") && !url.starts_with("wss://") {
                    app_println!("{} URL must start with ws:// or wss://", "⚠️".yellow());
                    return Ok(());
                }

                let server_config = mcp::McpServerConfig {
                    name: name.to_string(),
                    command: None,
                    args: None,
                    url: Some(url.to_string()),
                    env: None,
                    enabled: true,
                };

                app_println!("{} Adding MCP server: {}", "🔧".blue(), name.cyan());
                app_println!("  URL: {}", url);

                match mcp_manager.add_server(name, server_config).await {
                    Ok(_) => {
                        app_println!(
                            "{} Successfully added MCP server: {}",
                            "✅".green(),
                            name.cyan()
                        );
                        app_println!(
                            "{} Use '/mcp connect {}' to connect to this server",
                            "💡".blue(),
                            name
                        );
                    }
                    Err(e) => {
                        app_eprintln!("{} Failed to add MCP server '{}': {}", "✗".red(), name, e);
                    }
                }
            } else {
                app_println!("{} Connection type must be 'stdio' or 'ws'", "⚠️".yellow());
                app_println!("{} Available types:", "💡".blue());
                app_println!("  - stdio: For command-line based MCP servers");
                app_println!("  - ws: For WebSocket-based MCP servers");
            }
        }
        "remove" => {
            if args.len() < 2 {
                app_println!("{} Usage: /mcp remove <server_name>", "⚠️".yellow());
                return Ok(());
            }

            match mcp_manager.remove_server(args[1]).await {
                Ok(_) => {
                    app_println!("{} Removed MCP server: {}", "🗑️".red(), args[1].cyan());
                }
                Err(e) => {
                    app_eprintln!(
                        "{} Failed to remove MCP server '{}': {}",
                        "✗".red(),
                        args[1],
                        e
                    );
                }
            }
        }
        "connect-all" => match mcp_manager.connect_all_enabled().await {
            Ok(_) => {
                app_println!(
                    "{} Attempted to connect to all enabled MCP servers",
                    "🔄".blue()
                );
            }
            Err(e) => {
                app_eprintln!("{} Failed to connect to MCP servers: {}", "✗".red(), e);
            }
        },
        "test" => {
            if args.len() < 2 {
                app_println!("{} Usage: /mcp test <command>", "⚠️".yellow());
                app_println!(
                    "{} Test if a command is available and executable",
                    "💡".blue()
                );
                return Ok(());
            }

            let command = args[1];
            app_println!("{} Testing command: {}", "🧪".blue(), command.cyan());

            // Try to run the command with --version or --help to test if it exists
            let test_args = if command == "npx" {
                vec!["--version".to_string()]
            } else {
                vec!["--version".to_string()]
            };

            match tokio::process::Command::new(command)
                .args(&test_args)
                .output()
                .await
            {
                Ok(output) => {
                    if output.status.success() {
                        app_println!(
                            "{} Command '{}' is available and executable",
                            "✅".green(),
                            command
                        );
                        if !output.stdout.is_empty() {
                            let version = String::from_utf8_lossy(&output.stdout);
                            app_println!("  Version: {}", version.trim());
                        }
                    } else {
                        app_println!(
                            "{} Command '{}' exists but failed to execute",
                            "⚠️".yellow(),
                            command
                        );
                        if !output.stderr.is_empty() {
                            let error = String::from_utf8_lossy(&output.stderr);
                            app_println!("  Error: {}", error.trim());
                        }
                    }
                }
                Err(e) => {
                    app_println!(
                        "{} Command '{}' not found or not executable",
                        "✗".red(),
                        command
                    );
                    app_println!("  Error: {}", e);
                    app_println!("{} Suggestions:", "💡".blue());
                    app_println!("  - Install the command/tool if missing");
                    app_println!("  - Check if the command is in your PATH");
                    app_println!("  - Use the full path to the command");
                }
            }
        }
        "disconnect-all" => match mcp_manager.disconnect_all().await {
            Ok(_) => {
                app_println!("{} Disconnected from all MCP servers", "🔌".yellow());
            }
            Err(e) => {
                app_eprintln!("{} Failed to disconnect from MCP servers: {}", "✗".red(), e);
            }
        },
        _ => {
            app_println!("{} Unknown MCP command: {}", "⚠️".yellow(), args[0]);
            print_mcp_help();
        }
    }

    Ok(())
}

/// Print MCP help information

/// Print usage statistics
fn print_usage_stats(agent: &Agent) {
    let usage = agent.get_token_usage();
    app_println!("{}", "📊 Token Usage Statistics".cyan().bold());
    app_println!();
    app_println!("{}", "Request Summary:".green().bold());
    app_println!("  Requests made: {}", usage.request_count);
    app_println!();
    app_println!("{}", "Token Usage:".green().bold());
    app_println!("  Input tokens:  {}", usage.total_input_tokens);
    app_println!("  Output tokens: {}", usage.total_output_tokens);
    app_println!("  Total tokens: {}", usage.total_tokens());
    app_println!();

    if usage.request_count > 0 {
        let avg_input = usage.total_input_tokens as f64 / usage.request_count as f64;
        let avg_output = usage.total_output_tokens as f64 / usage.request_count as f64;
        let avg_total = usage.total_tokens() as f64 / usage.request_count as f64;

        app_println!("{}", "Average per request:".green().bold());
        app_println!("  Input tokens:  {:.1}", avg_input);
        app_println!("  Output tokens: {:.1}", avg_output);
        app_println!("  Total tokens: {:.1}", avg_total);
        app_println!();
    }
}

/// Create a progress spinner for API calls
fn create_spinner() -> ProgressBar {
    if output::is_tui_active() {
        return ProgressBar::hidden();
    }
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::default_spinner()
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"])
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );
    spinner.set_message("Thinking...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(100));
    spinner
}

/// Handle permissions commands
async fn handle_permissions_command(args: &[&str], agent: &mut Agent) -> Result<()> {
    use crate::security::PermissionResult;

    if args.is_empty() {
        // Display current permissions with full details
        let security_manager_ref = agent.get_bash_security_manager().clone();
        let security_manager = security_manager_ref.read().await;
        security_manager.display_permissions();
        return Ok(());
    }

    match args[0] {
        "show" | "list" => {
            let security_manager_ref = agent.get_bash_security_manager().clone();
            let security_manager = security_manager_ref.read().await;
            security_manager.display_permissions();
        }
        "test" => {
            if args.len() < 2 {
                app_println!("{} Usage: /permissions test <command>", "⚠️".yellow());
                return Ok(());
            }

            let command = args[1..].join(" ");
            let security_manager_ref = agent.get_bash_security_manager().clone();
            let security_manager = security_manager_ref.read().await;

            match security_manager.check_command_permission(&command) {
                PermissionResult::Allowed => {
                    app_println!("{} Command '{}' is ALLOWED", "✅".green(), command);
                }
                PermissionResult::Denied => {
                    app_println!("{} Command '{}' is DENIED", "❌".red(), command);
                }
                PermissionResult::RequiresPermission => {
                    app_println!(
                        "{} Command '{}' requires permission",
                        "❓".yellow(),
                        command
                    );
                }
            }
        }
        "allow" => {
            if args.len() < 2 {
                app_println!(
                    "{} Usage: /permissions allow <command_pattern>",
                    "⚠️".yellow()
                );
                app_println!("{} Examples:", "💡".blue());
                app_println!("  /permissions allow 'git *'");
                app_println!("  /permissions allow 'cargo test'");
                app_println!("  /permissions allow 'ls -la'");
                return Ok(());
            }

            let command = args[1..].join(" ");
            let security_manager_ref = agent.get_bash_security_manager().clone();
            let mut security_manager = security_manager_ref.write().await;

            security_manager.add_to_allowlist(command.clone());
            app_println!("{} Added '{}' to allowlist", "✅".green(), command);

            // Save to config
            if let Err(e) = save_permissions_to_config(&agent).await {
                app_println!("{} Failed to save permissions: {}", "⚠️".yellow(), e);
            }
        }
        "deny" => {
            if args.len() < 2 {
                app_println!(
                    "{} Usage: /permissions deny <command_pattern>",
                    "⚠️".yellow()
                );
                app_println!("{} Examples:", "💡".blue());
                app_println!("  /permissions deny 'rm *'");
                app_println!("  /permissions deny 'sudo *'");
                app_println!("  /permissions deny 'format'");
                return Ok(());
            }

            let command = args[1..].join(" ");
            let security_manager_ref = agent.get_bash_security_manager().clone();
            let mut security_manager = security_manager_ref.write().await;

            security_manager.add_to_denylist(command.clone());
            app_println!("{} Added '{}' to denylist", "❌".red(), command);

            // Save to config
            if let Err(e) = save_permissions_to_config(&agent).await {
                app_println!("{} Failed to save permissions: {}", "⚠️".yellow(), e);
            }
        }
        "remove-allow" => {
            if args.len() < 2 {
                app_println!(
                    "{} Usage: /permissions remove-allow <command_pattern>",
                    "⚠️".yellow()
                );
                return Ok(());
            }

            let command = args[1..].join(" ");
            let security_manager_ref = agent.get_bash_security_manager().clone();
            let mut security_manager = security_manager_ref.write().await;

            if security_manager.remove_from_allowlist(&command) {
                app_println!("{} Removed '{}' from allowlist", "🗑️".yellow(), command);

                // Save to config
                if let Err(e) = save_permissions_to_config(&agent).await {
                    app_println!("{} Failed to save permissions: {}", "⚠️".yellow(), e);
                }
            } else {
                app_println!(
                    "{} Command '{}' not found in allowlist",
                    "⚠️".yellow(),
                    command
                );
            }
        }
        "remove-deny" => {
            if args.len() < 2 {
                app_println!(
                    "{} Usage: /permissions remove-deny <command_pattern>",
                    "⚠️".yellow()
                );
                return Ok(());
            }

            let command = args[1..].join(" ");
            let security_manager_ref = agent.get_bash_security_manager().clone();
            let mut security_manager = security_manager_ref.write().await;

            if security_manager.remove_from_denylist(&command) {
                app_println!("{} Removed '{}' from denylist", "🗑️".yellow(), command);

                // Save to config
                if let Err(e) = save_permissions_to_config(&agent).await {
                    app_println!("{} Failed to save permissions: {}", "⚠️".yellow(), e);
                }
            } else {
                app_println!(
                    "{} Command '{}' not found in denylist",
                    "⚠️".yellow(),
                    command
                );
            }
        }
        "enable" => {
            let security_manager_ref = agent.get_bash_security_manager().clone();
            let mut security_manager = security_manager_ref.write().await;
            let mut security = security_manager.get_security().clone();
            security.enabled = true;
            security_manager.update_security(security);
            app_println!("{} Bash security enabled", "✅".green());

            // Save to config
            if let Err(e) = save_permissions_to_config(&agent).await {
                app_println!("{} Failed to save permissions: {}", "⚠️".yellow(), e);
            }
        }
        "disable" => {
            let security_manager_ref = agent.get_bash_security_manager().clone();
            let mut security_manager = security_manager_ref.write().await;
            let mut security = security_manager.get_security().clone();
            security.enabled = false;
            security_manager.update_security(security);
            app_println!("{} Bash security disabled", "⚠️".yellow());
            app_println!(
                "{} Warning: This allows any bash command to be executed!",
                "⚠️".red().bold()
            );

            // Save to config
            if let Err(e) = save_permissions_to_config(&agent).await {
                app_println!("{} Failed to save permissions: {}", "⚠️".yellow(), e);
            }
        }
        "ask-on" => {
            let security_manager_ref = agent.get_bash_security_manager().clone();
            let mut security_manager = security_manager_ref.write().await;
            let mut security = security_manager.get_security().clone();
            security.ask_for_permission = true;
            security_manager.update_security(security);
            app_println!("{} Ask for permission enabled", "✅".green());

            // Save to config
            if let Err(e) = save_permissions_to_config(&agent).await {
                app_println!("{} Failed to save permissions: {}", "⚠️".yellow(), e);
            }
        }
        "ask-off" => {
            let security_manager_ref = agent.get_bash_security_manager().clone();
            let mut security_manager = security_manager_ref.write().await;
            let mut security = security_manager.get_security().clone();
            security.ask_for_permission = false;
            security_manager.update_security(security);
            app_println!("{} Ask for permission disabled", "⚠️".yellow());
            app_println!("{} Unknown commands will be denied by default", "⚠️".red());

            // Save to config
            if let Err(e) = save_permissions_to_config(&agent).await {
                app_println!("{} Failed to save permissions: {}", "⚠️".yellow(), e);
            }
        }
        "help" => {
            print_permissions_help();
        }
        _ => {
            app_println!("{} Unknown permissions command: {}", "⚠️".yellow(), args[0]);
            app_println!("{} Available commands:", "💡".yellow());
            app_println!("  /permissions                - Show current permissions");
            app_println!("  /permissions help          - Show permissions help");
            app_println!("  /permissions test <cmd>    - Test if a command is allowed");
            app_println!("  /permissions allow <cmd>   - Add command to allowlist");
            app_println!("  /permissions deny <cmd>    - Add command to denylist");
            app_println!("  /permissions remove-allow <cmd> - Remove from allowlist");
            app_println!("  /permissions remove-deny <cmd> - Remove from denylist");
            app_println!("  /permissions enable        - Enable bash security");
            app_println!("  /permissions disable       - Disable bash security");
            app_println!("  /permissions ask-on        - Enable asking for permission");
            app_println!("  /permissions ask-off       - Disable asking for permission");
        }
    }

    Ok(())
}

/// Save current permissions to unified config file
async fn save_permissions_to_config(agent: &Agent) -> Result<()> {
    use crate::config::Config;

    // Load existing config to preserve other settings
    let mut existing_config = Config::load(None).await?;

    // Get current security settings from agent
    let updated_config = agent.get_config_for_save().await;

    // Update only the bash_security settings
    existing_config.bash_security = updated_config.bash_security;

    // Save the updated config
    match existing_config.save(None).await {
        Ok(_) => {
            app_println!("{} Permissions saved to unified config", "💾".blue());
        }
        Err(e) => {
            app_println!("{} Failed to save permissions: {}", "⚠️".yellow(), e);
        }
    }

    Ok(())
}

/// Handle file permissions commands
async fn handle_file_permissions_command(args: &[&str], agent: &mut Agent) -> Result<()> {
    use crate::security::FilePermissionResult;

    if args.is_empty() {
        // Display current file permissions with full details
        let file_security_manager_ref = agent.get_file_security_manager().clone();
        let file_security_manager = file_security_manager_ref.read().await;
        file_security_manager.display_file_permissions();
        return Ok(());
    }

    match args[0] {
        "show" | "list" => {
            let file_security_manager_ref = agent.get_file_security_manager().clone();
            let file_security_manager = file_security_manager_ref.read().await;
            file_security_manager.display_file_permissions();
        }
        "test" => {
            if args.len() < 3 {
                app_println!(
                    "{} Usage: /file-permissions test <operation> <path>",
                    "⚠️".yellow()
                );
                app_println!(
                    "{} Operations: write_file, edit_file, delete_file, create_directory",
                    "💡".blue()
                );
                return Ok(());
            }

            let operation = args[1];
            let path = args[2..].join(" ");
            let file_security_manager_ref = agent.get_file_security_manager().clone();
            let mut file_security_manager = file_security_manager_ref.write().await;

            match file_security_manager.check_file_permission(operation, &path) {
                FilePermissionResult::Allowed => {
                    app_println!(
                        "{} File operation '{}' on '{}' is ALLOWED",
                        "✅".green(),
                        operation,
                        path
                    );
                }
                FilePermissionResult::Denied => {
                    app_println!(
                        "{} File operation '{}' on '{}' is DENIED",
                        "❌".red(),
                        operation,
                        path
                    );
                }
                FilePermissionResult::RequiresPermission => {
                    app_println!(
                        "{} File operation '{}' on '{}' requires permission",
                        "❓".yellow(),
                        operation,
                        path
                    );
                }
            }
        }
        "enable" => {
            let file_security_manager_ref = agent.get_file_security_manager().clone();
            let mut file_security_manager = file_security_manager_ref.write().await;
            let mut security = file_security_manager.get_file_security().clone();
            security.enabled = true;
            file_security_manager.update_file_security(security);
            app_println!("{} File security enabled", "✅".green());

            // Save to config
            if let Err(e) = save_file_permissions_to_config(&agent).await {
                app_println!("{} Failed to save file permissions: {}", "⚠️".yellow(), e);
            }
        }
        "disable" => {
            let file_security_manager_ref = agent.get_file_security_manager().clone();
            let mut file_security_manager = file_security_manager_ref.write().await;
            let mut security = file_security_manager.get_file_security().clone();
            security.enabled = false;
            file_security_manager.update_file_security(security);
            app_println!("{} File security disabled", "⚠️".yellow());
            app_println!(
                "{} Warning: This allows any file operation to be executed!",
                "⚠️".red().bold()
            );

            // Save to config
            if let Err(e) = save_file_permissions_to_config(&agent).await {
                app_println!("{} Failed to save file permissions: {}", "⚠️".yellow(), e);
            }
        }
        "ask-on" => {
            let file_security_manager_ref = agent.get_file_security_manager().clone();
            let mut file_security_manager = file_security_manager_ref.write().await;
            let mut security = file_security_manager.get_file_security().clone();
            security.ask_for_permission = true;
            file_security_manager.update_file_security(security);
            app_println!("{} Ask for file permission enabled", "✅".green());

            // Save to config
            if let Err(e) = save_file_permissions_to_config(&agent).await {
                app_println!("{} Failed to save file permissions: {}", "⚠️".yellow(), e);
            }
        }
        "ask-off" => {
            let file_security_manager_ref = agent.get_file_security_manager().clone();
            let mut file_security_manager = file_security_manager_ref.write().await;
            let mut security = file_security_manager.get_file_security().clone();
            security.ask_for_permission = false;
            file_security_manager.update_file_security(security);
            app_println!("{} Ask for file permission disabled", "⚠️".yellow());
            app_println!(
                "{} All file operations will be allowed by default",
                "⚠️".red()
            );

            // Save to config
            if let Err(e) = save_file_permissions_to_config(&agent).await {
                app_println!("{} Failed to save file permissions: {}", "⚠️".yellow(), e);
            }
        }
        "reset-session" => {
            let file_security_manager_ref = agent.get_file_security_manager().clone();
            let mut file_security_manager = file_security_manager_ref.write().await;
            file_security_manager.reset_session_permissions();
            app_println!("{} Session file permissions reset", "🔄".blue());
            app_println!(
                "{} File operations will require permission again",
                "💡".blue()
            );
        }
        "help" => {
            print_file_permissions_help();
        }
        _ => {
            app_println!(
                "{} Unknown file permissions command: {}",
                "⚠️".yellow(),
                args[0]
            );
            app_println!("{} Available commands:", "💡".yellow());
            app_println!("  /file-permissions                - Show current file permissions");
            app_println!("  /file-permissions help          - Show file permissions help");
            app_println!(
                "  /file-permissions test <op> <path> - Test if file operation is allowed"
            );
            app_println!("  /file-permissions enable        - Enable file security");
            app_println!("  /file-permissions disable       - Disable file security");
            app_println!("  /file-permissions ask-on        - Enable asking for permission");
            app_println!("  /file-permissions ask-off       - Disable asking for permission");
            app_println!("  /file-permissions reset-session - Reset session permissions");
        }
    }

    Ok(())
}

/// Save current file permissions to unified config file
async fn save_file_permissions_to_config(agent: &Agent) -> Result<()> {
    use crate::config::Config;

    // Load existing config to preserve other settings
    let mut existing_config = Config::load(None).await?;

    // Get current file security settings from agent
    let file_security_manager_ref = agent.get_file_security_manager().clone();
    let file_security_manager = file_security_manager_ref.read().await;
    let updated_file_security = file_security_manager.get_file_security().clone();

    // Update only the file_security settings
    existing_config.file_security = updated_file_security;

    // Save the updated config
    match existing_config.save(None).await {
        Ok(_) => {
            app_println!("{} File permissions saved to unified config", "💾".blue());
        }
        Err(e) => {
            app_println!("{} Failed to save file permissions: {}", "⚠️".yellow(), e);
        }
    }

    Ok(())
}

#[derive(Parser)]
#[command(name = "aixplosion")]
#[command(about = "A CLI coding agent with pluggable LLM providers")]
#[command(version)]
struct Cli {
    /// The message to send to the agent
    #[arg(short = 'm', long)]
    message: Option<String>,

    /// Set the API key (overrides config file)
    #[arg(short = 'k', long)]
    api_key: Option<String>,

    /// LLM provider to use (anthropic, gemini, or z.ai)
    #[arg(long)]
    provider: Option<config::Provider>,

    /// Specify the model to use
    #[arg(long)]
    model: Option<String>,

    /// Configuration file path
    #[arg(short, long)]
    config: Option<String>,

    /// Run in non-interactive mode
    #[arg(short, long)]
    non_interactive: bool,

    /// Files to include as context
    #[arg(short = 'f', long = "file", value_name = "FILE")]
    context_files: Vec<String>,

    /// System prompt to use for the conversation
    #[arg(short = 's', long = "system", value_name = "PROMPT")]
    system_prompt: Option<String>,

    /// Enable streaming responses
    #[arg(long)]
    stream: bool,

    /// Enable 'yolo' mode - bypass all permission checks for file and tool operations
    #[arg(long)]
    yolo: bool,

    /// Enable plan-only mode (generate a plan in Markdown without making changes)
    #[arg(long = "plan-mode")]
    plan_mode: bool,

    /// Enable the optional web UI
    #[arg(long)]
    web: bool,

    /// Port for the web UI
    #[arg(long, default_value = "3000")]
    web_port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let is_interactive = cli.message.is_none() && !cli.non_interactive && !cli.web;

    // Create code formatter early so TUI can render input/output immediately
    let formatter = create_code_formatter()?;
    let _tui_guard = if is_interactive {
        Some(Arc::new(tui::init_tui_output(&formatter)?))
    } else {
        None
    };

    // Initialize logger
    output::init_logger(log::LevelFilter::Info);
    debug!("Starting AIxplosion");

    // Display large red warning if yolo mode is enabled
    if cli.yolo {
        display_yolo_warning();
    }

    // Load configuration
    let mut config = Config::load(cli.config.as_deref()).await?;

    // If provider is specified on command line, always apply its defaults
    // This overrides any custom base_url or model settings from the config file
    if let Some(provider) = cli.provider {
        config.set_provider(provider);
    }

    // Initialize database
    info!("Initializing database...");
    let db_path = get_database_path()?;
    let database_manager = DatabaseManager::new(db_path).await?;
    info!(
        "Database initialized at: {}",
        database_manager.path().display()
    );

    // Override API key if provided via command line (highest priority)
    if let Some(api_key) = cli.api_key {
        config.api_key = api_key;
    } else if config.api_key.is_empty() {
        // If no API key from config, try environment variable for the selected provider
        config.api_key = config::provider_default_api_key(config.provider);
    }

    let model = cli
        .model
        .clone()
        .unwrap_or_else(|| config.default_model.clone());

    app_println!("Using configuration:");
    app_println!("  Provider: {}", config.provider);
    app_println!("  Base URL: {}", config.base_url);
    app_println!("  Model: {}", model);

    // Show yolo mode status
    if cli.yolo {
        app_println!(
            "  {} YOLO MODE ENABLED - All permission checks bypassed!",
            "🔥".red().bold()
        );
    }

    // Validate API key without exposing it
    if config.api_key.is_empty() {
        let env_hint = match config.provider {
            Provider::Anthropic => "ANTHROPIC_AUTH_TOKEN",
            Provider::Gemini => "GEMINI_API_KEY or GOOGLE_API_KEY",
            Provider::OpenAI => "OPENAI_API_KEY",
            Provider::Zai => "ZAI_API_KEY",
        };
        app_eprintln!(
            "{}",
            format!(
                "Error: API key is required for {}. Set {} or use --api-key",
                config.provider, env_hint
            )
            .red()
        );
        app_eprintln!(
            "Create a config file at {} or set {}",
            Config::default_config_path().display(),
            env_hint
        );
        std::process::exit(1);
    } else {
        app_println!(
            "  API Key: {}",
            if config.api_key.len() > 10 {
                format!(
                    "{}... ({} chars)",
                    &config.api_key[..8],
                    config.api_key.len()
                )
            } else {
                "configured".to_string()
            }
        );
    }

    // Create and run agent using the new async constructor
    let mut agent =
        Agent::new_with_plan_mode(config.clone(), model.clone(), cli.yolo, cli.plan_mode).await;

    // Initialize MCP manager
    let mcp_manager = Arc::new(McpManager::new());

    // Initialize MCP manager with config from unified config
    mcp_manager.initialize(config.mcp.clone()).await?;

    // Set MCP manager in agent
    agent = agent.with_mcp_manager(mcp_manager.clone());

    // Set database manager in agent
    let database_manager = Arc::new(database_manager);
    agent = agent.with_database_manager(database_manager.clone());

    // Connect to all enabled MCP servers
    info!("Connecting to MCP servers...");
    let mcp_connect_result = tokio::time::timeout(
        std::time::Duration::from_secs(30), // 30 second timeout for MCP connections
        mcp_manager.connect_all_enabled(),
    )
    .await;

    match mcp_connect_result {
        Ok(Ok(_)) => {
            info!("MCP servers connected successfully");
        }
        Ok(Err(e)) => {
            warn!("Failed to connect to MCP servers: {}", e);
            error!("MCP Server Connection Issues:");
            error!("  - Check that MCP servers are configured correctly: /mcp list");
            error!("  - Verify server commands/URLs are valid");
            error!("  - Ensure all dependencies are installed");
            error!("  - Use '/mcp test <command>' to verify command availability");
            error!("  - Tool calls to unavailable MCP servers will fail");
        }
        Err(_) => {
            warn!("MCP server connection timed out after 30 seconds");
            error!("MCP Server Connection Timeout:");
            error!("  - MCP servers are taking too long to respond");
            error!("  - Check if servers are running and accessible");
            error!("  - Use '/mcp reconnect <server>' to try connecting manually");
        }
    }

    // Force initial refresh of MCP tools after connecting
    info!("Refreshing MCP tools...");
    let mcp_refresh_result = tokio::time::timeout(
        std::time::Duration::from_secs(15), // 15 second timeout for MCP tools refresh
        agent.force_refresh_mcp_tools(),
    )
    .await;

    match mcp_refresh_result {
        Ok(Ok(_)) => {
            info!("MCP tools loaded successfully");
        }
        Ok(Err(e)) => {
            warn!("Failed to refresh MCP tools on startup: {}", e);
            error!("MCP Tools Loading Failed:");
            error!("  - Connected MCP servers may not be responding properly");
            error!("  - Tools may have invalid schemas or descriptions");
            error!("  - Use '/mcp tools' to check available tools");
            error!("  - Use '/mcp reconnect <server>' to fix connection issues");
        }
        Err(_) => {
            warn!("MCP tools refresh timed out after 15 seconds");
            error!("MCP Tools Refresh Timeout:");
            error!("  - MCP servers are taking too long to provide tools");
            error!("  - Some tools may not be available initially");
            error!("  - Tools will be refreshed on demand during use");
        }
    }

    // Display YOLO mode warning after MCP configuration is complete
    if cli.yolo {
        display_mcp_yolo_warning();
    }

    // Set system prompt - use command line prompt if provided, otherwise use config default
    match &cli.system_prompt {
        Some(system_prompt) => {
            agent.set_system_prompt(system_prompt.clone());
            app_println!(
                "{} Using custom system prompt: {}",
                "✓".green(),
                system_prompt
            );
        }
        None => {
            // Use config's default system prompt if available
            if let Some(default_prompt) = &config.default_system_prompt {
                agent.set_system_prompt(default_prompt.clone());
                app_println!("{} Using default system prompt from config", "✓".green());
            }
        }
    }

    if cli.plan_mode {
        agent.apply_plan_mode_prompt();
        app_println!(
            "{} Plan mode enabled: generating read-only plans and saving them to the database.",
            "û".green()
        );
    }

    // Add context files
    add_context_files(&mut agent, &cli.context_files).await?;

    // Create initial conversation in database
    match agent.start_new_conversation().await {
        Ok(conversation_id) => {
            info!("Started initial conversation: {}", conversation_id);
        }
        Err(e) => {
            warn!("Failed to create initial conversation: {}", e);
        }
    }

    if cli.web {
        if cli.message.is_some() || cli.non_interactive {
            app_println!(
                "{} Ignoring -m/--message and --non-interactive flags because --web was supplied.",
                "?".yellow()
            );
        }

        let shared_agent = Arc::new(AsyncMutex::new(agent));
        let subagent_manager = Arc::new(AsyncMutex::new(subagent::SubagentManager::new()?));
        {
            let mut manager = subagent_manager.lock().await;
            manager.load_all_subagents().await?;
        }

        let state = web::WebState {
            agent: shared_agent,
            database: database_manager.clone(),
            mcp_manager: mcp_manager.clone(),
            subagent_manager,
            permission_hub: Arc::new(web::PermissionHub::new()),
        };

        web::launch_web_ui(state, cli.web_port).await?;
        return Ok(());
    }

    if let Some(message) = cli.message {
        // Display the message with file highlighting
        let highlighted_message = formatter.format_input_with_file_highlighting(&message);
        app_println!("> {}", highlighted_message);

        // Single message mode
        if cli.stream {
            let cancellation_flag = Arc::new(AtomicBool::new(false));
            let (streaming_state, stream_callback) = create_streaming_renderer(&formatter);
            let response = agent
                .process_message_with_stream(
                    &message,
                    Some(Arc::clone(&stream_callback)),
                    None,
                    cancellation_flag,
                )
                .await;
            if let Ok(mut renderer) = streaming_state.lock() {
                if let Err(e) = renderer.finish() {
                    app_eprintln!("{} Streaming formatter error: {}", "Error".red(), e);
                }
            }
            response?;
            print_usage_stats(&agent);
        } else {
            let cancellation_flag = Arc::new(AtomicBool::new(false));
            let spinner = create_spinner();
            let response = agent.process_message(&message, cancellation_flag).await?;
            spinner.finish_and_clear();
            formatter.print_formatted(&response)?;
            print_usage_stats(&agent);
        }
    } else if cli.non_interactive {
        // Read from stdin
        let mut input = String::new();
        io::stdin().read_to_string(&mut input)?;
        let trimmed_input = input.trim();

        // Display the input with file highlighting
        let highlighted_input = formatter.format_input_with_file_highlighting(trimmed_input);
        app_println!("> {}", highlighted_input);

        let cancellation_flag = Arc::new(AtomicBool::new(false));
        if cli.stream {
            let (streaming_state, stream_callback) = create_streaming_renderer(&formatter);
            let response = agent
                .process_message_with_stream(
                    trimmed_input,
                    Some(Arc::clone(&stream_callback)),
                    None,
                    cancellation_flag,
                )
                .await;
            if let Ok(mut renderer) = streaming_state.lock() {
                if let Err(e) = renderer.finish() {
                    app_eprintln!("{} Streaming formatter error: {}", "Error".red(), e);
                }
            }
            response?;
            print_usage_stats(&agent);
        } else {
            let spinner = create_spinner();
            let response = agent
                .process_message(trimmed_input, cancellation_flag)
                .await?;
            spinner.finish_and_clear();
            formatter.print_formatted(&response)?;
            print_usage_stats(&agent);
        }
    } else {
        if let Some(tui) = _tui_guard.as_ref() {
            run_tui_interactive(
                Arc::clone(tui),
                &mut agent,
                &mcp_manager,
                &formatter,
                cli.stream,
                cli.plan_mode,
            )
            .await?;
        } else {
            return Err(anyhow!("Interactive mode requires TUI initialization"));
        }
    }
    // Print final usage stats before exiting (only for interactive mode)
    if is_interactive {
        print_usage_stats(&agent);
    }

    // Disconnect from all MCP servers
    if let Err(e) = mcp_manager.disconnect_all().await {
        warn!("Failed to disconnect from MCP servers: {}", e);
    }

    // Close database connection
    database_manager.close().await;

    Ok(())
}
