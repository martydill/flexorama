use clap::Parser;

/// Flexorama CLI - An AI-powered agent for code and task automation
#[derive(Parser, Debug)]
#[clap(name = "flexorama")]
#[clap(about = "AI-powered agent for code and task automation", long_about = None)]
pub struct Cli {
    /// The message to send to the agent
    #[arg(short = 'm', long)]
    pub message: Option<String>,

    /// Set the API key (overrides config file)
    #[arg(short = 'k', long)]
    pub api_key: Option<String>,

    /// LLM provider to use (anthropic, gemini, openai, openrouter, z.ai, or ollama)
    #[arg(long)]
    pub provider: Option<crate::config::Provider>,

    /// Specify the model to use
    #[arg(long)]
    pub model: Option<String>,

    /// Configuration file path
    #[arg(short, long)]
    pub config: Option<String>,

    /// Run in non-interactive mode
    #[arg(short, long)]
    pub non_interactive: bool,

    /// Files to include as context
    #[arg(short = 'f', long = "file", value_name = "FILE")]
    pub context_files: Vec<String>,

    /// System prompt to use for the conversation
    #[arg(short = 's', long = "system", value_name = "PROMPT")]
    pub system_prompt: Option<String>,

    /// Disable streaming responses
    #[arg(long = "no-stream")]
    pub no_stream: bool,

    /// Enable 'yolo' mode - bypass all permission checks for file and tool operations
    #[arg(long)]
    pub yolo: bool,

    /// Enable plan-only mode (generate a plan in Markdown without making changes)
    #[arg(long = "plan-mode")]
    pub plan_mode: bool,

    /// Enable ACP (Agent Client Protocol) server mode
    #[arg(long)]
    pub acp: bool,

    /// Enable debug logging for ACP messages
    #[arg(long)]
    pub acp_debug: bool,

    /// Enable the optional web UI
    #[arg(long)]
    pub web: bool,

    /// Port for the web UI
    #[arg(long, default_value = "3000")]
    pub web_port: u16,

    /// Resume the most recent conversation
    #[arg(long = "continue", conflicts_with = "message")]
    pub continue_session: bool,

    /// Resume a specific conversation by ID
    #[arg(long = "resume", value_name = "ID", conflicts_with = "continue_session")]
    pub resume_conversation: Option<String>,

    /// Run the interactive setup wizard
    #[arg(long)]
    pub setup: bool,

    /// Enable verbose logging (shows info/debug messages during startup)
    #[arg(short, long)]
    pub verbose: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn test_cli_default_values() {
        let cli = Cli::try_parse_from(vec!["flexorama"]).unwrap();

        assert_eq!(cli.message, None);
        assert_eq!(cli.api_key, None);
        assert_eq!(cli.provider, None);
        assert_eq!(cli.model, None);
        assert_eq!(cli.config, None);
        assert!(!cli.non_interactive);
        assert!(cli.context_files.is_empty());
        assert_eq!(cli.system_prompt, None);
        assert!(!cli.no_stream);
        assert!(!cli.setup);
        assert!(!cli.verbose);
        assert!(!cli.yolo);
        assert!(!cli.plan_mode);
        assert!(!cli.web);
        assert_eq!(cli.web_port, 3000);
        assert!(!cli.continue_session);
        assert!(cli.resume_conversation.is_none());
    }

    #[test]
    fn test_cli_with_message() {
        let cli = Cli::try_parse_from(vec!["flexorama", "-m", "Hello, world!"]).unwrap();
        assert_eq!(cli.message, Some("Hello, world!".to_string()));
    }

    #[test]
    fn test_cli_with_api_key() {
        let cli = Cli::try_parse_from(vec!["flexorama", "-k", "test-key"]).unwrap();
        assert_eq!(cli.api_key, Some("test-key".to_string()));
    }

    #[test]
    fn test_cli_with_context_files() {
        let cli =
            Cli::try_parse_from(vec!["flexorama", "-f", "file1.txt", "-f", "file2.txt"]).unwrap();
        assert_eq!(cli.context_files.len(), 2);
        assert_eq!(cli.context_files[0], "file1.txt");
        assert_eq!(cli.context_files[1], "file2.txt");
    }

    #[test]
    fn test_cli_with_system_prompt() {
        let cli =
            Cli::try_parse_from(vec!["flexorama", "-s", "You are a helpful assistant"]).unwrap();
        assert_eq!(
            cli.system_prompt,
            Some("You are a helpful assistant".to_string())
        );
    }

