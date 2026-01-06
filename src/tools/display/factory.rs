use crate::tools::registry::{DisplayContext, OutputMode};
use serde_json::Value;
use std::time::Instant;

/// Factory for creating tool display instances
pub struct DisplayFactory;

impl DisplayFactory {
    /// Create the appropriate display for a tool based on context and environment
    pub fn create_display(
        tool_name: &str,
        arguments: &Value,
        registry: &crate::tools::registry::ToolRegistry,
    ) -> Box<dyn super::ToolDisplay> {
        let metadata = registry
            .get_metadata(tool_name)
            .cloned()
            .unwrap_or_else(|| {
                crate::tools::registry::ToolRegistry::get_default_metadata(tool_name)
            });

        let context = DisplayContext {
            tool_name: tool_name.to_string(),
            arguments: arguments.clone(),
            start_time: Instant::now(),
            metadata,
            output_mode: Self::detect_output_mode(),
        };

        match context.output_mode {
            OutputMode::Pretty => Box::new(super::pretty::PrettyDisplay::new(context)),
            OutputMode::Simple => Box::new(super::simple::SimpleDisplay::new(context)),
            OutputMode::Json => Box::new(super::json::JsonDisplay::new(context)),
        }
    }

    /// Detect the appropriate output mode based on environment
    fn detect_output_mode() -> OutputMode {
        // For now, always use pretty output
        // In the future, this could check for:
        // - TTY detection
        // - Environment variables
        // - Configuration settings
        // - Output redirection
        OutputMode::Pretty
    }

    /// Create a display with a specific output mode (for testing or forced modes)
    pub fn create_display_with_mode(
        tool_name: &str,
        arguments: &Value,
        registry: &crate::tools::registry::ToolRegistry,
        output_mode: OutputMode,
    ) -> Box<dyn super::ToolDisplay> {
        let metadata = registry
            .get_metadata(tool_name)
            .cloned()
            .unwrap_or_else(|| {
                crate::tools::registry::ToolRegistry::get_default_metadata(tool_name)
            });

        let context = DisplayContext {
            tool_name: tool_name.to_string(),
            arguments: arguments.clone(),
            start_time: Instant::now(),
            metadata,
            output_mode: output_mode.clone(),
        };

        match output_mode {
            OutputMode::Pretty => Box::new(super::pretty::PrettyDisplay::new(context)),
            OutputMode::Simple => Box::new(super::simple::SimpleDisplay::new(context)),
            OutputMode::Json => Box::new(super::json::JsonDisplay::new(context)),
        }
    }
}
