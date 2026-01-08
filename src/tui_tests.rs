use crate::tui::*;

#[cfg(test)]
mod tests {
    use super::*;

    // =============================================================================
    // TextPosition Tests
    // =============================================================================

    #[test]
    fn test_text_position_new() {
        let pos = TextPosition::new(5, 10);
        assert_eq!(pos.line_idx, 5);
        assert_eq!(pos.char_offset, 10);
    }

    #[test]
    fn test_text_position_min_max_same_line() {
        let pos1 = TextPosition::new(5, 10);
        let pos2 = TextPosition::new(5, 20);
        let (min, max) = pos1.min_max(pos2);
        assert_eq!(min.line_idx, 5);
        assert_eq!(min.char_offset, 10);
        assert_eq!(max.line_idx, 5);
        assert_eq!(max.char_offset, 20);
    }

    #[test]
    fn test_text_position_min_max_different_lines() {
        let pos1 = TextPosition::new(5, 10);
        let pos2 = TextPosition::new(3, 20);
        let (min, max) = pos1.min_max(pos2);
        assert_eq!(min.line_idx, 3);
        assert_eq!(min.char_offset, 20);
        assert_eq!(max.line_idx, 5);
        assert_eq!(max.char_offset, 10);
    }

    #[test]
    fn test_text_position_min_max_equal_positions() {
        let pos1 = TextPosition::new(5, 10);
        let pos2 = TextPosition::new(5, 10);
        let (min, max) = pos1.min_max(pos2);
        assert_eq!(min.line_idx, 5);
        assert_eq!(min.char_offset, 10);
        assert_eq!(max.line_idx, 5);
        assert_eq!(max.char_offset, 10);
    }

    #[test]
    fn test_text_position_min_max_reversed() {
        let pos1 = TextPosition::new(3, 20);
        let pos2 = TextPosition::new(5, 10);
        let (min, max) = pos2.min_max(pos1);
        assert_eq!(min.line_idx, 3);
        assert_eq!(min.char_offset, 20);
        assert_eq!(max.line_idx, 5);
        assert_eq!(max.char_offset, 10);
    }

    // =============================================================================
    // OutputBuffer Tests
    // =============================================================================

    #[test]
    fn test_output_buffer_new() {
        let buffer = OutputBuffer::new(100);
        assert_eq!(buffer.max_lines, 100);
        assert_eq!(buffer.lines.len(), 1);
        assert_eq!(buffer.lines[0], "");
    }

    #[test]
    fn test_output_buffer_push_text_simple() {
        let mut buffer = OutputBuffer::new(100);
        buffer.push_text("Hello, World!");
        assert_eq!(buffer.lines.len(), 1);
        assert_eq!(buffer.lines[0], "Hello, World!");
    }

    #[test]
    fn test_output_buffer_push_text_empty() {
        let mut buffer = OutputBuffer::new(100);
        buffer.push_text("");
        assert_eq!(buffer.lines.len(), 1);
        assert_eq!(buffer.lines[0], "");
    }

    #[test]
    fn test_output_buffer_push_text_with_newline() {
        let mut buffer = OutputBuffer::new(100);
        buffer.push_text("Line 1\nLine 2\nLine 3");
        assert_eq!(buffer.lines.len(), 3);
        assert_eq!(buffer.lines[0], "Line 1");
        assert_eq!(buffer.lines[1], "Line 2");
        assert_eq!(buffer.lines[2], "Line 3");
    }

    #[test]
    fn test_output_buffer_push_text_with_crlf() {
        let mut buffer = OutputBuffer::new(100);
        buffer.push_text("Line 1\r\nLine 2\r\nLine 3");
        assert_eq!(buffer.lines.len(), 3);
        assert_eq!(buffer.lines[0], "Line 1");
        assert_eq!(buffer.lines[1], "Line 2");
        assert_eq!(buffer.lines[2], "Line 3");
    }

    #[test]
    fn test_output_buffer_push_text_with_cr() {
        let mut buffer = OutputBuffer::new(100);
        buffer.push_text("Line 1\rLine 2\rLine 3");
        assert_eq!(buffer.lines.len(), 3);
        assert_eq!(buffer.lines[0], "Line 1");
        assert_eq!(buffer.lines[1], "Line 2");
        assert_eq!(buffer.lines[2], "Line 3");
    }

