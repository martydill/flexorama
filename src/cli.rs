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

    /// LLM provider to use (anthropic, gemini, or z.ai)
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

    /// Enable the optional web UI
    #[arg(long)]
    pub web: bool,

    /// Port for the web UI
    #[arg(long, default_value = "3000")]
    pub web_port: u16,
}
