use crate::tools::registry::{DisplayContext, DisplayFormat};
use colored::Colorize;
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

/// Pretty display for tool calls with boxes, colors, and detailed formatting
pub struct PrettyDisplay {
    context: DisplayContext,
}

impl PrettyDisplay {
    /// Create a new pretty display for the given context
    pub fn new(context: DisplayContext) -> Self {
        Self { context }
    }

    /// Get current time as formatted string
    fn get_current_time(&self) -> String {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(duration) => {
                let hours = (duration.as_secs() % 86400) / 3600;
                let minutes = (duration.as_secs() % 3600) / 60;
                let seconds = duration.as_secs() % 60;
                format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
            }
            Err(_) => "00:00:00".to_string(),
        }
    }

    /// Format tool call details based on display format
    fn format_tool_details(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let arguments = &self.context.arguments;

        match &self.context.metadata.display_format {
            DisplayFormat::File { show_size } => {
                if let Some(path) = arguments.get("path").and_then(|v| v.as_str()) {
                    lines.push(format!(
                        "{} {} {} {}",
                        "│".dimmed(),
                        "📄".yellow(),
                        "File:".bold(),
                        path.green()
                    ));

                    if *show_size {
                        // Try to get content size for write operations
                        if let Some(content) = arguments.get("content").and_then(|v| v.as_str()) {
                            lines.push(format!(
                                "{} {} {} {} bytes",
                                "│".dimmed(),
                                "📝".yellow(),
                                "Size:".bold(),
                                content.len().to_string().green()
                            ));
                        }

                        // For edit operations, show old/new sizes
                        if let Some(old_text) = arguments.get("old_text").and_then(|v| v.as_str()) {
                            lines.push(format!(
                                "{} {} {} {} bytes",
                                "│".dimmed(),
                                "📝".yellow(),
                                "Old:".bold(),
                                old_text.len().to_string().yellow()
                            ));
                        }
                        if let Some(new_text) = arguments.get("new_text").and_then(|v| v.as_str()) {
                            lines.push(format!(
                                "{} {} {} {} bytes",
                                "│".dimmed(),
                                "📝".yellow(),
                                "New:".bold(),
                                new_text.len().to_string().green()
                            ));
                        }
                    }
                }
            }
            DisplayFormat::Command { show_working_dir } => {
                if let Some(command) = arguments.get("command").and_then(|v| v.as_str()) {
                    lines.push(format!(
                        "{} {} {} {}",
                        "│".dimmed(),
                        "💻".yellow(),
                        "Command:".bold(),
                        command.green()
                    ));

                    if *show_working_dir {
                        // Add current working directory info
                        if let Ok(current_dir) = std::env::current_dir() {
                            lines.push(format!(
                                "{} {} {} {}",
                                "│".dimmed(),
                                "📍".yellow(),
                                "Working Dir:".bold(),
                                current_dir.display().to_string().blue()
                            ));
                        }
                    }
                }
            }
            DisplayFormat::Directory { show_item_count } => {
                if let Some(path) = arguments.get("path").and_then(|v| v.as_str()) {
                    lines.push(format!(
                        "{} {} {} {}",
                        "│".dimmed(),
                        "📍".yellow(),
                        "Path:".bold(),
                        path.green()
                    ));
                }
                // Item count would be shown after execution
            }
            DisplayFormat::Generic => {
                // Show all arguments for generic tools
                for (key, value) in arguments.as_object().unwrap_or(&serde_json::Map::new()) {
                    let value_str = if value.is_string() {
                        value.as_str().unwrap_or("").to_string()
                    } else {
                        serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
                    };

                    lines.push(format!(
                        "{} {} {} {}",
                        "│".dimmed(),
                        "⚙️".yellow(),
                        format!("{}:", key).bold(),
                        value_str.green()
                    ));
                }
            }
        }

        lines
    }

    /// Format the result content with appropriate truncation
    fn format_result_content(&self, content: &str, is_error: bool) -> Vec<String> {
        let mut lines = Vec::new();
        let content_lines: Vec<&str> = content.lines().collect();
        let total_lines = content_lines.len();
        let max_display_lines = 5;

        if total_lines == 0 {
            if is_error {
                lines.push(format!("│",).dimmed().to_string());
                lines.push(format!(
                    "{}   {}",
                    "│".dimmed(),
                    "[No error details]".dimmed()
                ));
            } else {
                lines.push(format!("│",).dimmed().to_string());
                lines.push(format!("{}   {}", "│".dimmed(), "[No output]".dimmed()));
            }
        } else {
            // Display limited lines
            let display_lines = if total_lines <= max_display_lines {
                total_lines
            } else {
                max_display_lines
            };

            if is_error {
                lines.push(format!("{} {}", "│".dimmed(), "Error:".red().bold()));
                for line in content_lines.iter().take(display_lines) {
                    lines.push(format!("{}   {}", "│".dimmed(), line.red()));
                }
            } else {
                lines.push(format!("{} {}", "│".dimmed(), "Output:".green().bold()));
                for line in content_lines.iter().take(display_lines) {
                    lines.push(format!("{}   {}", "│".dimmed(), line));
                }
            }

            // Show truncation indicator if content was limited
            if total_lines > max_display_lines {
                let remaining = total_lines - max_display_lines;
                lines.push(format!(
                    "{}   {}",
                    "│".dimmed(),
                    format!("[... {} more lines omitted]", remaining).dimmed()
                ));
            }
        }

        lines
    }
}

