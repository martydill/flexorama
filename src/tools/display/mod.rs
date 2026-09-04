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
pub mod pretty;
pub mod simple;

// Re-export for convenience
pub use factory::DisplayFactory;

#[cfg(test)]
mod tests {
    use super::factory::DisplayFactory;
    use super::pretty::PrettyDisplay;
    use super::simple::SimpleDisplay;
    use super::ToolDisplay as _;
    use crate::tools::registry::{
        DisplayContext, DisplayFormat, OutputMode, ToolMetadata, ToolRegistry,
    };
    use serial_test::serial;
    use serde_json::json;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct RecordingSink {
        lines: Mutex<Vec<(String, bool)>>,
    }

    impl crate::output::OutputSink for RecordingSink {
        fn write(&self, text: &str, is_err: bool) {
            self.lines
                .lock()
                .expect("lines lock")
                .push((text.to_string(), is_err));
        }

        fn flush(&self) {}
    }

    impl RecordingSink {
        fn stdout_text(&self) -> String {
            self.lines
                .lock()
                .expect("lines lock")
                .iter()
                .filter(|(_, is_err)| !*is_err)
                .map(|(text, _)| text.as_str())
                .collect::<Vec<_>>()
                .join("")
        }

        fn stderr_text(&self) -> String {
            self.lines
                .lock()
                .expect("lines lock")
                .iter()
                .filter(|(_, is_err)| *is_err)
                .map(|(text, _)| text.as_str())
                .collect::<Vec<_>>()
                .join("")
        }
    }

    /// Capture whatever a display prints through the global output sink.
    fn with_sink<F: FnOnce()>(f: F) -> (String, String) {
        colored::control::set_override(false);
        let sink = Arc::new(RecordingSink::default());
        crate::output::set_output_sink(sink.clone());
        f();
        crate::output::clear_output_sink();
        colored::control::unset_override();
        (sink.stdout_text(), sink.stderr_text())
    }

    fn context(name: &str, format: DisplayFormat, arguments: serde_json::Value) -> DisplayContext {
        DisplayContext {
            tool_name: name.to_string(),
            arguments,
            start_time: std::time::Instant::now(),
            metadata: ToolMetadata {
                name: name.to_string(),
                icon: "🧪",
                display_format: format,
                readonly: true,
            },
            output_mode: OutputMode::Simple,
        }
    }

    #[test]
    #[serial]
    fn factory_prefers_pretty_mode_when_tui_is_active() {
        let (out, _) = with_sink(|| {
            // A registered output sink means the TUI is active, which forces Pretty.
            let registry = ToolRegistry::with_builtin_tools();
            let mut display = DisplayFactory::create_display(
                "Read",
                &json!({ "path": "/tmp/x.txt" }),
                &registry,
            );
            display.complete_success("file body");
        });

        assert!(out.contains("Tool Call:"), "pretty output expected: {}", out);
        assert!(out.contains("SUCCESS"));
        assert!(out.contains("file body"));
    }

    #[test]
    #[serial]
    fn factory_falls_back_to_default_metadata_for_unknown_tools() {
        let (out, _) = with_sink(|| {
            let registry = ToolRegistry::new();
            let mut display = DisplayFactory::create_display(
                "mystery_tool",
                &json!({ "path": "/tmp/x.txt" }),
                &registry,
            );
            display.complete_success("done");
        });

        assert!(out.contains("mystery_tool"), "output: {}", out);
    }

    #[test]
    #[serial]
    fn simple_display_reports_success_with_first_result_line() {
        let (out, _) = with_sink(|| {
            let mut display = SimpleDisplay::new(context(
                "Read",
                DisplayFormat::File { show_size: false },
                json!({ "path": "/tmp/x.txt" }),
            ));
            display.complete_success("line one\nline two");
        });

        assert!(out.contains("[tool] Read ok"), "output: {}", out);
        assert!(out.contains("line one"));
        assert!(!out.contains("line two"), "non-edit tools show first line only");
    }

    #[test]
    #[serial]
    fn simple_display_shows_full_diff_result_for_edit_and_write() {
        for tool in ["Edit", "Write"] {
            let (out, _) = with_sink(|| {
                let mut display = SimpleDisplay::new(context(
                    tool,
                    DisplayFormat::File { show_size: true },
                    json!({}),
                ));
                display.complete_success("diff line one\ndiff line two");
            });

            assert!(out.contains(&format!("[tool] {tool} ok")), "output: {}", out);
            assert!(out.contains("diff line one\ndiff line two"), "output: {}", out);
        }
    }

