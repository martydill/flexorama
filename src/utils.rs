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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    #[test]
    fn test_get_home_agents_md_path() {
        let path = get_home_agents_md_path();
        assert!(path.to_string_lossy().contains(".flexorama"));
        assert!(path.to_string_lossy().ends_with("AGENTS.md"));
    }

    #[test]
    fn test_get_home_agents_md_path_is_absolute() {
        let path = get_home_agents_md_path();
        assert!(path.is_absolute() || path.starts_with("."));
    }

    #[test]
    fn test_create_spinner_returns_valid_spinner() {
        let spinner = create_spinner();
        assert!(!spinner.is_finished());
    }

    #[test]
    fn test_spinner_can_be_finished() {
        let spinner = create_spinner();
        spinner.finish_and_clear();
        assert!(spinner.is_finished());
    }

    #[tokio::test]
    async fn test_print_usage_stats_with_zero_usage() {
        let config = Config::default();
        let agent = crate::agent::Agent::new_with_plan_mode(
            config,
            "claude-3-5-sonnet-20241022".to_string(),
            false,
            false
        ).await;

        // This should not panic with zero usage
        print_usage_stats(&agent);

        let usage = agent.get_token_usage();
        assert_eq!(usage.request_count, 0);
        assert_eq!(usage.total_input_tokens, 0);
        assert_eq!(usage.total_output_tokens, 0);
    }
}
