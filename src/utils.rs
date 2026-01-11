use crate::agent::Agent;
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};

/// Print token usage statistics for the agent
pub fn print_usage_stats(agent: &Agent) {
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
pub fn create_spinner() -> ProgressBar {
    if crate::output::is_tui_active() {
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

/// Get the path to AGENTS.md in the user's home .flexorama directory
pub fn get_home_agents_md_path() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".flexorama")
        .join("AGENTS.md")
}
