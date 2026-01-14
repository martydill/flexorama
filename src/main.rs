use anyhow::{anyhow, Result};
use clap::Parser;
use colored::*;
use log::{debug, error, info, warn};
use std::collections::{HashMap, HashSet};
use std::io::{self, Read};
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::{Mutex as AsyncMutex, RwLock};

use flexorama::{app_eprintln, app_println};
use flexorama::*;

use agent::Agent;
use cli::Cli;
use config::{Config, Provider};
use csrf::CsrfManager;
use database::{get_database_path, DatabaseManager};
use formatter::create_code_formatter;
use help::{display_mcp_yolo_warning, display_yolo_warning};
use interactive::{add_context_files, run_tui_interactive};
use mcp::McpManager;
use processing::create_streaming_renderer;
use subagent::SubagentManager;
use utils::{create_spinner, print_usage_stats};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let is_interactive = cli.message.is_none() && !cli.non_interactive && !cli.web;
    let stream = !cli.no_stream;

    // Create code formatter early so TUI can render input/output immediately
    let formatter = create_code_formatter()?;
    let _tui_guard = if is_interactive {
        Some(Arc::new(tui::init_tui_output(&formatter)?))
    } else {
        None
    };

    // Initialize logger
    output::init_logger(log::LevelFilter::Info);
    debug!("Starting Flexorama");

    // Display large red warning if yolo mode is enabled
    if cli.yolo {
        display_yolo_warning();
    }

    // Load configuration
    let mut config = Config::load(cli.config.as_deref()).await?;

    // If provider is specified on command line, always apply its defaults
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
    if let Some(api_key) = cli.api_key.clone() {
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
    if config.api_key.is_empty() && config.provider != Provider::Ollama {
        let env_hint = match config.provider {
            Provider::Anthropic => "ANTHROPIC_AUTH_TOKEN",
            Provider::Gemini => "GEMINI_API_KEY or GOOGLE_API_KEY",
            Provider::OpenAI => "OPENAI_API_KEY",
            Provider::Zai => "ZAI_API_KEY",
            Provider::Ollama => "OLLAMA_API_KEY (optional for local instances)",
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
    mcp_manager.initialize(config.mcp.clone()).await?;
    agent = agent.with_mcp_manager(mcp_manager.clone());

    // Set database manager in agent
    let database_manager = Arc::new(database_manager);
    agent = agent.with_database_manager(database_manager.clone());

    // Initialize SkillManager
    info!("Initializing skill manager...");
    let config_arc = Arc::new(RwLock::new(config.clone()));
    let skill_manager = Arc::new(AsyncMutex::new(crate::skill::SkillManager::new(
        config_arc.clone(),
    )?));
    agent = agent.with_skill_manager(skill_manager.clone());

    let (deactivated, skill_names) = {
        let mut manager = skill_manager.lock().await;
        manager.load_all_skills().await?;

        // Activate all skills not explicitly deactivated in config
        let config_read = config_arc.read().await;
        let deactivated_skills = config_read.skills.deactivated_skills.clone();
        drop(config_read);

        let deactivated: HashSet<String> = deactivated_skills.into_iter().collect();
        let skill_names: Vec<String> = manager
            .list_skills()
            .iter()
            .map(|skill| skill.name.clone())
            .collect();

        (deactivated, skill_names)
    };

    for skill_name in skill_names {
        if deactivated.contains(&skill_name) {
            info!(
                "Skill '{}' is explicitly deactivated; skipping activation",
                skill_name
            );
            continue;
        }
        if let Err(e) = agent.activate_skill(&skill_name).await {
            warn!("Failed to activate skill '{}': {}", skill_name, e);
        } else {
            info!("Activated skill: {}", skill_name);
        }
    }

    info!("Skill manager initialized");

    // Connect to all enabled MCP servers
    info!("Connecting to MCP servers...");
    let mcp_connect_result = tokio::time::timeout(
        std::time::Duration::from_secs(30),
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
        std::time::Duration::from_secs(15),
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
            "✓".green()
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

    // Run the appropriate mode
    if cli.web {
        run_web_mode(
            cli,
            agent,
            database_manager.clone(),
            mcp_manager.clone(),
            skill_manager.clone(),
        )
        .await?;
    } else if let Some(message) = cli.message {
        run_single_message_mode(message, &mut agent, &formatter, stream).await?;
    } else if cli.non_interactive {
        run_non_interactive_mode(&mut agent, &formatter, stream).await?;
    } else {
        run_interactive_mode(
            _tui_guard,
            &mut agent,
            &mcp_manager,
            &formatter,
            stream,
            cli.plan_mode,
        )
        .await?;
    }

    // Cleanup
    if let Err(e) = mcp_manager.disconnect_all().await {
        warn!("Failed to disconnect from MCP servers: {}", e);
    }
    database_manager.close().await;

    Ok(())
}

/// Run web mode
async fn run_web_mode(
    cli: Cli,
    agent: Agent,
    database_manager: Arc<DatabaseManager>,
    mcp_manager: Arc<McpManager>,
    skill_manager: Arc<AsyncMutex<skill::SkillManager>>,
) -> Result<()> {
    if cli.message.is_some() || cli.non_interactive {
        app_println!(
            "{} Ignoring -m/--message and --non-interactive flags because --web was supplied.",
            "⚠".yellow()
        );
    }

    let shared_agent = Arc::new(AsyncMutex::new(agent));
    let subagent_manager = Arc::new(AsyncMutex::new(SubagentManager::new()?));
    {
        let mut manager = subagent_manager.lock().await;
        manager.load_all_subagents().await?;
    }

    let state = web::WebState {
        agent: shared_agent,
        database: database_manager,
        mcp_manager,
        subagent_manager,
        permission_hub: Arc::new(web::PermissionHub::new()),
        skill_manager,
        conversation_agents: Arc::new(AsyncMutex::new(HashMap::new())),
        csrf_manager: Arc::new(CsrfManager::new()),
    };

    web::launch_web_ui(state, cli.web_port).await?;
    Ok(())
}

/// Run single message mode
async fn run_single_message_mode(
    message: String,
    agent: &mut Agent,
    formatter: &formatter::CodeFormatter,
    stream: bool,
) -> Result<()> {
    let highlighted_message = formatter.format_input_with_file_highlighting(&message);
    app_println!("> {}", highlighted_message);

    let cancellation_flag = Arc::new(AtomicBool::new(false));

    if stream {
        let (streaming_state, stream_callback) = create_streaming_renderer(formatter);
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

    print_usage_stats(agent);
    Ok(())
}

/// Run non-interactive mode (read from stdin)
async fn run_non_interactive_mode(
    agent: &mut Agent,
    formatter: &formatter::CodeFormatter,
    stream: bool,
) -> Result<()> {
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    let trimmed_input = input.trim();

    let highlighted_input = formatter.format_input_with_file_highlighting(trimmed_input);
    app_println!("> {}", highlighted_input);

    let cancellation_flag = Arc::new(AtomicBool::new(false));

    if stream {
        let (streaming_state, stream_callback) = create_streaming_renderer(formatter);
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
    } else {
        let spinner = create_spinner();
        let response = agent
            .process_message(trimmed_input, cancellation_flag)
            .await?;
        spinner.finish_and_clear();
        formatter.print_formatted(&response)?;
    }

    print_usage_stats(agent);
    Ok(())
}

/// Run interactive mode
async fn run_interactive_mode(
    tui_guard: Option<Arc<tui::Tui>>,
    agent: &mut Agent,
    mcp_manager: &Arc<McpManager>,
    formatter: &formatter::CodeFormatter,
    stream: bool,
    plan_mode: bool,
) -> Result<()> {
    if let Some(tui) = tui_guard.as_ref() {
        run_tui_interactive(
            Arc::clone(tui),
            agent,
            mcp_manager,
            formatter,
            stream,
            plan_mode,
        )
        .await?;
        print_usage_stats(agent);
    } else {
        return Err(anyhow!("Interactive mode requires TUI initialization"));
    }
    Ok(())
}
