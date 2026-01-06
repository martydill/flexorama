use crossterm::{cursor, style::Print, terminal, ExecutableCommand, QueueableCommand};
use std::fs;
use std::io::Write;
use std::path::Path;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_completion_in_middle() {
        // Test @ completion in the middle of input
        let input = "help me understand @sr";
        let cursor_pos = input.len();
        let result = get_completion(input, cursor_pos);

        // Should complete "@sr" to "@src/" and preserve the rest
        assert!(result.is_some());
        let completed = result.unwrap();
        assert!(completed.starts_with("help me understand @src"));
    }

    #[test]
    fn test_file_completion_with_text_after() {
        // Test @ completion with text after the path
        let input = "check @sr and other stuff";
        let cursor_pos = 9; // Position after "@sr" (not 8)
        let result = get_completion(input, cursor_pos);

        // Should complete "@sr" to "@src/" and preserve " and other stuff"
        assert!(result.is_some());
        let completed = result.unwrap();
        assert!(completed.starts_with("check @src"));
        assert!(completed.contains(" and other stuff"));
    }

    #[test]
    fn test_file_completion_at_beginning() {
        // Test @ completion at beginning (should still work)
        let input = "@sr";
        let cursor_pos = input.len();
        let result = get_completion(input, cursor_pos);

        assert!(result.is_some());
        let completed = result.unwrap();
        assert!(completed.starts_with("@src"));
    }

    #[test]
    fn test_no_completion_without_at() {
        // Test that no completion happens without @
        let input = "help me understand sr";
        let cursor_pos = input.len();
        let result = get_completion(input, cursor_pos);

        assert!(result.is_none());
    }

    #[test]
    fn test_multiple_at_symbols() {
        // Test completion with multiple @ symbols (should complete the last one)
        let input = "check @other/file.rs and @sr";
        let cursor_pos = input.len();
        let result = get_completion(input, cursor_pos);

        // Should complete the last @sr
        assert!(result.is_some());
        let completed = result.unwrap();
        assert!(completed.contains("@src"));
        // Should preserve the first @other/file.rs
        assert!(completed.contains("@other/file.rs"));
    }

    #[test]
    fn test_command_completion_still_works() {
        // Test that command completion still works at the beginning
        let input = "/hel";
        let cursor_pos = input.len();
        let result = get_completion(input, cursor_pos);

        assert!(result.is_some());
        let completed = result.unwrap();
        assert_eq!(completed, "/help");
    }

    #[test]
    fn test_no_command_completion_in_middle() {
        // Test that command completion doesn't trigger in middle of text
        let input = "help me /hel";
        let cursor_pos = input.len();
        let result = get_completion(input, cursor_pos);

        // Should not complete commands in the middle
        assert!(result.is_none());
    }

    #[test]
    fn test_at_command_ignored() {
        // Test that @commands are not completed as file paths
        let input = "check @file-permissions";
        let cursor_pos = input.len();
        let result = get_completion(input, cursor_pos);

        // Should not complete @file-permissions as a file path
        assert!(result.is_none());
    }
}

/// Get completion suggestions based on current input and cursor position
pub fn get_completion(input: &str, cursor_pos: usize) -> Option<String> {
    let input = input.trim_start();

    // Command completions - only if at start of input
    let commands = vec![
        "/help",
        "/stats",
        "/usage",
        "/context",
        "/search",
        "/resume",
        "/clear",
        "/reset-stats",
        "/permissions",
        "/file-permissions",
        "/mcp",
        "/exit",
        "/quit",
    ];

    // Check for @file completion anywhere in the input
    if let Some(completion) = check_file_completion(input, cursor_pos) {
        return Some(completion);
    }

    // Command completion - only if we're at the beginning or the input starts with a command
    if cursor_pos == 0 || input.starts_with('/') {
        for cmd in commands {
            if cmd.starts_with(input) && cmd != input {
                return Some(cmd.to_string());
            }
        }

        // MCP command completions
        if input.starts_with("/mcp ") {
            let mcp_part = &input[5..];
            let mcp_commands = vec![
                "list",
                "add",
                "remove",
                "connect",
                "disconnect",
                "reconnect",
                "tools",
                "connect-all",
                "disconnect-all",
                "test",
                "help",
            ];

            for cmd in mcp_commands {
                if cmd.starts_with(mcp_part) && cmd != mcp_part {
                    return Some(format!("/mcp {}", cmd));
                }
            }
        }

        // Permission command completions
        if input.starts_with("/permissions ") {
            let perm_part = &input[13..];
            let perm_commands = vec![
                "show",
                "list",
                "test",
                "allow",
                "deny",
                "remove-allow",
                "remove-deny",
                "enable",
                "disable",
                "ask-on",
                "ask-off",
                "help",
            ];

            for cmd in perm_commands {
                if cmd.starts_with(perm_part) && cmd != perm_part {
                    return Some(format!("/permissions {}", cmd));
                }
            }
        }

        if input.starts_with("/file-permissions ") {
            let perm_part = &input[18..];
            let perm_commands = vec![
                "show",
                "list",
                "test",
                "enable",
                "disable",
                "ask-on",
                "ask-off",
                "reset-session",
                "help",
            ];

            for cmd in perm_commands {
                if cmd.starts_with(perm_part) && cmd != perm_part {
                    return Some(format!("/file-permissions {}", cmd));
                }
            }
        }
    }

    None
}

