//! Diff rendering for file edits using the similar crate.
//! Provides colored diff output with +/- indicators and hunk headers.

use colored::Colorize;

/// Format a diff between old and new content with colored output.
/// Returns the formatted diff string.
pub fn format_diff(path: &str, old_content: &str, new_content: &str) -> String {
    let diff = similar::TextDiff::configure()
        .algorithm(similar::Algorithm::Patience)
        .diff_lines(old_content, new_content);

    let mut result = String::new();

    // Add header with file path
    result.push_str(&format!("{} {}", "Changes in".cyan().bold(), path.white().bold()));
    result.push('\n');

    // Process each diff operation
    let old_lines: Vec<&str> = old_content.lines().collect();
    let new_lines: Vec<&str> = new_content.lines().collect();

    for op in diff.ops() {
        match op {
            similar::DiffOp::Equal { old_index: _, new_index: _, len: _ } => {
                // Skip equal ranges in diff output (reduces noise)
            }
            similar::DiffOp::Delete { old_index, old_len, new_index } => {
                // Add hunk header
                let hunk_header = format!("@@ -{},{} +{},{} @@", old_index + 1, old_len, new_index + 1, 0);
                result.push_str(&hunk_header.dimmed().to_string());
                result.push('\n');

                // Show deleted lines
                for i in *old_index..*old_index + *old_len {
                    if let Some(line) = old_lines.get(i) {
                        result.push_str(&format!("- {}", line).red().to_string());
                        result.push('\n');
                    }
                }
            }
            similar::DiffOp::Insert { old_index, new_index, new_len } => {
                // Add hunk header
                let hunk_header = format!("@@ -{},{} +{},{} @@", old_index + 1, 0, new_index + 1, new_len);
                result.push_str(&hunk_header.dimmed().to_string());
                result.push('\n');

                // Show inserted lines
                for i in *new_index..*new_index + *new_len {
                    if let Some(line) = new_lines.get(i) {
                        result.push_str(&format!("+ {}", line).green().to_string());
                        result.push('\n');
                    }
                }
            }
            similar::DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                // Add hunk header
                let hunk_header = format!("@@ -{},{} +{},{} @@", old_index + 1, old_len, new_index + 1, new_len);
                result.push_str(&hunk_header.dimmed().to_string());
                result.push('\n');

                // First show deletions from old content
                for i in *old_index..*old_index + *old_len {
                    if let Some(line) = old_lines.get(i) {
                        result.push_str(&format!("- {}", line).red().to_string());
                        result.push('\n');
                    }
                }

                // Then show insertions from new content
                for i in *new_index..*new_index + *new_len {
                    if let Some(line) = new_lines.get(i) {
                        result.push_str(&format!("+ {}", line).green().to_string());
                        result.push('\n');
                    }
                }
            }
        }

        // Add blank line between hunks for readability
        result.push('\n');
    }

    result
}

/// Create a formatted diff for a file write (Write tool)
pub fn format_write_diff(path: &str, new_content: &str) -> String {
    let mut result = String::new();

    // Add header
    result.push_str(&format!("{} {}", "New file".cyan().bold(), path.white().bold()));
    result.push('\n');
    result.push_str(&format!("{} bytes", new_content.len()).dimmed().to_string());
    result.push('\n');

    // Show a preview of the new content (first few lines)
    let preview_lines: Vec<&str> = new_content.lines().take(10).collect();
    if !preview_lines.is_empty() {
        result.push('\n');
        result.push_str(&"Preview (first 10 lines):".dimmed().to_string());
        result.push('\n');
        for line in preview_lines {
            result.push_str(&format!("+ {}", line).green().to_string());
            result.push('\n');
        }
        if new_content.lines().count() > 10 {
            result.push_str(&format!("+ ... ({} more lines)", new_content.lines().count() - 10).green().dimmed().to_string());
            result.push('\n');
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_diff_basic() {
        let old = "line 1\nline 2\nline 3\n";
        let new = "line 1\nline 2 modified\nline 3\n";
        let result = format_diff("test.txt", old, new);
        assert!(result.contains("test.txt"));
        assert!(result.contains("line 2 modified"));
    }

    #[test]
    fn test_format_diff_insertion() {
        let old = "line 1\nline 3\n";
        let new = "line 1\nline 2\nline 3\n";
        let result = format_diff("test.txt", old, new);
        assert!(result.contains("+"));
        assert!(result.contains("line 2"));
    }

    #[test]
    fn test_format_diff_deletion() {
        let old = "line 1\nline 2\nline 3\n";
        let new = "line 1\nline 3\n";
        let result = format_diff("test.txt", old, new);
        assert!(result.contains("-"));
        assert!(result.contains("line 2"));
    }

    #[test]
    fn test_format_write_diff() {
        let content = "line 1\nline 2\nline 3\n";
        let result = format_write_diff("new_file.txt", content);
        assert!(result.contains("new_file.txt"));
        assert!(result.contains("New file"));
        assert!(result.contains("line 1"));
    }

    #[test]
    fn test_format_diff_empty_content() {
        let old = "";
        let new = "new line\n";
        let result = format_diff("test.txt", old, new);
        assert!(result.contains("test.txt"));
        assert!(result.contains("new line"));
    }

    #[test]
    fn test_format_diff_replacement() {
        let old = "replace me\n";
        let new = "with this\n";
        let result = format_diff("test.txt", old, new);
        assert!(result.contains("-"));
        assert!(result.contains("replace me"));
        assert!(result.contains("+"));
        assert!(result.contains("with this"));
    }
}
