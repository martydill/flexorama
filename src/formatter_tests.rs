use crate::formatter::create_code_formatter;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_highlighting() {
        let formatter = create_code_formatter().unwrap();

        // Test single file reference
        let input = "Please read @test_file.txt";
        let highlighted = formatter.format_input_with_file_highlighting(input);
        assert!(highlighted.contains("@test_file.txt"));

        // Test multiple file references
        let input = "Compare @file1.txt and @file2.txt";
        let highlighted = formatter.format_input_with_file_highlighting(input);
        assert!(highlighted.contains("@file1.txt"));
        assert!(highlighted.contains("@file2.txt"));

        // Test no file references
        let input = "Just a regular message without files";
        let highlighted = formatter.format_input_with_file_highlighting(input);
        assert_eq!(input, highlighted);

        // Test file reference with path
        let input = "Check @src/main.rs for details";
        let highlighted = formatter.format_input_with_file_highlighting(input);
        assert!(highlighted.contains("@src/main.rs"));
    }
}
