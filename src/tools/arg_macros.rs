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

/// Extract a required array argument from a ToolCall
///
/// # Example
/// ```ignore
/// let paths = extract_array_arg!(call, "paths")?;
/// ```
#[macro_export]
macro_rules! extract_array_arg {
    ($call:expr, $name:expr) => {
        $call
            .arguments
            .get($name)
            .ok_or_else(|| anyhow::anyhow!("Missing '{}' argument", $name))?
    };
}

#[cfg(test)]
mod tests {
    use crate::tools::types::ToolCall;
    use serde_json::json;

    fn call(arguments: serde_json::Value) -> ToolCall {
        ToolCall {
            id: "test-id".to_string(),
            name: "test".to_string(),
            arguments,
        }
    }

    fn string_arg(arguments: serde_json::Value, name: &str) -> anyhow::Result<String> {
        let c = call(arguments);
        Ok(extract_string_arg!(c, name).to_string())
    }

    fn optional_string_arg(arguments: serde_json::Value, name: &str) -> Option<String> {
        let c = call(arguments);
        extract_optional_string_arg!(c, name).map(str::to_string)
    }

    fn int_arg(arguments: serde_json::Value, name: &str) -> anyhow::Result<i64> {
        let c = call(arguments);
        Ok(extract_int_arg!(c, name))
    }

    fn optional_int_arg(arguments: serde_json::Value, name: &str) -> Option<i64> {
        let c = call(arguments);
        extract_optional_int_arg!(c, name)
    }

    fn bool_arg(arguments: serde_json::Value, name: &str) -> anyhow::Result<bool> {
        let c = call(arguments);
        Ok(extract_bool_arg!(c, name))
    }

    fn optional_bool_arg(arguments: serde_json::Value, name: &str) -> bool {
        let c = call(arguments);
        extract_optional_bool_arg!(c, name)
    }

    fn array_arg(arguments: serde_json::Value, name: &str) -> anyhow::Result<serde_json::Value> {
        let c = call(arguments);
        Ok(extract_array_arg!(c, name).clone())
    }

    #[test]
    fn extract_string_arg_returns_value_when_present() {
        assert_eq!(
            string_arg(json!({ "path": "/tmp/a.txt" }), "path").unwrap(),
            "/tmp/a.txt"
        );
    }

    #[test]
    fn extract_string_arg_fails_when_missing() {
        let err = string_arg(json!({}), "path").unwrap_err();
        assert_eq!(err.to_string(), "Missing 'path' argument");
    }

    #[test]
    fn extract_string_arg_fails_on_non_string_value() {
        let err = string_arg(json!({ "path": 3 }), "path").unwrap_err();
        assert_eq!(err.to_string(), "Missing 'path' argument");
    }

    #[test]
    fn extract_optional_string_arg_variants() {
        assert_eq!(
            optional_string_arg(json!({ "name": "x" }), "name"),
            Some("x".to_string())
        );
        assert_eq!(optional_string_arg(json!({}), "name"), None);
        assert_eq!(optional_string_arg(json!({ "name": 5 }), "name"), None);
    }

    #[test]
    fn extract_int_arg_returns_value_when_present() {
        assert_eq!(int_arg(json!({ "count": 42 }), "count").unwrap(), 42);
    }

    #[test]
    fn extract_int_arg_fails_when_missing_or_invalid() {
        let missing = int_arg(json!({}), "count").unwrap_err();
        assert_eq!(
            missing.to_string(),
            "Missing or invalid 'count' argument"
        );

        let invalid = int_arg(json!({ "count": "42" }), "count").unwrap_err();
        assert_eq!(
            invalid.to_string(),
            "Missing or invalid 'count' argument"
        );
    }

    #[test]
    fn extract_optional_int_arg_variants() {
        assert_eq!(optional_int_arg(json!({ "limit": 7 }), "limit"), Some(7));
        assert_eq!(optional_int_arg(json!({}), "limit"), None);
        assert_eq!(optional_int_arg(json!({ "limit": true }), "limit"), None);
    }

    #[test]
    fn extract_bool_arg_returns_value_when_present() {
        assert!(bool_arg(json!({ "enabled": true }), "enabled").unwrap());
        assert!(!bool_arg(json!({ "enabled": false }), "enabled").unwrap());
    }

    #[test]
    fn extract_bool_arg_fails_when_missing_or_invalid() {
        let missing = bool_arg(json!({}), "enabled").unwrap_err();
        assert!(missing.to_string().contains("'enabled'"));

        let invalid = bool_arg(json!({ "enabled": "yes" }), "enabled").unwrap_err();
        assert!(invalid.to_string().contains("'enabled'"));
    }

    #[test]
    fn extract_optional_bool_arg_defaults_to_false() {
        assert!(optional_bool_arg(json!({ "dry": true }), "dry"));
        assert!(!optional_bool_arg(json!({}), "dry"));
        assert!(!optional_bool_arg(json!({ "dry": 1 }), "dry"));
    }

    #[test]
    fn extract_array_arg_returns_value_when_present() {
        assert_eq!(
            array_arg(json!({ "paths": ["a", "b"] }), "paths").unwrap(),
            json!(["a", "b"])
        );
    }

    #[test]
    fn extract_array_arg_fails_when_missing() {
        let err = array_arg(json!({}), "paths").unwrap_err();
        assert_eq!(err.to_string(), "Missing 'paths' argument");
    }
}