    #[test]
    #[serial]
    fn simple_display_reports_errors_on_stderr() {
        let (out, err) = with_sink(|| {
            let mut display = SimpleDisplay::new(context(
                "delete_file",
                DisplayFormat::File { show_size: false },
                json!({}),
            ));
            display.complete_error("permission denied\nstack details");
        });

        assert!(out.is_empty(), "success stream should be empty: {}", out);
        assert!(err.contains("[tool] delete_file failed"), "stderr: {}", err);
        assert!(err.contains("permission denied"));
        assert!(!err.contains("stack details"));
    }

    #[test]
    #[serial]
    fn simple_display_show_call_details_is_a_noop() {
        let (out, err) = with_sink(|| {
            let mut display = SimpleDisplay::new(context(
                "Read",
                DisplayFormat::Generic,
                json!({ "path": "/tmp/x.txt" }),
            ));
            display.show_call_details(&json!({ "path": "/tmp/x.txt" }));
        });

        assert!(out.is_empty());
        assert!(err.is_empty());
    }

    #[test]
    #[serial]
    fn pretty_display_formats_file_params_with_sizes() {
        let (out, _) = with_sink(|| {
            let mut display = PrettyDisplay::new(context(
                "Write",
                DisplayFormat::File { show_size: true },
                json!({ "path": "/tmp/out.txt", "content": "12345", "old_text": "ab", "new_text": "cde" }),
            ));
            display.complete_success("wrote file");
        });

        assert!(out.contains("Tool Call: Write"), "output: {}", out);
        assert!(out.contains("path=/tmp/out.txt"));
        assert!(out.contains("size=5"));
        assert!(out.contains("old_len=2"));
        assert!(out.contains("new_len=3"));
        assert!(out.contains("Result: Write"));
        assert!(out.contains("SUCCESS"));
        assert!(out.contains("wrote file"));
    }

    #[test]
    #[serial]
    fn pretty_display_formats_command_params_and_truncates() {
        let long_command = "x".repeat(100);
        let command = format!("echo {long_command}");
        let (out, _) = with_sink(|| {
            let mut display = PrettyDisplay::new(context(
                "Bash",
                DisplayFormat::Command,
                json!({ "command": command }),
            ));
            display.complete_success("");
        });

        assert!(out.contains("Tool Call: Bash"), "output: {}", out);
        // "echo " plus 55 x's fills the 60-char budget before the ellipsis
        let expected = format!("cmd=echo {}...", "x".repeat(55));
        assert!(out.contains(&expected), "output: {}", out);
        assert!(!out.contains(&"x".repeat(70)), "output: {}", out);
    }

    #[test]
    #[serial]
    fn pretty_display_formats_generic_params_from_json_values() {
        let (out, _) = with_sink(|| {
            let mut display = PrettyDisplay::new(context(
                "use_skill",
                DisplayFormat::Generic,
                json!({ "depth": 3, "extra": "ignored", "name": "code-review", "zeta": "dropped" }),
            ));
            display.complete_success("");
        });

        // Only the first 3 params (in map order) are rendered
        assert!(out.contains("depth=3"), "output: {}", out);
        assert!(out.contains("extra=ignored"), "output: {}", out);
        assert!(out.contains("name=code-review"), "output: {}", out);
        assert!(!out.contains("zeta=dropped"), "output: {}", out);
    }

    #[test]
    #[serial]
    fn pretty_display_handles_arguments_without_params() {
        let (out, _) = with_sink(|| {
            let mut display = PrettyDisplay::new(context(
                "list_directory",
                DisplayFormat::Directory,
                json!({}),
            ));
            display.complete_success("");
        });

        assert!(out.contains("Tool Call: list_directory"), "output: {}", out);
        assert!(!out.contains("path="));
    }

    #[test]
    #[serial]
    fn pretty_display_reports_errors_with_failed_status() {
        let (out, err) = with_sink(|| {
            let mut display = PrettyDisplay::new(context(
                "Bash",
                DisplayFormat::Command,
                json!({ "command": "false" }),
            ));
            display.complete_error("command failed");
        });

        assert!(err.is_empty());
        assert!(out.contains("FAILED"), "output: {}", out);
        assert!(!out.contains("SUCCESS"));
    }

    #[test]
    #[serial]
    fn pretty_display_truncates_multiline_values_to_one_line() {
        let long_tail = "x".repeat(70);
        let path = format!("a.txt\n{long_tail}");
        let (out, _) = with_sink(|| {
            let mut display = PrettyDisplay::new(context(
                "Write",
                DisplayFormat::File { show_size: true },
                json!({ "path": path, "content": "" }),
            ));
            display.complete_success("");
        });

        // Newlines collapse to spaces and the value is cut at 60 chars
        assert!(out.contains("path=a.txt "), "output: {}", out);
        assert!(!out.contains(&long_tail), "output: {}", out);
        assert!(out.contains("..."), "output: {}", out);
    }
}