impl super::ToolDisplay for PrettyDisplay {
    fn show_call_details(&self, _arguments: &Value) {
        let icon = self.context.metadata.icon;

        app_println!(
            "{}",
            "┌─────────────────────────────────────────────────".dimmed()
        );
        app_println!(
            "{} {} {} {}",
            "│".dimmed(),
            icon,
            format!("Tool Call: {}", self.context.tool_name.cyan().bold()),
            format!("[{}]", self.get_current_time()).dimmed()
        );

        // Add formatted tool details
        for line in self.format_tool_details() {
            app_println!("{}", line);
        }

        app_println!(
            "{}",
            "└─────────────────────────────────────────────────".dimmed()
        );

        // Flush to ensure immediate display
        crate::output::flush();
    }

    fn complete_success(&mut self, result: &str) {
        let duration = self.context.start_time.elapsed();
        let icon = "✅";
        let status = "SUCCESS".green().bold();

        app_println!(
            "{}",
            "┌─────────────────────────────────────────────────".dimmed()
        );
        app_println!(
            "{} {} {} {} ({})",
            "│".dimmed(),
            icon,
            format!("Result: {}", self.context.tool_name.cyan().bold()),
            status,
            format!("{:.2}s", duration.as_secs_f64()).dimmed()
        );

        // Add formatted result content
        for line in self.format_result_content(result, false) {
            app_println!("{}", line);
        }

        app_println!(
            "{}",
            "└─────────────────────────────────────────────────".dimmed()
        );

        // Flush to ensure immediate display
        crate::output::flush();
    }

    fn complete_error(&mut self, error: &str) {
        let duration = self.context.start_time.elapsed();
        let icon = "❌";
        let status = "FAILED".red().bold();

        app_println!(
            "{}",
            "┌─────────────────────────────────────────────────".dimmed()
        );
        app_println!(
            "{} {} {} {} ({})",
            "│".dimmed(),
            icon,
            format!("Result: {}", self.context.tool_name.cyan().bold()),
            status,
            format!("{:.2}s", duration.as_secs_f64()).dimmed()
        );

        // Add formatted error content
        for line in self.format_result_content(error, true) {
            app_println!("{}", line);
        }

        app_println!(
            "{}",
            "└─────────────────────────────────────────────────".dimmed()
        );

        // Flush to ensure immediate display
        crate::output::flush();
    }
}


