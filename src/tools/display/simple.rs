use crate::tools::registry::{DisplayContext, DisplayFormat};
use serde_json::Value;

/// Simple text-only display for non-interactive environments
pub struct SimpleDisplay {
    context: DisplayContext,
}

impl SimpleDisplay {
    /// Create a new simple display for the given context
    pub fn new(context: DisplayContext) -> Self {
        app_println!("▶ {} {}...", context.metadata.icon, context.tool_name);
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

    /// Format result content with truncation
    fn format_result_content(&self, content: &str) -> Vec<String> {
        let mut lines = Vec::new();
        let content_lines: Vec<&str> = content.lines().collect();
        let total_lines = content_lines.len();
        let max_display_lines = 5;

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
        // Show tool details in simple format
        for line in self.format_tool_details() {
            app_println!("{}", line);
        }
    }

    fn complete_success(&mut self, result: &str) {
        let duration = self.context.start_time.elapsed();
        app_println!("✅ {} completed in {:?}", self.context.tool_name, duration);

        // Show limited result
        if !result.is_empty() {
            for line in self.format_result_content(result) {
                app_println!("{}", line);
            }
        }
    }

    fn complete_error(&mut self, error: &str) {
        let duration = self.context.start_time.elapsed();
        app_println!("❌ {} failed in {:?}", self.context.tool_name, duration);

        app_println!("   Error:");
        // Show limited error
        for line in self.format_result_content(error) {
            app_println!("{}", line);
        }
    }
}