    #[test]
    fn test_output_buffer_push_text_multiple_calls() {
        let mut buffer = OutputBuffer::new(100);
        buffer.push_text("First");
        buffer.push_text(" Second");
        buffer.push_text("\nThird");
        assert_eq!(buffer.lines.len(), 2);
        assert_eq!(buffer.lines[0], "First Second");
        assert_eq!(buffer.lines[1], "Third");
    }

    #[test]
    fn test_output_buffer_push_text_max_lines_limit() {
        let mut buffer = OutputBuffer::new(3);
        buffer.push_text("Line 1\nLine 2\nLine 3\nLine 4\nLine 5");
        assert_eq!(buffer.lines.len(), 3);
        assert_eq!(buffer.lines[0], "Line 3");
        assert_eq!(buffer.lines[1], "Line 4");
        assert_eq!(buffer.lines[2], "Line 5");
    }

    #[test]
    fn test_output_buffer_push_text_trailing_newline() {
        let mut buffer = OutputBuffer::new(100);
        buffer.push_text("Line 1\n");
        assert_eq!(buffer.lines.len(), 2);
        assert_eq!(buffer.lines[0], "Line 1");
        assert_eq!(buffer.lines[1], "");
    }

    #[test]
    fn test_output_buffer_push_text_leading_newline() {
        let mut buffer = OutputBuffer::new(100);
        buffer.push_text("\nLine 1");
        assert_eq!(buffer.lines.len(), 2);
        assert_eq!(buffer.lines[0], "");
        assert_eq!(buffer.lines[1], "Line 1");
    }

    // =============================================================================
    // strip_ansi_codes Tests
    // =============================================================================

