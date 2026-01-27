use crate::agent::{self, Agent};
use crate::formatter;
use crate::utils::create_spinner;
use colored::*;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

/// Create a streaming renderer
pub fn create_streaming_renderer(
    formatter: &formatter::CodeFormatter,
) -> (
    Arc<Mutex<formatter::StreamingResponseFormatter>>,
    Arc<dyn Fn(String) + Send + Sync>,
) {
    let state = Arc::new(Mutex::new(formatter::StreamingResponseFormatter::new(
        formatter.clone(),
    )));
    let callback_state = Arc::clone(&state);
    let callback: Arc<dyn Fn(String) + Send + Sync> = Arc::new(move |content: String| {
        if content.is_empty() {
            return;
        }
        if let Ok(mut renderer) = callback_state.lock() {
            if let Err(e) = renderer.handle_chunk(&content) {
                app_eprintln!("{} Streaming formatter error: {}", "Error".red(), e);
            }
        }
    });
    (state, callback)
}

/// Process input and handle streaming/non-streaming response
pub async fn process_input(
    input: &str,
    agent: &mut Agent,
    formatter: &formatter::CodeFormatter,
    stream: bool,
    cancellation_flag: Arc<AtomicBool>,
    on_tool_event: Option<Arc<dyn Fn(agent::StreamToolEvent) + Send + Sync>>,
) {
    // Show spinner while processing (only for non-streaming)
    if stream {
        let (streaming_state, stream_callback) = create_streaming_renderer(formatter);
        let result = agent
            .process_message_with_stream(
                &input,
                Some(Arc::clone(&stream_callback)),
                on_tool_event,
                cancellation_flag.clone(),
            )
            .await;

        if let Ok(mut renderer) = streaming_state.lock() {
            if let Err(e) = renderer.finish() {
                app_eprintln!("{} Streaming formatter error: {}", "Error".red(), e);
            }
        }

        match result {
            Ok(_response) => {
                app_println!();
            }
            Err(e) => {
                if e.to_string().contains("CANCELLED") {
                    // Cancellation handled silently
                } else {
                    // Print newline first to ensure error appears on its own line
                    // (streaming output may not end with a newline)
                    app_println!();
                    app_eprintln!("{}: {}", "Error".red(), e);
                }
                app_println!();
            }
        }
    } else {
        let spinner = create_spinner();
        let result = agent
            .process_message_with_stream(&input, None, on_tool_event, cancellation_flag.clone())
            .await;
        spinner.finish_and_clear();

        match result {
            Ok(response) => {
                // Only print response if it's not empty (i.e., not just @file references)
                if !response.is_empty() {
                    if let Err(e) = formatter.print_formatted(&response) {
                        app_eprintln!("{} formatting response: {}", "Error".red(), e);
                    }
                }
                app_println!();
            }
            Err(e) => {
                if e.to_string().contains("CANCELLED") {
                    // Cancellation handled silently
                } else {
                    app_eprintln!("{}: {}", "Error".red(), e);
                }
                app_println!();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formatter::CodeFormatter;

    #[test]
    fn test_create_streaming_renderer() {
        let formatter = CodeFormatter::new().unwrap();
        let (_state, callback) = create_streaming_renderer(&formatter);

        // Test that callback can be called without panicking
        callback("test content".to_string());
        callback("".to_string()); // Empty content should be handled
    }

    #[test]
    fn test_create_streaming_renderer_with_multiple_chunks() {
        let formatter = CodeFormatter::new().unwrap();
        let (state, callback) = create_streaming_renderer(&formatter);

        // Send multiple chunks
        callback("Hello ".to_string());
        callback("World".to_string());
        callback("!".to_string());

        // Verify state is accessible
        assert!(state.lock().is_ok());
    }

    #[test]
    fn test_streaming_renderer_handles_empty_content() {
        let formatter = CodeFormatter::new().unwrap();
        let (_state, callback) = create_streaming_renderer(&formatter);

        // Should not panic with empty content
        callback("".to_string());
    }

    #[test]
    fn test_streaming_renderer_state_is_accessible() {
        let formatter = CodeFormatter::new().unwrap();
        let (state, _callback) = create_streaming_renderer(&formatter);

        // Verify we can lock the state
        let lock_result = state.lock();
        assert!(lock_result.is_ok());
    }
}
