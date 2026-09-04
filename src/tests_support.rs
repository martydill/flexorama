//! Shared helpers used by in-module test suites.

/// Captures everything written to the global output sink while `f` runs,
/// returning the combined text (stdout and stderr interleaved).
pub fn capture_stdout<F: FnOnce()>(f: F) -> String {
    use std::sync::{Arc, Mutex};

    struct Sink(Mutex<Vec<(String, bool)>>);

    impl crate::output::OutputSink for Sink {
        fn write(&self, text: &str, is_err: bool) {
            self.0.lock().unwrap().push((text.to_string(), is_err));
        }

        fn flush(&self) {}
    }

    colored::control::set_override(false);
    let sink = Arc::new(Sink(Mutex::new(Vec::new())));
    crate::output::set_output_sink(sink.clone());
    f();
    crate::output::clear_output_sink();
    colored::control::unset_override();

    let guard = sink.0.lock().unwrap();
    let text = guard
        .iter()
        .map(|(line, _)| line.as_str())
        .collect::<Vec<_>>()
        .join("");
    drop(guard);
    text
}
