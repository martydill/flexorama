use serde_json::Value;

/// Common trait for tool display implementations
pub trait ToolDisplay: Send {
    /// Show the tool call details (optional for simple display)
    fn show_call_details(&self, arguments: &Value) {
        let _ = arguments;
        // Default implementation does nothing for simple display
    }

    /// Complete the tool call with success
    fn complete_success(&mut self, result: &str);

    /// Complete the tool call with error
    fn complete_error(&mut self, error: &str);
}

pub mod factory;
pub mod json;
pub mod pretty;
pub mod simple;

// Re-export for convenience
pub use factory::DisplayFactory;