    #[test]
    fn test_cli_flags() {
        let cli = Cli::try_parse_from(vec![
            "flexorama",
            "--non-interactive",
            "--no-stream",
            "--yolo",
            "--plan-mode",
        ])
        .unwrap();

        assert!(cli.non_interactive);
        assert!(cli.no_stream);
        assert!(cli.yolo);
        assert!(cli.plan_mode);
    }

    #[test]
    fn test_cli_web_mode() {
        let cli = Cli::try_parse_from(vec!["flexorama", "--web", "--web-port", "8080"]).unwrap();

        assert!(cli.web);
        assert_eq!(cli.web_port, 8080);
    }

    #[test]
    fn test_cli_with_config_file() {
        let cli = Cli::try_parse_from(vec!["flexorama", "-c", "/path/to/config.toml"]).unwrap();
        assert_eq!(cli.config, Some("/path/to/config.toml".to_string()));
    }

    #[test]
    fn test_cli_with_model() {
        let cli = Cli::try_parse_from(vec!["flexorama", "--model", "gpt-4"]).unwrap();
        assert_eq!(cli.model, Some("gpt-4".to_string()));
    }

    #[test]
    fn test_cli_continue() {
        let cli = Cli::try_parse_from(vec!["flexorama", "--continue"]).unwrap();

        assert!(cli.continue_session);
        assert!(!cli.web);
        assert_eq!(cli.web_port, 3000);
        assert!(cli.resume_conversation.is_none());
    }

    #[test]
    fn test_cli_resume() {
        let cli = Cli::try_parse_from(vec!["flexorama", "--resume", "conv-123"]).unwrap();

        assert_eq!(cli.resume_conversation, Some("conv-123".to_string()));
        assert!(!cli.continue_session);
    }

    #[test]
    fn test_cli_continue_conflicts_with_message() {
        let cli = Cli::try_parse_from(vec!["flexorama", "--continue", "-m", "test"]);
        assert!(cli.is_err(), "Should conflict with message");
    }

    #[test]
    fn test_cli_with_setup() {
        let cli = Cli::try_parse_from(vec!["flexorama", "--setup"]).unwrap();
        assert!(cli.setup);
        assert_eq!(cli.message, None);
        assert_eq!(cli.api_key, None);
    }

    #[test]
    fn test_cli_setup_with_message() {
        let cli = Cli::try_parse_from(vec!["flexorama", "--setup", "-m", "test"]);
        assert!(cli.is_ok(), "Setup and message can coexist");
        let cli = cli.unwrap();
        assert!(cli.setup);
        assert_eq!(cli.message, Some("test".to_string()));
    }

    #[test]
    fn test_cli_continue_resumes_most_recent() {
        let cli = Cli::try_parse_from(vec!["flexorama", "--continue"]).unwrap();
        assert!(cli.continue_session);
        assert!(cli.resume_conversation.is_none());
    }

    #[test]
    fn test_cli_resume_conflicts_with_continue() {
        let cli = Cli::try_parse_from(vec!["flexorama", "--continue", "--resume", "id"]);
        assert!(cli.is_err(), "Should conflict with resume");
    }

    #[test]
    fn test_cli_verify_app() {
        // This ensures the CLI definition is valid
        Cli::command().debug_assert();
    }

    #[test]
    fn test_cli_long_and_short_flags() {
        let cli_short = Cli::try_parse_from(vec![
            "flexorama",
            "-m",
            "test",
            "-k",
            "key",
            "-f",
            "file.txt",
            "-s",
            "prompt",
            "-c",
            "config.toml",
        ])
        .unwrap();

        let cli_long = Cli::try_parse_from(vec![
            "flexorama",
            "--message",
            "test",
            "--api-key",
            "key",
            "--file",
            "file.txt",
            "--system",
            "prompt",
            "--config",
            "config.toml",
        ])
        .unwrap();

        assert_eq!(cli_short.message, cli_long.message);
        assert_eq!(cli_short.api_key, cli_long.api_key);
        assert_eq!(cli_short.context_files, cli_long.context_files);
        assert_eq!(cli_short.system_prompt, cli_long.system_prompt);
        assert_eq!(cli_short.config, cli_long.config);
    }
}
