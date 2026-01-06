use crate::tools::registry::{DisplayContext, DisplayFormat};
use serde_json::json;
use serde_json::Value;

/// JSON display for tool calls - outputs structured data
pub struct JsonDisplay {
    context: DisplayContext,
}

impl JsonDisplay {
    /// Create a new JSON display for the given context
    pub fn new(context: DisplayContext) -> Self {
        Self { context }
    }

    /// Convert tool call to JSON format
    fn tool_call_to_json(&self, arguments: &Value) -> Value {
        let mut result = json!({
            "type": "tool_call",
            "tool": {
                "name": self.context.tool_name,
                "icon": self.context.metadata.icon,
                "description": self.context.metadata.description,
                "display_format": match &self.context.metadata.display_format {
                    DisplayFormat::File { show_size } => json!({
                        "type": "file",
                        "show_size": show_size
                    }),
                    DisplayFormat::Command { show_working_dir } => json!({
                        "type": "command",
                        "show_working_dir": show_working_dir
                    }),
                    DisplayFormat::Directory { show_item_count } => json!({
                        "type": "directory",
                        "show_item_count": show_item_count
                    }),
                    DisplayFormat::Generic => json!({
                        "type": "generic"
                    }),
                }
            },
            "arguments": arguments,
            "timestamp": {
                "start": self.context.start_time.elapsed().as_secs_f64()
            }
        });

        // Add specific argument formatting based on tool type
        if let Some(formatted_args) = self.format_specific_arguments() {
            result["formatted_arguments"] = formatted_args;
        }

        result
    }

    /// Format specific arguments based on tool type
    fn format_specific_arguments(&self) -> Option<Value> {
        let arguments = &self.context.arguments;

        match &self.context.metadata.display_format {
            DisplayFormat::File { show_size: _ } => {
                if let Some(path) = arguments.get("path").and_then(|v| v.as_str()) {
                    let mut formatted = json!({
                        "path": path
                    });

                    if let Some(content) = arguments.get("content").and_then(|v| v.as_str()) {
                        formatted["content_size"] = json!(content.len());
                    }

                    if let Some(old_text) = arguments.get("old_text").and_then(|v| v.as_str()) {
                        formatted["old_text_size"] = json!(old_text.len());
                    }

                    if let Some(new_text) = arguments.get("new_text").and_then(|v| v.as_str()) {
                        formatted["new_text_size"] = json!(new_text.len());
                    }

                    Some(formatted)
                } else {
                    None
                }
            }
            DisplayFormat::Command { show_working_dir } => {
                let mut formatted = json!({});

                if let Some(command) = arguments.get("command").and_then(|v| v.as_str()) {
                    formatted["command"] = json!(command);
                }

                if *show_working_dir {
                    if let Ok(current_dir) = std::env::current_dir() {
                        formatted["working_directory"] = json!(current_dir.to_string_lossy());
                    }
                }

                Some(formatted)
            }
            DisplayFormat::Directory { show_item_count: _ } => {
                if let Some(path) = arguments.get("path").and_then(|v| v.as_str()) {
                    Some(json!({
                        "path": path
                    }))
                } else {
                    None
                }
            }
            DisplayFormat::Generic => {
                // For generic tools, just return the arguments as-is
                Some(arguments.clone())
            }
        }
    }

    /// Convert result to JSON format
    fn result_to_json(&self, content: &str, is_error: bool) -> Value {
        let duration = self.context.start_time.elapsed();

        json!({
            "type": "tool_result",
            "tool": {
                "name": self.context.tool_name,
                "icon": self.context.metadata.icon
            },
            "success": !is_error,
            "duration_seconds": duration.as_secs_f64(),
            "content": content,
            "content_size": content.len(),
            "content_lines": content.lines().count(),
            "truncated": content.lines().count() > 3,
            "timestamp": {
                "end": duration.as_secs_f64()
            }
        })
    }

    /// Convert tool call and result to a single JSON format
    fn combined_json(&self, content: &str, is_error: bool) -> Value {
        let mut output = self.tool_call_to_json(&self.context.arguments);
        output["type"] = json!("tool_event");
        output["result"] = self.result_to_json(content, is_error);
        output
    }
}

impl super::ToolDisplay for JsonDisplay {
    fn show_call_details(&self, _arguments: &Value) {
        let _ = _arguments;
        // Tool call details are shown with the result as a single combined output.
    }

    fn complete_success(&mut self, result: &str) {
        let json_output = self.combined_json(result, false);
        app_println!("{}", serde_json::to_string_pretty(&json_output).unwrap());
    }

    fn complete_error(&mut self, error: &str) {
        let json_output = self.combined_json(error, true);
        app_println!("{}", serde_json::to_string_pretty(&json_output).unwrap());
    }
}
