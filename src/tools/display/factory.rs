use crate::tools::registry::{DisplayContext, OutputMode};
use serde_json::Value;
use std::io::IsTerminal;
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
        }
    }

    /// Detect the appropriate output mode based on environment
    fn detect_output_mode() -> OutputMode {
        if crate::output::is_tui_active() {
            return OutputMode::Pretty;
        }

        if std::io::stdout().is_terminal() {
            OutputMode::Pretty
        } else {
            OutputMode::Simple
        }
    }
}