/// Check for file completion with @ syntax anywhere in the input
fn check_file_completion(input: &str, cursor_pos: usize) -> Option<String> {
    // Find the last @ symbol before the cursor position
    let input_up_to_cursor = &input[..cursor_pos];

    if let Some(at_pos) = input_up_to_cursor.rfind('@') {
        // Check if this @ is part of a command (like @file-permissions)
        let remaining_after_at = &input_up_to_cursor[at_pos + 1..];
        if remaining_after_at.starts_with("file-permissions")
            || remaining_after_at.starts_with("permissions")
        {
            return None; // Don't complete commands that start with @
        }

        // Extract the path part after @ up to the cursor
        let path_part = &input_up_to_cursor[at_pos + 1..];

        // Find where the path ends (either at cursor or at whitespace)
        let path_end = path_part
            .find(char::is_whitespace)
            .unwrap_or(path_part.len());
        let current_path = &path_part[..path_end];

        // Try to complete the current path
        if let Some(completion) = complete_file_path(current_path) {
            // Reconstruct the full input with the completion
            let before_at = &input[..at_pos];
            let after_path = &input_up_to_cursor[at_pos + 1 + current_path.len()..];
            let after_cursor = &input[cursor_pos..];

            // Combine: before_at + @ + completion + after_path + after_cursor
            Some(format!(
                "{}@{}{}{}",
                before_at, completion, after_path, after_cursor
            ))
        } else {
            None
        }
    } else {
        None
    }
}

/// Complete file paths for @ syntax
fn complete_file_path(path_part: &str) -> Option<String> {
    let (dir_part, file_prefix) = if let Some(last_slash) = path_part.rfind('/') {
        (&path_part[..last_slash], &path_part[last_slash + 1..])
    } else if let Some(last_slash) = path_part.rfind('\\') {
        (&path_part[..last_slash], &path_part[last_slash + 1..])
    } else {
        ("", path_part)
    };

    let search_dir = if dir_part.is_empty() {
        Path::new(".")
    } else {
        Path::new(dir_part)
    };

    if let Ok(entries) = fs::read_dir(search_dir) {
        let mut matches: Vec<String> = entries
            .filter_map(|entry| entry.ok())
            .filter(|entry| {
                let file_name = entry.file_name();
                let file_name_str = file_name.to_string_lossy();
                file_name_str.starts_with(file_prefix)
            })
            .map(|entry| {
                let file_name = entry.file_name().to_string_lossy().to_string();
                if entry.path().is_dir() {
                    format!("{}/", file_name)
                } else {
                    file_name
                }
            })
            .collect();

        matches.sort();

        if let Some(first_match) = matches.first() {
            if matches.len() == 1 {
                // Single match - return it
                let full_path = if dir_part.is_empty() {
                    first_match.clone()
                } else {
                    format!("{}/{}", dir_part, first_match)
                };
                Some(full_path)
            } else {
                // Multiple matches - find common prefix
                let common_prefix = find_common_prefix(&matches);
                let full_path = if dir_part.is_empty() {
                    common_prefix
                } else {
                    format!("{}/{}", dir_part, common_prefix)
                };
                Some(full_path)
            }
        } else {
            None
        }
    } else {
        None
    }
}

/// Find common prefix among multiple strings
fn find_common_prefix(strings: &[String]) -> String {
    if strings.is_empty() {
        return String::new();
    }

    let first = &strings[0];
    let mut end = first.len();

    for s in strings.iter().skip(1) {
        end = end.min(s.len());
        while !first[..end].starts_with(&s[..end]) {
            end -= 1;
        }
    }

    first[..end].to_string()
}

/// Handle tab completion in raw mode
pub fn handle_tab_completion(input: &str, cursor_pos: usize) -> Option<String> {
    if let Some(completion) = get_completion(input, cursor_pos) {
        // Clear current line and show completion
        std::io::stdout()
            .execute(terminal::Clear(terminal::ClearType::CurrentLine))
            .unwrap()
            .execute(cursor::MoveToColumn(0))
            .unwrap()
            .queue(Print("> "))
            .unwrap()
            .queue(Print(&completion))
            .unwrap()
            .flush()
            .unwrap();

        Some(completion)
    } else {
        None
    }
}
