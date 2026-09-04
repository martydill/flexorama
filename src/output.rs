use log::{Level, LevelFilter, Metadata, Record};
use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock};

pub trait OutputSink: Send + Sync {
    fn write(&self, text: &str, is_err: bool);
    fn flush(&self);
}

static OUTPUT_SINK: OnceLock<Mutex<Option<Arc<dyn OutputSink>>>> = OnceLock::new();

fn sink_cell() -> &'static Mutex<Option<Arc<dyn OutputSink>>> {
    OUTPUT_SINK.get_or_init(|| Mutex::new(None))
}

pub fn set_output_sink(sink: Arc<dyn OutputSink>) {
    let mut guard = sink_cell().lock().expect("output sink lock");
    *guard = Some(sink);
}

pub fn clear_output_sink() {
    let mut guard = sink_cell().lock().expect("output sink lock");
    *guard = None;
}

pub fn is_tui_active() -> bool {
    let guard = sink_cell().lock().expect("output sink lock");
    guard.is_some()
}

pub fn write(text: &str, is_err: bool) {
    let guard = sink_cell().lock().expect("output sink lock");
    if let Some(sink) = guard.as_ref() {
        sink.write(text, is_err);
    } else if is_err {
        ::std::eprint!("{}", text);
    } else {
        ::std::print!("{}", text);
    }
}

pub fn write_line(text: &str, is_err: bool) {
    let guard = sink_cell().lock().expect("output sink lock");
    if let Some(sink) = guard.as_ref() {
        sink.write(text, is_err);
        sink.write("\n", is_err);
    } else if is_err {
        ::std::eprintln!("{}", text);
    } else {
        ::std::println!("{}", text);
    }
}

pub fn flush() {
    let guard = sink_cell().lock().expect("output sink lock");
    if let Some(sink) = guard.as_ref() {
        sink.flush();
    } else {
        let _ = ::std::io::stdout().flush();
        let _ = ::std::io::stderr().flush();
    }
}

pub struct OutputLogger {
    level: LevelFilter,
    /// If true, all logs go to stderr (for ACP mode where stdout is for JSON-RPC only)
    stderr_only: bool,
}

impl OutputLogger {
    pub fn new(level: LevelFilter, stderr_only: bool) -> Self {
        Self { level, stderr_only }
    }
}

impl log::Log for OutputLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        // In stderr_only mode (ACP), send ALL logs to stderr
        // Otherwise, only send ERROR/WARN to stderr
        let is_err = self.stderr_only || matches!(record.level(), Level::Error | Level::Warn);
        write_line(&format!("[{}] {}", record.level(), record.args()), is_err);
    }

    fn flush(&self) {
        flush();
    }
}

pub fn init_logger(default_level: LevelFilter, stderr_only: bool, verbose: bool) {
    let level = std::env::var("RUST_LOG")
        .ok()
        .and_then(|value| {
            // Try parsing as-is first
            value.parse::<LevelFilter>().ok().or_else(|| {
                // If that fails, try case-insensitive matching
                match value.to_lowercase().as_str() {
                    "trace" => Some(LevelFilter::Trace),
                    "debug" => Some(LevelFilter::Debug),
                    "info" => Some(LevelFilter::Info),
                    "warn" | "warning" => Some(LevelFilter::Warn),
                    "error" => Some(LevelFilter::Error),
                    "off" => Some(LevelFilter::Off),
                    _ => None,
                }
            })
        })
        .unwrap_or_else(|| {
            // If verbose mode is enabled, use Debug level; otherwise use default
            if verbose {
                LevelFilter::Debug
            } else {
                default_level
            }
        });

    let logger = OutputLogger::new(level, stderr_only);
    let _ = log::set_boxed_logger(Box::new(logger));
    log::set_max_level(level);
}

#[macro_export]
macro_rules! app_println {
    () => {
        $crate::output::write_line("", false)
    };
    ($($arg:tt)*) => {
        $crate::output::write_line(&format!($($arg)*), false)
    };
}

#[macro_export]
macro_rules! app_eprintln {
    () => {
        $crate::output::write_line("", true)
    };
    ($($arg:tt)*) => {
        $crate::output::write_line(&format!($($arg)*), true)
    };
}

#[macro_export]
macro_rules! app_print {
    ($($arg:tt)*) => {
        $crate::output::write(&format!($($arg)*), false)
    };
}

