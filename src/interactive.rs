use anyhow::Result;
use colored::*;
use log::debug;
use std::collections::VecDeque;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

use crate::agent::{self, Agent};
use crate::commands::{handle_shell_command, handle_slash_command};
use crate::formatter;
use crate::logo;
use crate::mcp::McpManager;
use crate::processing::process_input;
use crate::tui;
use crate::utils::{get_home_agents_md_path, print_usage_stats};

/// Run the TUI interactive mode
pub async fn run_tui_interactive(
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
    app_println!("{}", "💻 Flexorama - Interactive Mode".green().bold());
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
    tui.set_todos(&agent.get_todos().await)?;

    let todo_handle = agent.todos_handle();
    let tui_for_todos = Arc::clone(&tui);
    let on_tool_event: Arc<dyn Fn(agent::StreamToolEvent) + Send + Sync> =
        Arc::new(move |event: agent::StreamToolEvent| {
            if event.event != "tool_result" {
                return;
            }
            if event.name != "create_todo"
                && event.name != "complete_todo"
                && event.name != "list_todos"
            {
                return;
            }
            let todo_handle = Arc::clone(&todo_handle);
            let tui_for_todos = Arc::clone(&tui_for_todos);
            tokio::spawn(async move {
                let todos = {
                    let guard = todo_handle.lock().await;
                    guard.clone()
                };
                let _ = tui_for_todos.set_todos(&todos);
            });
        });

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
                    app_println!("\n{} Cancelling AI conversation...", "🛑".yellow());
                }
            }
            Ok(tui::InputResult::Exit) => {
                exit_for_input.store(true, Ordering::SeqCst);
                let cancel = { cancel_for_input.lock().expect("cancel lock").clone() };
                if let Some(flag) = cancel {
                    flag.store(true, Ordering::SeqCst);
                    app_println!("\n{} Cancelling and exiting...", "🛑".yellow());
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
            app_println!("{}", "Goodbye! 👋".green());
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
            Some(Arc::clone(&on_tool_event)),
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
pub async fn add_context_files(agent: &mut Agent, context_files: &[String]) -> Result<()> {
    // Always add AGENTS.md from ~/.flexorama/ if it exists (priority)
    let home_agents_md = get_home_agents_md_path();
    if home_agents_md.exists() {
        debug!("Auto-adding AGENTS.md from ~/.flexorama/ as context");
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
