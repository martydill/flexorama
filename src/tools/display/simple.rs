use crate::tools::registry::{DisplayContext, DisplayFormat};
use serde_json::Value;

/// Simple text-only display for non-interactive environments
pub struct SimpleDisplay {
    context: DisplayContext,
}

impl SimpleDisplay {
    /// Create a new simple display for the given context
    pub fn new(context: DisplayContext) -> Self {
        Self { context }
    }

    /// Format tool call details in simple format
    fn format_tool_details(&self) -> Vec<String> {
        let mut lines = Vec::new();
        let arguments = &self.context.arguments;

        match &self.context.metadata.display_format {
            DisplayFormat::File { show_size: _ } => {
                if let Some(path) = arguments.get("path").and_then(|v| v.as_str()) {
                    lines.push(format!("  File: {}", path));
                }
            }
            DisplayFormat::Command { show_working_dir } => {
                if let Some(command) = arguments.get("command").and_then(|v| v.as_str()) {
                    lines.push(format!("  Command: {}", command));
                    if *show_working_dir {
                        if let Ok(current_dir) = std::env::current_dir() {
                            lines.push(format!("  Working Dir: {}", current_dir.display()));
                        }
                    }
                }
            }
            DisplayFormat::Directory { show_item_count: _ } => {
                if let Some(path) = arguments.get("path").and_then(|v| v.as_str()) {
                    lines.push(format!("  Path: {}", path));
                }
            }
            DisplayFormat::Generic => {
                // Show all arguments for generic tools
                for (key, value) in arguments.as_object().unwrap_or(&serde_json::Map::new()) {
                    let value_str = if value.is_string() {
                        value.as_str().unwrap_or("").to_string()
                    } else {
                        serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
                    };
                    lines.push(format!("  {}: {}", key, value_str));
                }
            }
        }

        lines
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
            DisplayFormat::Command { show_working_dir: _ } => {
                if let Some(command) = arguments.get("command").and_then(|v| v.as_str()) {
                    parts.push(format!("cmd={}", truncate(command.to_string())));
                }
            }
            DisplayFormat::Directory { show_item_count: _ } => {
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

    /// Format result content with truncation
    fn format_result_content(&self, content: &str) -> Vec<String> {
        let mut lines = Vec::new();
        let content_lines: Vec<&str> = content.lines().collect();
        let total_lines = content_lines.len();
        let max_display_lines = 3;

        if total_lines <= max_display_lines {
            // Show all lines if within limit
            for line in content_lines {
                lines.push(format!("   {}", line));
            }
        } else {
            // Show first 5 lines and indicate truncation
            for line in content_lines.iter().take(max_display_lines) {
                lines.push(format!("   {}", line));
            }
            let remaining = total_lines - max_display_lines;
            lines.push(format!(
                "   [... {} more lines omitted] [{} bytes total]",
                remaining,
                content.len()
            ));
        }

        lines
    }
}

impl super::ToolDisplay for SimpleDisplay {
    fn show_call_details(&self, _arguments: &Value) {
        let _ = _arguments;
        // Tool call details are shown with the result as a single combined output.
    }

    fn complete_success(&mut self, result: &str) {
        let duration = self.context.start_time.elapsed();
        let call_summary = match self.format_inline_params() {
            Some(params) => format!("{} {}", self.context.tool_name, params),
            None => self.context.tool_name.clone(),
        };

        app_println!(
            "? {} {} SUCCESS ({:.2}s)",
            self.context.metadata.icon,
            call_summary,
            duration.as_secs_f64()
        );

        app_println!("  Output:");
        if result.is_empty() {
            app_println!("   [No output]");
        } else {
            for line in self.format_result_content(result) {
                app_println!("{}", line);
            }
        }
    }

    fn complete_error(&mut self, error: &str) {
        let duration = self.context.start_time.elapsed();
        let call_summary = match self.format_inline_params() {
            Some(params) => format!("{} {}", self.context.tool_name, params),
            None => self.context.tool_name.clone(),
        };

        app_println!(
            "? {} {} FAILED ({:.2}s)",
            self.context.metadata.icon,
            call_summary,
            duration.as_secs_f64()
        );

        app_println!("  Error:");
        if error.is_empty() {
            app_println!("   [No error details]");
        } else {
            for line in self.format_result_content(error) {
                app_println!("{}", line);
            }
        }
    }
}

