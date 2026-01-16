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

    /// Format tool call parameters for the header line
    fn format_inline_params(&self) -> Option<String> {
        let mut parts = Vec::new();
        let arguments = &self.context.arguments;

        let truncate = |value: String| {
            let trimmed = value.replace("\n", " ").trim().to_string();
            if trimmed.len() > 60 {
                format!("{}...", &trimmed[..60])
            } else {
                trimmed
            }
        };

        match &self.context.metadata.display_format {
            DisplayFormat::File { show_size } => {
                if let Some(path) = arguments.get("path").and_then(|v| v.as_str()) {
                    parts.push(format!("path={}", truncate(path.to_string())));
                }
                if *show_size {
                    if let Some(content) = arguments.get("content").and_then(|v| v.as_str()) {
                        parts.push(format!("size={}", content.len()));
                    }
                    if let Some(old_text) = arguments.get("old_text").and_then(|v| v.as_str()) {
                        parts.push(format!("old_len={}", old_text.len()));
                    }
                    if let Some(new_text) = arguments.get("new_text").and_then(|v| v.as_str()) {
                        parts.push(format!("new_len={}", new_text.len()));
                    }
                }
            }
            DisplayFormat::Command => {
                if let Some(command) = arguments.get("command").and_then(|v| v.as_str()) {
                    parts.push(format!("cmd={}", truncate(command.to_string())));
                }
            }
            DisplayFormat::Directory => {
                if let Some(path) = arguments.get("path").and_then(|v| v.as_str()) {
                    parts.push(format!("path={}", truncate(path.to_string())));
                }
            }
            DisplayFormat::Generic => {
                if let Some(obj) = arguments.as_object() {
                    for (key, value) in obj.iter().take(3) {
                        let value_str = if value.is_string() {
                            value.as_str().unwrap_or("").to_string()
                        } else {
                            serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
                        };
                        parts.push(format!("{}={}", key, truncate(value_str)));
                    }
                }
            }
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" "))
        }
    }
}

impl super::ToolDisplay for PrettyDisplay {
    fn show_call_details(&self, _arguments: &Value) {
        let _ = _arguments;
        // Tool call details are shown with the result as a single combined output.
    }

    fn complete_success(&mut self, result: &str) {
        let duration = self.context.start_time.elapsed();
        let icon = self.context.metadata.icon;
        let status = "SUCCESS".green().bold();

        app_println!(
            "{}",
            "--------------------------------------------------".dimmed()
        );
        let call_summary = match self.format_inline_params() {
            Some(params) => format!(
                "Tool Call: {} {}",
                self.context.tool_name.cyan().bold(),
                params
            ),
            None => format!("Tool Call: {}", self.context.tool_name.cyan().bold()),
        };

        app_println!(
            "{} {} {} {}",
            "|".dimmed(),
            icon,
            call_summary,
            format!("[{}]", self.get_current_time()).dimmed()
        );

        app_println!(
            "{} {} {} {} ({})",
            "|".dimmed(),
            icon,
            format!("Result: {}", self.context.tool_name.cyan().bold()),
            status,
            format!("{:.2}s", duration.as_secs_f64()).dimmed()
        );

        let _ = result;

        app_println!(
            "{}",
            "--------------------------------------------------".dimmed()
        );

        // Flush to ensure immediate display
        crate::output::flush();
    }

    fn complete_error(&mut self, error: &str) {
        let duration = self.context.start_time.elapsed();
        let icon = self.context.metadata.icon;
        let status = "FAILED".red().bold();

        app_println!(
            "{}",
            "--------------------------------------------------".dimmed()
        );
        let call_summary = match self.format_inline_params() {
            Some(params) => format!(
                "Tool Call: {} {}",
                self.context.tool_name.cyan().bold(),
                params
            ),
            None => format!("Tool Call: {}", self.context.tool_name.cyan().bold()),
        };

        app_println!(
            "{} {} {} {}",
            "|".dimmed(),
            icon,
            call_summary,
            format!("[{}]", self.get_current_time()).dimmed()
        );

        app_println!(
            "{} {} {} {} ({})",
            "|".dimmed(),
            icon,
            format!("Result: {}", self.context.tool_name.cyan().bold()),
            status,
            format!("{:.2}s", duration.as_secs_f64()).dimmed()
        );

        let _ = error;

        app_println!(
            "{}",
            "--------------------------------------------------".dimmed()
        );

        // Flush to ensure immediate display
        crate::output::flush();
    }
}