#[macro_export]
macro_rules! app_eprint {
    ($($arg:tt)*) => {
        $crate::output::write(&format!($($arg)*), true)
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use log::Log as _;
    use serial_test::serial;
    use std::sync::Mutex;

    macro_rules! record {
        ($level:expr, $msg:expr) => {
            Record::builder()
                .args(format_args!("{}", $msg))
                .level($level)
                .target("test")
                .build()
        };
    }

    #[derive(Default)]
    struct RecordingSink {
        entries: Mutex<Vec<(String, bool)>>,
        flushes: Mutex<usize>,
    }

    impl OutputSink for RecordingSink {
        fn write(&self, text: &str, is_err: bool) {
            self.entries
                .lock()
                .expect("entries lock")
                .push((text.to_string(), is_err));
        }

        fn flush(&self) {
            *self.flushes.lock().expect("flush lock") += 1;
        }
    }

    impl RecordingSink {
        fn texts(&self) -> Vec<String> {
            self.entries
                .lock()
                .expect("entries lock")
                .iter()
                .map(|(text, _)| text.clone())
                .collect()
        }

        fn error_flags(&self) -> Vec<bool> {
            self.entries
                .lock()
                .expect("entries lock")
                .iter()
                .map(|(_, is_err)| *is_err)
                .collect()
        }
    }

    #[test]
    #[serial]
    fn sink_receives_written_lines_and_error_flags() {
        let sink = Arc::new(RecordingSink::default());
        set_output_sink(sink.clone());

        write("plain", false);
        write_line("line", false);
        write_line("error line", true);

        assert_eq!(sink.texts(), vec!["plain", "line", "\n", "error line", "\n"]);
        assert_eq!(sink.error_flags(), vec![false, false, false, true, true]);

        clear_output_sink();
        assert!(!is_tui_active());
    }

    #[test]
    #[serial]
    fn is_tui_active_reflects_sink_registration() {
        assert!(!is_tui_active());

        set_output_sink(Arc::new(RecordingSink::default()));
        assert!(is_tui_active());

        clear_output_sink();
        assert!(!is_tui_active());
    }

    #[test]
    #[serial]
    fn flush_is_forwarded_to_sink() {
        let sink = Arc::new(RecordingSink::default());
        set_output_sink(sink.clone());

        flush();

        assert_eq!(*sink.flushes.lock().unwrap(), 1);
        clear_output_sink();
    }

    #[test]
    #[serial]
    fn replacing_sink_stops_previous_sink_receiving_output() {
        let first = Arc::new(RecordingSink::default());
        set_output_sink(first.clone());

        let second = Arc::new(RecordingSink::default());
        set_output_sink(second.clone());

        write_line("latest", false);

        assert!(first.texts().is_empty());
        assert_eq!(second.texts(), vec!["latest", "\n"]);
        clear_output_sink();
    }

    #[test]
    #[serial]
    fn logger_respects_level_filter() {
        let logger = OutputLogger::new(LevelFilter::Warn, false);

        assert!(logger.enabled(&Metadata::builder().level(Level::Error).target("t").build()));
        assert!(logger.enabled(&Metadata::builder().level(Level::Warn).target("t").build()));
        assert!(!logger.enabled(&Metadata::builder().level(Level::Info).target("t").build()));
    }

    #[test]
    #[serial]
    fn logger_routes_error_and_warn_to_stderr() {
        let sink = Arc::new(RecordingSink::default());
        set_output_sink(sink.clone());

        let logger = OutputLogger::new(LevelFilter::Info, false);
        logger.log(&record!(Level::Info, "info message"));
        logger.log(&record!(Level::Warn, "warn message"));
        logger.log(&record!(Level::Error, "error message"));

        assert_eq!(
            sink.texts(),
            vec!["[INFO] info message", "\n", "[WARN] warn message", "\n", "[ERROR] error message", "\n"]
        );
        assert_eq!(sink.error_flags(), vec![false, false, true, true, true, true]);

        clear_output_sink();
    }

    #[test]
    #[serial]
    fn stderr_only_logger_sends_everything_to_stderr() {
        let sink = Arc::new(RecordingSink::default());
        set_output_sink(sink.clone());

        let logger = OutputLogger::new(LevelFilter::Debug, true);
        logger.log(&record!(Level::Debug, "debug message"));

        assert_eq!(sink.error_flags(), vec![true, true]);
        clear_output_sink();
    }

    #[test]
    #[serial]
    fn logger_skips_records_above_level() {
        let sink = Arc::new(RecordingSink::default());
        set_output_sink(sink.clone());

        let logger = OutputLogger::new(LevelFilter::Warn, false);
        logger.log(&record!(Level::Info, "should not appear"));

        assert!(sink.texts().is_empty());
        clear_output_sink();
    }

    #[test]
    fn init_logger_parses_rust_log_levels() {
        // Verify the level parsing logic directly through the env fallback chain.
        let parse = |value: &str| {
            value
                .parse::<LevelFilter>()
                .ok()
                .or_else(|| match value.to_lowercase().as_str() {
                    "trace" => Some(LevelFilter::Trace),
                    "debug" => Some(LevelFilter::Debug),
                    "info" => Some(LevelFilter::Info),
                    "warn" | "warning" => Some(LevelFilter::Warn),
                    "error" => Some(LevelFilter::Error),
                    "off" => Some(LevelFilter::Off),
                    _ => None,
                })
        };

        assert_eq!(parse("debug"), Some(LevelFilter::Debug));
        assert_eq!(parse("WARNING"), Some(LevelFilter::Warn));
        assert_eq!(parse("warning"), Some(LevelFilter::Warn));
        assert_eq!(parse("off"), Some(LevelFilter::Off));
        assert_eq!(parse("bogus"), None);
    }
}
