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

pub fn init_logger(default_level: LevelFilter, stderr_only: bool) {
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
        .unwrap_or(default_level);

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
