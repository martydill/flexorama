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
        Cancelled,
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
                let _ = input_tx.send(InputEvent::Cancelled);
            }
            Ok(tui::InputResult::Exit) => {
                let cancel = { cancel_for_input.lock().expect("cancel lock").clone() };
                if let Some(flag) = cancel {
                    flag.store(true, Ordering::SeqCst);
                    app_println!("\n{} Cancelling AI conversation...", "🛑".yellow());
                    let _ = input_tx.send(InputEvent::Cancelled);
                    continue;
                }
                exit_for_input.store(true, Ordering::SeqCst);
                app_println!("\n{} Exiting...", "🛑".yellow());
                let _ = input_tx.send(InputEvent::Exit);
                break;
            }
            Err(e) => {
                // On Windows, rapid clicking in the console can cause transient
                // crossterm errors. Log and continue rather than exiting.
                debug!("TUI input error (continuing): {}", e);
                continue;
            }
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
                Some(InputEvent::Cancelled) => continue,
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
        let clear_todos = {
            let processing_fut = process_input(
                &input,
                agent,
                formatter,
                stream,
                cancellation_flag_for_processing.clone(),
                Some(Arc::clone(&on_tool_event)),
            );
            tokio::pin!(processing_fut);
            let mut clear_todos = false;
            let mut processing_done = false;
            while !processing_done {
                tokio::select! {
                    _ = &mut processing_fut => {
                        processing_done = true;
                    }
                    event = input_rx.recv() => {
                        match event {
                            Some(InputEvent::Queued) => continue,
                            Some(InputEvent::Cancelled) => {
                                cancellation_flag_for_processing.store(true, Ordering::SeqCst);
                                clear_todos = true;
                                processing_done = true;
                            }
                            Some(InputEvent::Exit) => {
                                exit_requested.store(true, Ordering::SeqCst);
                                cancellation_flag_for_processing.store(true, Ordering::SeqCst);
                                clear_todos = true;
                                processing_done = true;
                            }
                            None => {
                                processing_done = true;
                            }
                        }
                    }
                }
            }
            clear_todos
        };
        if clear_todos {
            agent.clear_todos_for_current_conversation().await;
            let _ = tui.set_todos(&[]);
        }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use std::fs;
    use std::io::Write;
    use tempfile::TempDir;

    #[tokio::test]
    async fn test_add_context_files_empty_list() {
        let config = Config::default();
        let mut agent = crate::agent::Agent::new_with_plan_mode(
            config,
            "claude-3-5-sonnet-20241022".to_string(),
            false,
            false,
        )
        .await;
        let context_files: Vec<String> = vec![];

        let result = add_context_files(&mut agent, &context_files).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_add_context_files_with_nonexistent_file() {
        let config = Config::default();
        let mut agent = crate::agent::Agent::new_with_plan_mode(
            config,
            "claude-3-5-sonnet-20241022".to_string(),
            false,
            false,
        )
        .await;
        let context_files = vec!["nonexistent_file.txt".to_string()];

        // Should not panic even if file doesn't exist
        let result = add_context_files(&mut agent, &context_files).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_add_context_files_with_existing_file() {
        let config = Config::default();
        let mut agent = crate::agent::Agent::new_with_plan_mode(
            config,
            "claude-3-5-sonnet-20241022".to_string(),
            false,
            false,
        )
        .await;

        // Create a temporary file
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        let mut file = fs::File::create(&file_path).unwrap();
        writeln!(file, "Test content").unwrap();

        let context_files = vec![file_path.to_str().unwrap().to_string()];

        let result = add_context_files(&mut agent, &context_files).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_add_context_files_multiple_files() {
        let config = Config::default();
        let mut agent = crate::agent::Agent::new_with_plan_mode(
            config,
            "claude-3-5-sonnet-20241022".to_string(),
            false,
            false,
        )
        .await;

        // Create temporary files
        let temp_dir = TempDir::new().unwrap();
        let file1 = temp_dir.path().join("file1.txt");
        let file2 = temp_dir.path().join("file2.txt");

        fs::File::create(&file1).unwrap();
        fs::File::create(&file2).unwrap();

        let context_files = vec![
            file1.to_str().unwrap().to_string(),
            file2.to_str().unwrap().to_string(),
        ];

        let result = add_context_files(&mut agent, &context_files).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_home_agents_md_path_from_interactive() {
        let path = get_home_agents_md_path();
        assert!(path.to_string_lossy().contains(".flexorama"));
        assert!(path.to_string_lossy().ends_with("AGENTS.md"));
    }

    #[tokio::test]
    async fn test_add_context_files_with_temp_directory() {
        let config = Config::default();
        let mut agent = crate::agent::Agent::new_with_plan_mode(
            config,
            "claude-3-5-sonnet-20241022".to_string(),
            false,
            false,
        )
        .await;

        // Create a temporary directory with test files
        let temp_dir = TempDir::new().unwrap();
        let file1 = temp_dir.path().join("file1.txt");
        let file2 = temp_dir.path().join("file2.txt");
        let file3 = temp_dir.path().join("file3.txt");

        fs::File::create(&file1).unwrap();
        fs::File::create(&file2).unwrap();
        fs::File::create(&file3).unwrap();

        let context_files = vec![
            file1.to_str().unwrap().to_string(),
            file2.to_str().unwrap().to_string(),
            file3.to_str().unwrap().to_string(),
        ];

        let result = add_context_files(&mut agent, &context_files).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_add_context_files_mixed_existing_nonexisting() {
        let config = Config::default();
        let mut agent = crate::agent::Agent::new_with_plan_mode(
            config,
            "claude-3-5-sonnet-20241022".to_string(),
            false,
            false,
        )
        .await;

        let temp_dir = TempDir::new().unwrap();
        let existing_file = temp_dir.path().join("exists.txt");
        fs::File::create(&existing_file).unwrap();

        let context_files = vec![
            existing_file.to_str().unwrap().to_string(),
            "nonexistent1.txt".to_string(),
            "nonexistent2.txt".to_string(),
        ];

        // Should handle both existing and non-existing files gracefully
        let result = add_context_files(&mut agent, &context_files).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_add_context_files_with_special_characters() {
        let config = Config::default();
        let mut agent = crate::agent::Agent::new_with_plan_mode(
            config,
            "claude-3-5-sonnet-20241022".to_string(),
            false,
            false,
        )
        .await;

        let temp_dir = TempDir::new().unwrap();
        let file_with_spaces = temp_dir.path().join("file with spaces.txt");
        fs::File::create(&file_with_spaces).unwrap();

        let context_files = vec![file_with_spaces.to_str().unwrap().to_string()];

        let result = add_context_files(&mut agent, &context_files).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_add_context_files_large_number() {
        let config = Config::default();
        let mut agent = crate::agent::Agent::new_with_plan_mode(
            config,
            "claude-3-5-sonnet-20241022".to_string(),
            false,
            false,
        )
        .await;

        let temp_dir = TempDir::new().unwrap();
        let mut context_files = Vec::new();

        // Create 20 test files
        for i in 0..20 {
            let file = temp_dir.path().join(format!("file{}.txt", i));
            fs::File::create(&file).unwrap();
            context_files.push(file.to_str().unwrap().to_string());
        }

        let result = add_context_files(&mut agent, &context_files).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_add_context_files_with_content() {
        let config = Config::default();
        let mut agent = crate::agent::Agent::new_with_plan_mode(
            config,
            "claude-3-5-sonnet-20241022".to_string(),
            false,
            false,
        )
        .await;

        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("content.txt");
        let mut file = fs::File::create(&file_path).unwrap();
        writeln!(
            file,
            "This is test content\nWith multiple lines\nAnd more text"
        )
        .unwrap();

        let context_files = vec![file_path.to_str().unwrap().to_string()];

        let result = add_context_files(&mut agent, &context_files).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_add_context_files_empty_file() {
        let config = Config::default();
        let mut agent = crate::agent::Agent::new_with_plan_mode(
            config,
            "claude-3-5-sonnet-20241022".to_string(),
            false,
            false,
        )
        .await;

        let temp_dir = TempDir::new().unwrap();
        let empty_file = temp_dir.path().join("empty.txt");
        fs::File::create(&empty_file).unwrap();

        let context_files = vec![empty_file.to_str().unwrap().to_string()];

        let result = add_context_files(&mut agent, &context_files).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_add_context_files_relative_paths() {
        let config = Config::default();
        let mut agent = crate::agent::Agent::new_with_plan_mode(
            config,
            "claude-3-5-sonnet-20241022".to_string(),
            false,
            false,
        )
        .await;

        let context_files = vec![
            "./relative/path/file.txt".to_string(),
            "../parent/file.txt".to_string(),
        ];

        // Should handle gracefully even if paths don't exist
        let result = add_context_files(&mut agent, &context_files).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_add_context_files_absolute_paths() {
        let config = Config::default();
        let mut agent = crate::agent::Agent::new_with_plan_mode(
            config,
            "claude-3-5-sonnet-20241022".to_string(),
            false,
            false,
        )
        .await;

        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("absolute.txt");
        fs::File::create(&file).unwrap();

        let context_files = vec![file.to_str().unwrap().to_string()];

        let result = add_context_files(&mut agent, &context_files).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_add_context_files_duplicate_paths() {
        let config = Config::default();
        let mut agent = crate::agent::Agent::new_with_plan_mode(
            config,
            "claude-3-5-sonnet-20241022".to_string(),
            false,
            false,
        )
        .await;

        let temp_dir = TempDir::new().unwrap();
        let file = temp_dir.path().join("duplicate.txt");
        fs::File::create(&file).unwrap();

        let file_str = file.to_str().unwrap().to_string();
        let context_files = vec![file_str.clone(), file_str.clone(), file_str];

        // Should handle duplicate paths gracefully
        let result = add_context_files(&mut agent, &context_files).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_add_context_files_with_subdirectories() {
        let config = Config::default();
        let mut agent = crate::agent::Agent::new_with_plan_mode(
            config,
            "claude-3-5-sonnet-20241022".to_string(),
            false,
            false,
        )
        .await;

        let temp_dir = TempDir::new().unwrap();
        let subdir = temp_dir.path().join("subdir");
        fs::create_dir(&subdir).unwrap();
        let file = subdir.join("file.txt");
        fs::File::create(&file).unwrap();

        let context_files = vec![file.to_str().unwrap().to_string()];

        let result = add_context_files(&mut agent, &context_files).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_add_context_files_binary_file() {
        let config = Config::default();
        let mut agent = crate::agent::Agent::new_with_plan_mode(
            config,
            "claude-3-5-sonnet-20241022".to_string(),
            false,
            false,
        )
        .await;

        let temp_dir = TempDir::new().unwrap();
        let binary_file = temp_dir.path().join("binary.bin");
        let mut file = fs::File::create(&binary_file).unwrap();
        // Write some binary data
        file.write_all(&[0u8, 1, 2, 3, 4, 255, 254, 253]).unwrap();

        let context_files = vec![binary_file.to_str().unwrap().to_string()];

        // Should handle binary files gracefully
        let result = add_context_files(&mut agent, &context_files).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_home_agents_md_path_consistency() {
        let path1 = get_home_agents_md_path();
        let path2 = get_home_agents_md_path();
        assert_eq!(path1, path2);
    }

    #[test]
    fn test_get_home_agents_md_path_is_absolute() {
        let path = get_home_agents_md_path();
        assert!(path.is_absolute());
    }

    #[tokio::test]
    async fn test_add_context_files_unicode_content() {
        let config = Config::default();
        let mut agent = crate::agent::Agent::new_with_plan_mode(
            config,
            "claude-3-5-sonnet-20241022".to_string(),
            false,
            false,
        )
        .await;

        let temp_dir = TempDir::new().unwrap();
        let unicode_file = temp_dir.path().join("unicode.txt");
        let mut file = fs::File::create(&unicode_file).unwrap();
        writeln!(file, "Hello 世界 مرحبا Привет").unwrap();

        let context_files = vec![unicode_file.to_str().unwrap().to_string()];

        let result = add_context_files(&mut agent, &context_files).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_add_context_files_very_long_path() {
        let config = Config::default();
        let mut agent = crate::agent::Agent::new_with_plan_mode(
            config,
            "claude-3-5-sonnet-20241022".to_string(),
            false,
            false,
        )
        .await;

        // Create a path with many nested directories
        let temp_dir = TempDir::new().unwrap();
        let mut long_path = temp_dir.path().to_path_buf();
        for i in 0..10 {
            long_path = long_path.join(format!("dir{}", i));
        }
        fs::create_dir_all(&long_path).unwrap();
        let file = long_path.join("file.txt");
        fs::File::create(&file).unwrap();

        let context_files = vec![file.to_str().unwrap().to_string()];

        let result = add_context_files(&mut agent, &context_files).await;
        assert!(result.is_ok());
    }
}
