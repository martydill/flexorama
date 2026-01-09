use crate::formatter::{create_code_formatter, CodeFormatter, StreamingResponseFormatter};
use anyhow::Result;

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================================
    // CodeFormatter Creation and Initialization Tests
    // ============================================================================

    #[test]
    fn test_code_formatter_creation() {
        let formatter = create_code_formatter();
        assert!(formatter.is_ok());
    }

    #[test]
    fn test_code_formatter_new() {
        let formatter = CodeFormatter::new();
        assert!(formatter.is_ok());
    }

    #[test]
    fn test_code_formatter_clone() {
        let formatter = create_code_formatter().unwrap();
        let cloned = formatter.clone();

        // Test that cloned formatter works correctly
        let input = "Test @file.txt";
        let result1 = formatter.format_input_with_file_highlighting(input);
        let result2 = cloned.format_input_with_file_highlighting(input);
        assert_eq!(result1, result2);
    }

    // ============================================================================
    // Language Normalization Tests
    // ============================================================================

    #[test]
    fn test_normalize_language_javascript() {
        let formatter = create_code_formatter().unwrap();
        assert_eq!(formatter.normalize_language("js"), "javascript");
        assert_eq!(formatter.normalize_language("jsx"), "javascript");
        assert_eq!(formatter.normalize_language("javascript"), "javascript");
    }

    #[test]
    fn test_normalize_language_typescript() {
        let formatter = create_code_formatter().unwrap();
        assert_eq!(formatter.normalize_language("ts"), "typescript");
        assert_eq!(formatter.normalize_language("tsx"), "typescript");
        assert_eq!(formatter.normalize_language("typescript"), "typescript");
    }

    #[test]
    fn test_normalize_language_python() {
        let formatter = create_code_formatter().unwrap();
        assert_eq!(formatter.normalize_language("py"), "python");
        assert_eq!(formatter.normalize_language("python"), "python");
    }

    #[test]
    fn test_normalize_language_rust() {
        let formatter = create_code_formatter().unwrap();
        assert_eq!(formatter.normalize_language("rs"), "rust");
        assert_eq!(formatter.normalize_language("rust"), "rust");
    }

    #[test]
    fn test_normalize_language_shell() {
        let formatter = create_code_formatter().unwrap();
        assert_eq!(formatter.normalize_language("sh"), "bash");
        assert_eq!(formatter.normalize_language("bash"), "bash");
        assert_eq!(formatter.normalize_language("zsh"), "bash");
    }

    #[test]
    fn test_normalize_language_yaml() {
        let formatter = create_code_formatter().unwrap();
        assert_eq!(formatter.normalize_language("yml"), "yaml");
        assert_eq!(formatter.normalize_language("yaml"), "yaml");
    }

    #[test]
    fn test_normalize_language_cpp() {
        let formatter = create_code_formatter().unwrap();
        assert_eq!(formatter.normalize_language("cpp"), "cpp");
        assert_eq!(formatter.normalize_language("cxx"), "cpp");
        assert_eq!(formatter.normalize_language("cc"), "cpp");
    }

    #[test]
    fn test_normalize_language_markdown() {
        let formatter = create_code_formatter().unwrap();
        assert_eq!(formatter.normalize_language("md"), "markdown");
        assert_eq!(formatter.normalize_language("markdown"), "markdown");
    }

    #[test]
    fn test_normalize_language_empty() {
        let formatter = create_code_formatter().unwrap();
        assert_eq!(formatter.normalize_language(""), "text");
    }

    #[test]
    fn test_normalize_language_unknown() {
        let formatter = create_code_formatter().unwrap();
        assert_eq!(formatter.normalize_language("unknown"), "unknown");
    }

    #[test]
    fn test_normalize_language_case_insensitive() {
        let formatter = create_code_formatter().unwrap();
        // For known aliases, case doesn't matter
        assert_eq!(formatter.normalize_language("RS"), "rust");
        assert_eq!(formatter.normalize_language("rs"), "rust");
        assert_eq!(formatter.normalize_language("JS"), "javascript");
        assert_eq!(formatter.normalize_language("js"), "javascript");
    }

    // ============================================================================
    // File Highlighting Tests
    // ============================================================================

    #[test]
    fn test_file_highlighting_single_file() {
        let formatter = create_code_formatter().unwrap();
        let input = "Please read @test_file.txt";
        let highlighted = formatter.format_input_with_file_highlighting(input);
        assert!(highlighted.contains("@test_file.txt"));
    }

    #[test]
    fn test_file_highlighting_multiple_files() {
        let formatter = create_code_formatter().unwrap();
        let input = "Compare @file1.txt and @file2.txt";
        let highlighted = formatter.format_input_with_file_highlighting(input);
        assert!(highlighted.contains("@file1.txt"));
        assert!(highlighted.contains("@file2.txt"));
    }

    #[test]
    fn test_file_highlighting_no_files() {
        let formatter = create_code_formatter().unwrap();
        let input = "Just a regular message without files";
        let highlighted = formatter.format_input_with_file_highlighting(input);
        assert_eq!(input, highlighted);
    }

    #[test]
    fn test_file_highlighting_with_path() {
        let formatter = create_code_formatter().unwrap();
        let input = "Check @src/main.rs for details";
        let highlighted = formatter.format_input_with_file_highlighting(input);
        assert!(highlighted.contains("@src/main.rs"));
    }

    #[test]
    fn test_file_highlighting_multiple_paths() {
        let formatter = create_code_formatter().unwrap();
        let input = "Compare @src/lib.rs and @tests/integration.rs";
        let highlighted = formatter.format_input_with_file_highlighting(input);
        assert!(highlighted.contains("@src/lib.rs"));
        assert!(highlighted.contains("@tests/integration.rs"));
    }

    #[test]
    fn test_file_highlighting_with_extensions() {
        let formatter = create_code_formatter().unwrap();
        let input = "Check @config.toml, @package.json, and @Cargo.lock";
        let highlighted = formatter.format_input_with_file_highlighting(input);
        assert!(highlighted.contains("@config.toml"));
        assert!(highlighted.contains("@package.json"));
        assert!(highlighted.contains("@Cargo.lock"));
    }

    #[test]
    fn test_file_highlighting_at_start() {
        let formatter = create_code_formatter().unwrap();
        let input = "@file.txt is the main config";
        let highlighted = formatter.format_input_with_file_highlighting(input);
        assert!(highlighted.contains("@file.txt"));
    }

    #[test]
    fn test_file_highlighting_at_end() {
        let formatter = create_code_formatter().unwrap();
        let input = "The config is in @file.txt";
        let highlighted = formatter.format_input_with_file_highlighting(input);
        assert!(highlighted.contains("@file.txt"));
    }

    #[test]
    fn test_file_highlighting_cache_hit() {
        let formatter = create_code_formatter().unwrap();
        let input = "Test @file.txt";

        // First call should populate cache
        let result1 = formatter.format_input_with_file_highlighting(input);

        // Second call should hit cache and return same result
        let result2 = formatter.format_input_with_file_highlighting(input);

        assert_eq!(result1, result2);
    }

    #[test]
    fn test_file_highlighting_cache_miss() {
        let formatter = create_code_formatter().unwrap();
        let input1 = "Test @file1.txt";
        let input2 = "Test @file2.txt";

        let result1 = formatter.format_input_with_file_highlighting(input1);
        let result2 = formatter.format_input_with_file_highlighting(input2);

        // Results should be different since inputs are different
        assert_ne!(result1, result2);
    }

    #[test]
    fn test_file_highlighting_fast_path_no_at_symbol() {
        let formatter = create_code_formatter().unwrap();
        let input = "This has no file references at all";
        let result = formatter.format_input_with_file_highlighting(input);
        assert_eq!(input, result);
    }

    // ============================================================================
    // Code Block Formatting Tests
    // ============================================================================

    #[test]
    fn test_format_response_no_code_blocks() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "Just plain text without any code blocks";
        let result = formatter.format_response(input)?;
        assert!(result.contains("plain text"));
        Ok(())
    }

    #[test]
    fn test_format_response_single_code_block() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "Here's some code:\n```rust\nfn main() {}\n```";
        let result = formatter.format_response(input)?;
        assert!(result.contains("fn main"));
        Ok(())
    }

    #[test]
    fn test_format_response_multiple_code_blocks() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "First:\n```rust\nfn foo() {}\n```\nSecond:\n```python\ndef bar():\n    pass\n```";
        let result = formatter.format_response(input)?;
        assert!(result.contains("fn foo"));
        assert!(result.contains("def bar"));
        Ok(())
    }

    #[test]
    fn test_format_code_block_rust() -> Result<()> {
        let formatter = create_code_formatter()?;
        let code = "fn main() {\n    let x = 42;\n}";
        let result = formatter.format_code_block(code, "rust")?;
        assert!(result.contains("fn main"));
        assert!(result.contains("RUST"));
        Ok(())
    }

    #[test]
    fn test_format_code_block_python() -> Result<()> {
        let formatter = create_code_formatter()?;
        let code = "def hello():\n    print('Hello')";
        let result = formatter.format_code_block(code, "python")?;
        assert!(result.contains("def hello"));
        assert!(result.contains("PYTHON"));
        Ok(())
    }

    #[test]
    fn test_format_code_block_javascript() -> Result<()> {
        let formatter = create_code_formatter()?;
        let code = "function test() {\n    return 42;\n}";
        let result = formatter.format_code_block(code, "javascript")?;
        assert!(result.contains("function test"));
        assert!(result.contains("JAVASCRIPT"));
        Ok(())
    }

    #[test]
    fn test_format_code_block_typescript() -> Result<()> {
        let formatter = create_code_formatter()?;
        let code = "interface User {\n    name: string;\n}";
        let result = formatter.format_code_block(code, "typescript")?;
        assert!(result.contains("interface User"));
        assert!(result.contains("TYPESCRIPT"));
        Ok(())
    }

    #[test]
    fn test_format_code_block_json() -> Result<()> {
        let formatter = create_code_formatter()?;
        let code = r#"{"key": "value", "number": 42}"#;
        let result = formatter.format_code_block(code, "json")?;
        assert!(result.contains("key"));
        assert!(result.contains("JSON"));
        Ok(())
    }

    #[test]
    fn test_format_code_block_yaml() -> Result<()> {
        let formatter = create_code_formatter()?;
        let code = "name: test\nversion: 1.0";
        let result = formatter.format_code_block(code, "yaml")?;
        assert!(result.contains("name"));
        assert!(result.contains("YAML"));
        Ok(())
    }

    #[test]
    fn test_format_code_block_bash() -> Result<()> {
        let formatter = create_code_formatter()?;
        let code = "#!/bin/bash\necho 'Hello'";
        let result = formatter.format_code_block(code, "bash")?;
        assert!(result.contains("echo"));
        assert!(result.contains("BASH"));
        Ok(())
    }

    #[test]
    fn test_format_code_block_sql() -> Result<()> {
        let formatter = create_code_formatter()?;
        let code = "SELECT * FROM users WHERE id = 1";
        let result = formatter.format_code_block(code, "sql")?;
        assert!(result.contains("SELECT"));
        assert!(result.contains("SQL"));
        Ok(())
    }

    #[test]
    fn test_format_code_block_empty() -> Result<()> {
        let formatter = create_code_formatter()?;
        let code = "";
        let result = formatter.format_code_block(code, "rust")?;
        assert!(result.contains("RUST"));
        Ok(())
    }

    #[test]
    fn test_format_code_block_unknown_language() -> Result<()> {
        let formatter = create_code_formatter()?;
        let code = "some code here";
        let result = formatter.format_code_block(code, "unknown")?;
        assert!(result.contains("UNKNOWN"));
        assert!(result.contains("some code"));
        Ok(())
    }

    // ============================================================================
    // Language-Specific Highlighting Tests
    // ============================================================================

    #[test]
    fn test_highlight_rust_keywords() {
        let formatter = create_code_formatter().unwrap();
        let line = "fn main() { let mut x = 42; }";
        let result = formatter.highlight_line(line, "rust");
        assert!(result.contains("main"));
    }

    #[test]
    fn test_highlight_rust_types() {
        let formatter = create_code_formatter().unwrap();
        let line = "let s: String = String::from(\"test\");";
        let result = formatter.highlight_line(line, "rust");
        assert!(result.contains("String"));
    }

    #[test]
    fn test_highlight_rust_comments() {
        let formatter = create_code_formatter().unwrap();
        let line = "// This is a comment";
        let result = formatter.highlight_line(line, "rust");
        assert!(result.contains("comment"));
    }

    #[test]
    fn test_highlight_python_keywords() {
        let formatter = create_code_formatter().unwrap();
        let line = "def function(): pass";
        let result = formatter.highlight_line(line, "python");
        assert!(result.contains("function"));
    }

    #[test]
    fn test_highlight_python_comments() {
        let formatter = create_code_formatter().unwrap();
        let line = "# This is a comment";
        let result = formatter.highlight_line(line, "python");
        assert!(result.contains("comment"));
    }

    #[test]
    fn test_highlight_javascript_keywords() {
        let formatter = create_code_formatter().unwrap();
        let line = "const x = function() { return 42; }";
        let result = formatter.highlight_line(line, "javascript");
        assert!(result.contains("return"));
    }

    #[test]
    fn test_highlight_typescript_keywords() {
        let formatter = create_code_formatter().unwrap();
        let line = "interface User { name: string; }";
        let result = formatter.highlight_line(line, "typescript");
        assert!(result.contains("User"));
    }

    #[test]
    fn test_highlight_json_keys() {
        let formatter = create_code_formatter().unwrap();
        let line = r#"  "name": "value""#;
        let result = formatter.highlight_line(line, "json");
        assert!(result.contains("name"));
    }

    #[test]
    fn test_highlight_yaml_keys() {
        let formatter = create_code_formatter().unwrap();
        let line = "name: value";
        let result = formatter.highlight_line(line, "yaml");
        assert!(result.contains("name"));
    }

    #[test]
    fn test_highlight_html_tags() {
        let formatter = create_code_formatter().unwrap();
        let line = "<div class=\"test\">Content</div>";
        let result = formatter.highlight_line(line, "html");
        assert!(result.contains("div"));
    }

    #[test]
    fn test_highlight_css_properties() {
        let formatter = create_code_formatter().unwrap();
        let line = "color: red;";
        let result = formatter.highlight_line(line, "css");
        assert!(result.contains("color"));
    }

    #[test]
    fn test_highlight_bash_commands() {
        let formatter = create_code_formatter().unwrap();
        let line = "echo 'Hello World'";
        let result = formatter.highlight_line(line, "bash");
        assert!(result.contains("Hello"));
    }

    #[test]
    fn test_highlight_sql_keywords() {
        let formatter = create_code_formatter().unwrap();
        let line = "SELECT * FROM users";
        let result = formatter.highlight_line(line, "sql");
        assert!(result.contains("users"));
    }

    #[test]
    fn test_highlight_markdown_headers() {
        let formatter = create_code_formatter().unwrap();
        let line = "# Header 1";
        let result = formatter.highlight_line(line, "markdown");
        assert!(result.contains("Header"));
    }

    #[test]
    fn test_highlight_markdown_bold() {
        let formatter = create_code_formatter().unwrap();
        let line = "This is **bold** text";
        let result = formatter.highlight_line(line, "markdown");
        assert!(result.contains("bold"));
    }

    #[test]
    fn test_highlight_toml_sections() {
        let formatter = create_code_formatter().unwrap();
        let line = "[package]";
        let result = formatter.highlight_line(line, "toml");
        assert!(result.contains("package"));
    }

    #[test]
    fn test_highlight_toml_keys() {
        let formatter = create_code_formatter().unwrap();
        let line = "name = \"test\"";
        let result = formatter.highlight_line(line, "toml");
        assert!(result.contains("name"));
    }

    #[test]
    fn test_highlight_c_cpp_keywords() {
        let formatter = create_code_formatter().unwrap();
        let line = "int main() { return 0; }";
        let result = formatter.highlight_line(line, "c");
        assert!(result.contains("main"));
    }

    #[test]
    fn test_highlight_java_keywords() {
        let formatter = create_code_formatter().unwrap();
        let line = "public class Main { }";
        let result = formatter.highlight_line(line, "java");
        assert!(result.contains("Main"));
    }

    #[test]
    fn test_highlight_go_keywords() {
        let formatter = create_code_formatter().unwrap();
        let line = "func main() { }";
        let result = formatter.highlight_line(line, "go");
        assert!(result.contains("main"));
    }

    #[test]
    fn test_highlight_numbers() {
        let formatter = create_code_formatter().unwrap();
        let line = "The answer is 42 and pi is 3.14";
        let result = formatter.highlight_numbers(line);
        assert!(result.contains("42"));
        assert!(result.contains("3.14"));
    }

    #[test]
    fn test_highlight_numbers_various_formats() {
        let formatter = create_code_formatter().unwrap();
        let line = "Numbers: 0 1 123 456.789 0.5";
        let result = formatter.highlight_numbers(line);
        assert!(result.contains("0"));
        assert!(result.contains("123"));
        assert!(result.contains("456.789"));
    }

    // ============================================================================
    // Code Block Header/Footer Tests
    // ============================================================================

    #[test]
    fn test_build_code_block_header() {
        let formatter = create_code_formatter().unwrap();
        let header = formatter.build_code_block_header("rust");
        assert!(header.contains("RUST"));
        assert!(header.contains("┌"));
        assert!(header.contains("┐"));
    }

    #[test]
    fn test_build_code_block_footer() {
        let formatter = create_code_formatter().unwrap();
        let footer = formatter.build_code_block_footer("rust");
        assert!(footer.contains("└"));
        assert!(footer.contains("┘"));
        assert!(footer.contains("─"));
    }

    #[test]
    fn test_build_code_block_header_long_language() {
        let formatter = create_code_formatter().unwrap();
        let header = formatter.build_code_block_header("typescript");
        assert!(header.contains("TYPESCRIPT"));
    }

    #[test]
    fn test_build_code_block_footer_long_language() {
        let formatter = create_code_formatter().unwrap();
        let footer = formatter.build_code_block_footer("typescript");
        // Footer width should accommodate the language name
        assert!(footer.len() > 10);
    }

    // ============================================================================
    // StreamingResponseFormatter Tests
    // ============================================================================

    #[test]
    fn test_streaming_formatter_creation() {
        let formatter = create_code_formatter().unwrap();
        let _streaming = StreamingResponseFormatter::new(formatter);
    }

    #[test]
    fn test_streaming_formatter_handle_empty_chunk() -> Result<()> {
        let formatter = create_code_formatter()?;
        let mut streaming = StreamingResponseFormatter::new(formatter);
        streaming.handle_chunk("")?;
        Ok(())
    }

    #[test]
    fn test_streaming_formatter_handle_simple_text() -> Result<()> {
        let formatter = create_code_formatter()?;
        let mut streaming = StreamingResponseFormatter::new(formatter);
        streaming.handle_chunk("Hello\n")?;
        streaming.finish()?;
        Ok(())
    }

    #[test]
    fn test_streaming_formatter_handle_code_block() -> Result<()> {
        let formatter = create_code_formatter()?;
        let mut streaming = StreamingResponseFormatter::new(formatter);
        streaming.handle_chunk("```rust\n")?;
        streaming.handle_chunk("fn main() {}\n")?;
        streaming.handle_chunk("```\n")?;
        streaming.finish()?;
        Ok(())
    }

    #[test]
    fn test_streaming_formatter_multiple_chunks() -> Result<()> {
        let formatter = create_code_formatter()?;
        let mut streaming = StreamingResponseFormatter::new(formatter);
        streaming.handle_chunk("First line\n")?;
        streaming.handle_chunk("Second line\n")?;
        streaming.handle_chunk("Third line\n")?;
        streaming.finish()?;
        Ok(())
    }

    #[test]
    fn test_streaming_formatter_partial_line() -> Result<()> {
        let formatter = create_code_formatter()?;
        let mut streaming = StreamingResponseFormatter::new(formatter);
        streaming.handle_chunk("Partial ")?;
        streaming.handle_chunk("line\n")?;
        streaming.finish()?;
        Ok(())
    }

    #[test]
    fn test_streaming_formatter_finish_with_pending() -> Result<()> {
        let formatter = create_code_formatter()?;
        let mut streaming = StreamingResponseFormatter::new(formatter);
        streaming.handle_chunk("No newline")?;
        streaming.finish()?;
        Ok(())
    }

    #[test]
    fn test_streaming_formatter_code_block_with_language() -> Result<()> {
        let formatter = create_code_formatter()?;
        let mut streaming = StreamingResponseFormatter::new(formatter);
        streaming.handle_chunk("```python\n")?;
        streaming.handle_chunk("def test():\n")?;
        streaming.handle_chunk("    pass\n")?;
        streaming.handle_chunk("```\n")?;
        streaming.finish()?;
        Ok(())
    }

    #[test]
    fn test_streaming_formatter_mixed_content() -> Result<()> {
        let formatter = create_code_formatter()?;
        let mut streaming = StreamingResponseFormatter::new(formatter);
        streaming.handle_chunk("Some text\n")?;
        streaming.handle_chunk("```rust\n")?;
        streaming.handle_chunk("fn foo() {}\n")?;
        streaming.handle_chunk("```\n")?;
        streaming.handle_chunk("More text\n")?;
        streaming.finish()?;
        Ok(())
    }

    // ============================================================================
    // Edge Cases and Special Scenarios
    // ============================================================================

    #[test]
    fn test_format_response_with_nested_backticks() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "Code: ```rust\nlet s = \"`test`\";\n```";
        let result = formatter.format_response(input)?;
        assert!(result.contains("test"));
        Ok(())
    }

    #[test]
    fn test_format_response_with_special_characters() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "Special: ```\n<>&\"'\n```";
        let result = formatter.format_response(input)?;
        assert!(result.contains("<>&"));
        Ok(())
    }

    #[test]
    fn test_file_highlighting_with_special_characters() {
        let formatter = create_code_formatter().unwrap();
        let input = "File: @test-file_123.txt";
        let result = formatter.format_input_with_file_highlighting(input);
        assert!(result.contains("@test-file_123.txt"));
    }

    #[test]
    fn test_file_highlighting_multiple_at_symbols() {
        let formatter = create_code_formatter().unwrap();
        let input = "Check @file1.txt @ @file2.txt";
        let result = formatter.format_input_with_file_highlighting(input);
        assert!(result.contains("@file1.txt"));
        assert!(result.contains("@file2.txt"));
    }

    #[test]
    fn test_empty_input() -> Result<()> {
        let formatter = create_code_formatter()?;
        let result = formatter.format_response("")?;
        assert_eq!(result, "");
        Ok(())
    }

    #[test]
    fn test_file_highlighting_empty_input() {
        let formatter = create_code_formatter().unwrap();
        let result = formatter.format_input_with_file_highlighting("");
        assert_eq!(result, "");
    }

    #[test]
    fn test_whitespace_only_input() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "   \n\n   ";
        let result = formatter.format_response(input)?;
        assert!(result.contains("   "));
        Ok(())
    }

    #[test]
    fn test_code_block_with_empty_language() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "```\nplain code\n```";
        let result = formatter.format_response(input)?;
        assert!(result.contains("plain code"));
        Ok(())
    }

    #[test]
    fn test_incomplete_code_block() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "```rust\nfn main() {}";
        let result = formatter.format_response(input)?;
        // Should handle gracefully - incomplete blocks are not formatted
        assert!(result.contains("```rust"));
        Ok(())
    }

    #[test]
    fn test_consecutive_code_blocks() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "```rust\nfn foo() {}\n```\n```python\ndef bar():\n    pass\n```";
        let result = formatter.format_response(input)?;
        assert!(result.contains("fn foo"));
        assert!(result.contains("def bar"));
        Ok(())
    }

    #[test]
    fn test_highlight_line_unknown_language() {
        let formatter = create_code_formatter().unwrap();
        let line = "some random code 123";
        let result = formatter.highlight_line(line, "unknown");
        assert!(result.contains("code"));
        assert!(result.contains("123"));
    }

    #[test]
    fn test_normalize_language_mixed_case() {
        let formatter = create_code_formatter().unwrap();
        // For unknown full names, returns as-is (not normalized unless it's an alias)
        assert_eq!(formatter.normalize_language("RuSt"), "RuSt");
        assert_eq!(formatter.normalize_language("PyThOn"), "PyThOn");
        // But aliases work regardless of case
        assert_eq!(formatter.normalize_language("PY"), "python");
        assert_eq!(formatter.normalize_language("RS"), "rust");
    }

    #[test]
    fn test_text_before_and_after_code_blocks() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "Before\n```rust\ncode\n```\nAfter";
        let result = formatter.format_response(input)?;
        assert!(result.contains("Before"));
        assert!(result.contains("After"));
        assert!(result.contains("code"));
        Ok(())
    }

    #[test]
    fn test_streaming_formatter_empty_code_block() -> Result<()> {
        let formatter = create_code_formatter()?;
        let mut streaming = StreamingResponseFormatter::new(formatter);
        streaming.handle_chunk("```rust\n")?;
        streaming.handle_chunk("```\n")?;
        streaming.finish()?;
        Ok(())
    }

    #[test]
    fn test_streaming_formatter_code_block_without_language() -> Result<()> {
        let formatter = create_code_formatter()?;
        let mut streaming = StreamingResponseFormatter::new(formatter);
        streaming.handle_chunk("```\n")?;
        streaming.handle_chunk("code here\n")?;
        streaming.handle_chunk("```\n")?;
        streaming.finish()?;
        Ok(())
    }

    #[test]
    fn test_highlight_rust_string_with_escapes() {
        let formatter = create_code_formatter().unwrap();
        let line = r#"let s = "Hello \"World\"";"#;
        let result = formatter.highlight_line(line, "rust");
        assert!(result.contains("Hello"));
    }

    #[test]
    fn test_highlight_python_triple_quotes() {
        let formatter = create_code_formatter().unwrap();
        let line = r#""""docstring""""#;
        let result = formatter.highlight_line(line, "python");
        assert!(result.contains("docstring"));
    }

    #[test]
    fn test_highlight_javascript_template_strings() {
        let formatter = create_code_formatter().unwrap();
        let line = "const s = `template ${var}`;";
        let result = formatter.highlight_line(line, "javascript");
        assert!(result.contains("template"));
    }

    #[test]
    fn test_multiple_languages_in_single_response() -> Result<()> {
        let formatter = create_code_formatter()?;
        let input = "Rust:\n```rust\nfn main() {}\n```\nPython:\n```python\ndef main():\n    pass\n```\nJS:\n```javascript\nfunction main() {}\n```";
        let result = formatter.format_response(input)?;
        assert!(result.contains("RUST"));
        assert!(result.contains("PYTHON"));
        assert!(result.contains("JAVASCRIPT"));
        Ok(())
    }

    #[test]
    fn test_file_highlighting_with_dots_and_dashes() {
        let formatter = create_code_formatter().unwrap();
        let input = "Files: @my-file.test.ts and @another_file-v2.json";
        let result = formatter.format_input_with_file_highlighting(input);
        assert!(result.contains("@my-file.test.ts"));
        assert!(result.contains("@another_file-v2.json"));
    }

    #[test]
    fn test_cache_invalidation_on_different_input() {
        let formatter = create_code_formatter().unwrap();

        // Populate cache with first input
        let input1 = "Test @file1.txt";
        let _result1 = formatter.format_input_with_file_highlighting(input1);

        // Different input should not use cached result
        let input2 = "Test @file2.txt";
        let result2 = formatter.format_input_with_file_highlighting(input2);

        assert!(result2.contains("@file2.txt"));
    }

    #[test]
    fn test_format_text_with_file_highlighting_direct() {
        let formatter = create_code_formatter().unwrap();
        let input = "Check @readme.md and @license.txt";
        let result = formatter.format_text_with_file_highlighting(input);
        assert!(result.contains("@readme.md"));
        assert!(result.contains("@license.txt"));
    }

    #[test]
    fn test_code_block_footer_width_calculation() {
        let formatter = create_code_formatter().unwrap();
        let footer_short = formatter.build_code_block_footer("c");
        let footer_long = formatter.build_code_block_footer("typescript");

        // Longer language name should result in longer footer
        assert!(footer_long.len() > footer_short.len());
    }
}