    #[test]
    fn test_strip_ansi_codes_no_ansi() {
        let input = "Hello, World!";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "Hello, World!");
    }

    #[test]
    fn test_strip_ansi_codes_with_color() {
        let input = "\x1b[31mRed Text\x1b[0m";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "Red Text");
    }

    #[test]
    fn test_strip_ansi_codes_multiple_colors() {
        let input = "\x1b[31mRed\x1b[0m \x1b[32mGreen\x1b[0m \x1b[34mBlue\x1b[0m";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "Red Green Blue");
    }

    #[test]
    fn test_strip_ansi_codes_empty_string() {
        let input = "";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "");
    }

    #[test]
    fn test_strip_ansi_codes_only_ansi() {
        let input = "\x1b[31m\x1b[0m";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "");
    }

    #[test]
    fn test_strip_ansi_codes_bold_and_color() {
        let input = "\x1b[1m\x1b[31mBold Red\x1b[0m";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "Bold Red");
    }

    // =============================================================================
    // screen_col_to_char_offset Tests
    // =============================================================================

    #[test]
    fn test_screen_col_to_char_offset_no_ansi() {
        let line = "Hello, World!";
        assert_eq!(screen_col_to_char_offset(line, 0), 0);
        assert_eq!(screen_col_to_char_offset(line, 5), 5);
        assert_eq!(screen_col_to_char_offset(line, 13), 13);
    }

    #[test]
    fn test_screen_col_to_char_offset_with_ansi() {
        let line = "\x1b[31mRed\x1b[0m Text";
        // Screen position 0 should map to char 0 (before 'R')
        assert_eq!(screen_col_to_char_offset(line, 0), 0);
        // Screen position 3 should map to char 3 (before ' ')
        assert_eq!(screen_col_to_char_offset(line, 3), 3);
        // Screen position 7 should map to char 7 (end of 'Text')
        assert_eq!(screen_col_to_char_offset(line, 7), 7);
    }

    #[test]
    fn test_screen_col_to_char_offset_multiple_ansi() {
        let line = "\x1b[31mR\x1b[0m\x1b[32mG\x1b[0m\x1b[34mB\x1b[0m";
        assert_eq!(screen_col_to_char_offset(line, 0), 0);
        assert_eq!(screen_col_to_char_offset(line, 1), 1);
        assert_eq!(screen_col_to_char_offset(line, 2), 2);
        assert_eq!(screen_col_to_char_offset(line, 3), 3);
    }

    // =============================================================================
    // wrap_ansi_line Tests
    // =============================================================================

    #[test]
    fn test_wrap_ansi_line_no_wrap() {
        let line = "Hello";
        let result = wrap_ansi_line(line, 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "Hello");
    }

    #[test]
    fn test_wrap_ansi_line_exact_width() {
        let line = "Hello";
        let result = wrap_ansi_line(line, 5);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "Hello");
    }

    #[test]
    fn test_wrap_ansi_line_needs_wrap() {
        let line = "HelloWorld";
        let result = wrap_ansi_line(line, 5);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "Hello");
        assert_eq!(result[1], "World");
    }

    #[test]
    fn test_wrap_ansi_line_multiple_wraps() {
        let line = "HelloWorldFooBar";
        let result = wrap_ansi_line(line, 5);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], "Hello");
        assert_eq!(result[1], "World");
        assert_eq!(result[2], "FooBa");
        assert_eq!(result[3], "r");
    }

    #[test]
    fn test_wrap_ansi_line_with_ansi_codes() {
        let line = "\x1b[31mHelloWorld\x1b[0m";
        let result = wrap_ansi_line(line, 5);
        // The closing ANSI code adds a third segment since it comes after the text
        assert_eq!(result.len(), 3);
        // First segment should contain ANSI codes and "Hello"
        assert!(result[0].contains("Hello"));
        // Second segment should contain "World"
        assert!(result[1].contains("World"));
    }

    #[test]
    fn test_wrap_ansi_line_zero_width() {
        let line = "Hello";
        let result = wrap_ansi_line(line, 0);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "");
    }

    #[test]
    fn test_wrap_ansi_line_empty_string() {
        let line = "";
        let result = wrap_ansi_line(line, 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "");
    }

    // =============================================================================
    // normalize_cursor_pos Tests
    // =============================================================================

    #[test]
    fn test_normalize_cursor_pos_valid() {
        let text = "Hello";
        assert_eq!(normalize_cursor_pos(text, 0), 0);
        assert_eq!(normalize_cursor_pos(text, 3), 3);
        assert_eq!(normalize_cursor_pos(text, 5), 5);
    }

    #[test]
    fn test_normalize_cursor_pos_beyond_end() {
        let text = "Hello";
        assert_eq!(normalize_cursor_pos(text, 10), 5);
    }

    #[test]
    fn test_normalize_cursor_pos_utf8() {
        let text = "Hello 世界";
        // Valid positions
        assert_eq!(normalize_cursor_pos(text, 0), 0);
        assert_eq!(normalize_cursor_pos(text, 6), 6);
        // Position on UTF-8 character boundary
        assert_eq!(normalize_cursor_pos(text, 9), 9);
    }

    #[test]
    fn test_normalize_cursor_pos_empty_string() {
        let text = "";
        assert_eq!(normalize_cursor_pos(text, 0), 0);
        assert_eq!(normalize_cursor_pos(text, 5), 0);
    }

    // =============================================================================
    // previous_char_boundary Tests
    // =============================================================================

    #[test]
    fn test_previous_char_boundary_ascii() {
        let text = "Hello";
        assert_eq!(previous_char_boundary(text, 5), 4);
        assert_eq!(previous_char_boundary(text, 3), 2);
        assert_eq!(previous_char_boundary(text, 1), 0);
        assert_eq!(previous_char_boundary(text, 0), 0);
    }

    #[test]
    fn test_previous_char_boundary_utf8() {
        let text = "世界";
        // Each Chinese character is 3 bytes
        assert_eq!(previous_char_boundary(text, 6), 3);
        assert_eq!(previous_char_boundary(text, 3), 0);
        assert_eq!(previous_char_boundary(text, 0), 0);
    }

    #[test]
    fn test_previous_char_boundary_mixed() {
        let text = "Hello世界";
        assert_eq!(previous_char_boundary(text, 11), 8);
        assert_eq!(previous_char_boundary(text, 8), 5);
        assert_eq!(previous_char_boundary(text, 5), 4);
    }

    #[test]
    fn test_previous_char_boundary_empty() {
        let text = "";
        assert_eq!(previous_char_boundary(text, 0), 0);
    }

    // =============================================================================
    // next_char_boundary Tests
    // =============================================================================

    #[test]
    fn test_next_char_boundary_ascii() {
        let text = "Hello";
        assert_eq!(next_char_boundary(text, 0), 1);
        assert_eq!(next_char_boundary(text, 2), 3);
        assert_eq!(next_char_boundary(text, 4), 5);
        assert_eq!(next_char_boundary(text, 5), 5);
    }

    #[test]
    fn test_next_char_boundary_utf8() {
        let text = "世界";
        // Each Chinese character is 3 bytes
        assert_eq!(next_char_boundary(text, 0), 3);
        assert_eq!(next_char_boundary(text, 3), 6);
        assert_eq!(next_char_boundary(text, 6), 6);
    }

    #[test]
    fn test_next_char_boundary_mixed() {
        let text = "Hello世界";
        assert_eq!(next_char_boundary(text, 0), 1);
        assert_eq!(next_char_boundary(text, 4), 5);
        assert_eq!(next_char_boundary(text, 5), 8);
        assert_eq!(next_char_boundary(text, 8), 11);
        assert_eq!(next_char_boundary(text, 11), 11);
    }

    #[test]
    fn test_next_char_boundary_empty() {
        let text = "";
        assert_eq!(next_char_boundary(text, 0), 0);
    }

    // =============================================================================
    // cursor_position_in_lines Tests
    // =============================================================================

    #[test]
    fn test_cursor_position_in_lines_single_line() {
        let text = "Hello, World!";
        assert_eq!(cursor_position_in_lines(text, 0), (0, 0));
        assert_eq!(cursor_position_in_lines(text, 5), (0, 5));
        assert_eq!(cursor_position_in_lines(text, 13), (0, 13));
    }

    #[test]
    fn test_cursor_position_in_lines_multiple_lines() {
        let text = "Line 1\nLine 2\nLine 3";
        assert_eq!(cursor_position_in_lines(text, 0), (0, 0));
        assert_eq!(cursor_position_in_lines(text, 6), (0, 6));
        assert_eq!(cursor_position_in_lines(text, 7), (1, 0));
        assert_eq!(cursor_position_in_lines(text, 13), (1, 6));
        assert_eq!(cursor_position_in_lines(text, 14), (2, 0));
    }

    #[test]
    fn test_cursor_position_in_lines_at_newline() {
        let text = "Line 1\nLine 2";
        assert_eq!(cursor_position_in_lines(text, 6), (0, 6));
        assert_eq!(cursor_position_in_lines(text, 7), (1, 0));
    }

    #[test]
    fn test_cursor_position_in_lines_empty_lines() {
        let text = "\n\n";
        assert_eq!(cursor_position_in_lines(text, 0), (0, 0));
        assert_eq!(cursor_position_in_lines(text, 1), (1, 0));
        assert_eq!(cursor_position_in_lines(text, 2), (2, 0));
    }

    #[test]
    fn test_cursor_position_in_lines_beyond_end() {
        let text = "Hello";
        assert_eq!(cursor_position_in_lines(text, 100), (0, 5));
    }

    #[test]
    fn test_cursor_position_in_lines_empty_string() {
        let text = "";
        assert_eq!(cursor_position_in_lines(text, 0), (0, 0));
        assert_eq!(cursor_position_in_lines(text, 5), (0, 0));
    }

    // =============================================================================
    // build_output_lines Tests
    // =============================================================================

    #[test]
    fn test_build_output_lines_no_wrap() {
        let lines = vec!["Hello".to_string(), "World".to_string()];
        let result = build_output_lines(&lines, 20);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "Hello");
        assert_eq!(result[1], "World");
    }

    #[test]
    fn test_build_output_lines_with_wrap() {
        let lines = vec!["HelloWorld".to_string()];
        let result = build_output_lines(&lines, 5);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "Hello");
        assert_eq!(result[1], "World");
    }

    #[test]
    fn test_build_output_lines_empty_input() {
        let lines: Vec<String> = vec![];
        let result = build_output_lines(&lines, 20);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "");
    }

    #[test]
    fn test_build_output_lines_multiple_with_wrap() {
        let lines = vec!["12345678".to_string(), "ABCDEFGH".to_string()];
        let result = build_output_lines(&lines, 4);
        assert_eq!(result.len(), 4);
        assert_eq!(result[0], "1234");
        assert_eq!(result[1], "5678");
        assert_eq!(result[2], "ABCD");
        assert_eq!(result[3], "EFGH");
    }

    #[test]
    fn test_build_output_lines_zero_width() {
        let lines = vec!["Hello".to_string()];
        let result = build_output_lines(&lines, 0);
        // With zero width, max(1) makes it 1, so "Hello" wraps to 5 lines (one char each)
        assert_eq!(result.len(), 5);
    }

    // =============================================================================
    // build_queue_lines Tests
    // =============================================================================

    #[test]
    fn test_build_queue_lines_simple() {
        let queue = vec!["First".to_string(), "Second".to_string()];
        let result = build_queue_lines(&queue, 20, 10);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "1) First");
        assert_eq!(result[1], "2) Second");
    }

    #[test]
    fn test_build_queue_lines_empty_queue() {
        let queue: Vec<String> = vec![];
        let result = build_queue_lines(&queue, 20, 10);
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_build_queue_lines_zero_width() {
        let queue = vec!["First".to_string()];
        let result = build_queue_lines(&queue, 0, 10);
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_build_queue_lines_zero_max_lines() {
        let queue = vec!["First".to_string()];
        let result = build_queue_lines(&queue, 20, 0);
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_build_queue_lines_with_wrap() {
        let queue = vec!["VeryLongItemThatNeedsWrapping".to_string()];
        let result = build_queue_lines(&queue, 10, 10);
        assert!(result.len() > 1);
        assert!(result[0].starts_with("1) "));
    }

    #[test]
    fn test_build_queue_lines_truncation() {
        let queue = vec![
            "Item 1".to_string(),
            "Item 2".to_string(),
            "Item 3".to_string(),
            "Item 4".to_string(),
            "Item 5".to_string(),
        ];
        let result = build_queue_lines(&queue, 20, 3);
        assert_eq!(result.len(), 3);
        // Should render first 3 items without truncation message
        // because truncation only happens when max_lines is hit during wrapping
        assert_eq!(result[0], "1) Item 1");
        assert_eq!(result[1], "2) Item 2");
        assert_eq!(result[2], "3) Item 3");
    }

    #[test]
    fn test_build_queue_lines_double_digit_numbering() {
        let queue: Vec<String> = (1..=15).map(|i| format!("Item {}", i)).collect();
        let result = build_queue_lines(&queue, 30, 20);
        assert!(result[9].starts_with("10) "));
        assert!(result[14].starts_with("15) "));
    }

    // =============================================================================
    // build_input_layout Tests
    // =============================================================================

    #[test]
    fn test_build_input_layout_empty() {
        let snapshot = TuiSnapshot {
            output_lines: vec![],
            queued: vec![],
            input_display: String::new(),
            input_raw: String::new(),
            cursor_pos: 0,
            output_scroll: 0,
            selection_range: None,
        };
        let layout = build_input_layout(&snapshot, 80);
        assert_eq!(layout.lines.len(), 1);
        assert_eq!(layout.lines[0], "> ");
        assert_eq!(layout.cursor_row, 0);
        assert_eq!(layout.cursor_col, 2);
    }

    #[test]
    fn test_build_input_layout_simple_input() {
        let snapshot = TuiSnapshot {
            output_lines: vec![],
            queued: vec![],
            input_display: "Hello".to_string(),
            input_raw: "Hello".to_string(),
            cursor_pos: 5,
            output_scroll: 0,
            selection_range: None,
        };
        let layout = build_input_layout(&snapshot, 80);
        assert_eq!(layout.lines.len(), 1);
        assert_eq!(layout.lines[0], "> Hello");
        assert_eq!(layout.cursor_row, 0);
        assert_eq!(layout.cursor_col, 7);
    }

    #[test]
    fn test_build_input_layout_multiline() {
        let snapshot = TuiSnapshot {
            output_lines: vec![],
            queued: vec![],
            input_display: "Line 1\nLine 2".to_string(),
            input_raw: "Line 1\nLine 2".to_string(),
            cursor_pos: 13,
            output_scroll: 0,
            selection_range: None,
        };
        let layout = build_input_layout(&snapshot, 80);
        assert_eq!(layout.lines.len(), 2);
        assert_eq!(layout.lines[0], "> Line 1");
        assert_eq!(layout.lines[1], "... Line 2");
    }

    #[test]
    fn test_build_input_layout_cursor_beginning() {
        let snapshot = TuiSnapshot {
            output_lines: vec![],
            queued: vec![],
            input_display: "Hello".to_string(),
            input_raw: "Hello".to_string(),
            cursor_pos: 0,
            output_scroll: 0,
            selection_range: None,
        };
        let layout = build_input_layout(&snapshot, 80);
        assert_eq!(layout.cursor_row, 0);
        assert_eq!(layout.cursor_col, 2);
    }

    #[test]
    fn test_build_input_layout_cursor_middle() {
        let snapshot = TuiSnapshot {
            output_lines: vec![],
            queued: vec![],
            input_display: "Hello".to_string(),
            input_raw: "Hello".to_string(),
            cursor_pos: 3,
            output_scroll: 0,
            selection_range: None,
        };
        let layout = build_input_layout(&snapshot, 80);
        assert_eq!(layout.cursor_row, 0);
        assert_eq!(layout.cursor_col, 5);
    }

    #[test]
    fn test_build_input_layout_with_wrapping() {
        let snapshot = TuiSnapshot {
            output_lines: vec![],
            queued: vec![],
            input_display: "A".repeat(100),
            input_raw: "A".repeat(100),
            cursor_pos: 50,
            output_scroll: 0,
            selection_range: None,
        };
        let layout = build_input_layout(&snapshot, 40);
        // Should have multiple lines due to wrapping
        assert!(layout.lines.len() > 1);
    }

    // =============================================================================
    // Integration Tests
    // =============================================================================

    #[test]
    fn test_output_buffer_and_build_output_lines_integration() {
        let mut buffer = OutputBuffer::new(100);
        buffer.push_text("Line 1\nLine 2\nLine 3");
        let lines = build_output_lines(&buffer.lines, 80);
        assert_eq!(lines.len(), 3);
        assert_eq!(lines[0], "Line 1");
        assert_eq!(lines[1], "Line 2");
        assert_eq!(lines[2], "Line 3");
    }

    #[test]
    fn test_strip_ansi_and_screen_col_integration() {
        let ansi_line = "\x1b[31mRed Text\x1b[0m Normal";
        let stripped = strip_ansi_codes(ansi_line);
        assert_eq!(stripped, "Red Text Normal");

        // Screen position should map correctly
        let offset = screen_col_to_char_offset(ansi_line, 4);
        assert_eq!(offset, 4); // Should be at 'T' in "Text"
    }

    #[test]
    fn test_cursor_navigation_boundaries() {
        let text = "Hello World";
        let mut pos = 11; // End of string

        // Move backward
        pos = previous_char_boundary(text, pos);
        assert_eq!(pos, 10);

        // Move forward
        pos = next_char_boundary(text, pos);
        assert_eq!(pos, 11);

        // Try to move beyond end
        pos = next_char_boundary(text, pos);
        assert_eq!(pos, 11);
    }

    #[test]
    fn test_utf8_handling_comprehensive() {
        let text = "Hello 世界 World";

        // Test normalization
        assert_eq!(normalize_cursor_pos(text, 0), 0);
        assert_eq!(normalize_cursor_pos(text, 100), text.len());

        // Test navigation
        let pos = 6; // Start of first Chinese character
        let next = next_char_boundary(text, pos);
        assert_eq!(next, 9); // Should skip the 3-byte character

        let prev = previous_char_boundary(text, next);
        assert_eq!(prev, 6); // Should go back to start of character
    }

    #[test]
    fn test_wrap_and_build_lines_integration() {
        let line = "HelloWorld";
        let wrapped = wrap_ansi_line(line, 5);
        assert_eq!(wrapped.len(), 2);

        let built = build_output_lines(&vec![line.to_string()], 5);
        assert_eq!(built.len(), 2);
        assert_eq!(built[0], "Hello");
        assert_eq!(built[1], "World");
    }
}
