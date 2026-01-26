/// Macros for extracting arguments from ToolCall arguments in a concise way.
/// These macros reduce boilerplate and make tool implementations more readable.
/// Extract a required string argument from a ToolCall
///
/// # Example
/// ```ignore
/// let path = extract_string_arg!(call, "path")?;
/// ```
#[macro_export]
macro_rules! extract_string_arg {
    ($call:expr, $name:expr) => {
        $call
            .arguments
            .get($name)
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing '{}' argument", $name))?
    };
}

/// Extract an optional string argument from a ToolCall
///
/// # Example
/// ```ignore
/// let optional_path = extract_optional_string_arg!(call, "path");
/// ```
#[macro_export]
macro_rules! extract_optional_string_arg {
    ($call:expr, $name:expr) => {
        $call.arguments.get($name).and_then(|v| v.as_str())
    };
}

/// Extract a required integer argument from a ToolCall
///
/// # Example
/// ```ignore
/// let count = extract_int_arg!(call, "count")?;
/// ```
#[macro_export]
macro_rules! extract_int_arg {
    ($call:expr, $name:expr) => {
        $call
            .arguments
            .get($name)
            .and_then(|v| v.as_i64())
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid '{}' argument", $name))?
    };
}

/// Extract an optional integer argument from a ToolCall
///
/// # Example
/// ```ignore
/// let optional_count = extract_optional_int_arg!(call, "count");
/// ```
#[macro_export]
macro_rules! extract_optional_int_arg {
    ($call:expr, $name:expr) => {
        $call.arguments.get($name).and_then(|v| v.as_i64())
    };
}

/// Extract a required boolean argument from a ToolCall
///
/// # Example
/// ```ignore
/// let enabled = extract_bool_arg!(call, "enabled")?;
/// ```
#[macro_export]
macro_rules! extract_bool_arg {
    ($call:expr, $name:expr) => {
        $call
            .arguments
            .get($name)
            .and_then(|v| v.as_bool())
            .ok_or_else(|| anyhow::anyhow!("Missing or invalid '{}' argument", $name))?
    };
}

/// Extract an optional boolean argument from a ToolCall (defaults to false if not present)
///
/// # Example
/// ```ignore
/// let enabled = extract_optional_bool_arg!(call, "enabled");
/// ```
#[macro_export]
macro_rules! extract_optional_bool_arg {
    ($call:expr, $name:expr) => {
        $call
            .arguments
            .get($name)
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
    };
}
