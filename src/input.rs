use anyhow::Result;
use colored::*;
use crossterm::{
    cursor,
    event::{
        self, DisableBracketedPaste, EnableBracketedPaste, Event, KeyCode, KeyEvent, KeyEventKind,
        KeyModifiers,
    },
    terminal, ExecutableCommand, QueueableCommand,
};
use std::cell::Cell;
use std::io::{self, Write};
use std::thread_local;
use std::time::Duration;

use crate::autocomplete;
use crate::formatter;

/// Reverse search state
#[derive(Debug, Clone)]
pub struct ReverseSearchState {
    pub search_query: String,
    pub matched_entry: Option<String>,
    pub current_match_index: usize,
    pub all_matches: Vec<String>,
}

impl ReverseSearchState {
    pub fn new() -> Self {
        Self {
            search_query: String::new(),
            matched_entry: None,
            current_match_index: 0,
            all_matches: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.search_query.clear();
        self.matched_entry = None;
        self.current_match_index = 0;
        self.all_matches.clear();
    }

    pub fn is_active(&self) -> bool {
        !self.search_query.is_empty() || self.matched_entry.is_some()
    }
}

thread_local! {
    static LAST_RENDERED_LINES: Cell<usize> = Cell::new(1);
}

fn disable_raw_mode_and_bracketed_paste() -> Result<()> {
    io::stdout().execute(DisableBracketedPaste)?;
    terminal::disable_raw_mode()?;
    Ok(())
}

/// When bracketed paste is unavailable, a multiline paste arrives as a burst of Key events.
/// If the user hits Enter and more events are queued immediately, treat this as a paste newline
/// instead of submitting and drain the pending events into a single string.
fn drain_queued_input_after_enter() -> Result<Option<String>> {
    // Small window to catch paste bursts
    let mut collected = String::from("\n"); // Include the Enter that triggered this
    let mut has_non_newline = false;
    let mut seen_paste = false;

    // Drain everything currently queued (pastes typically arrive as a burst)
    while event::poll(Duration::from_millis(10))? {
        match event::read()? {
            Event::Paste(pasted) => {
                seen_paste = true;
                if pasted.chars().any(|c| c != '\n' && c != '\r') {
                    has_non_newline = true;
                }
                collected.push_str(&pasted.replace('\r', ""));
            }
            Event::Key(KeyEvent {
                code: KeyCode::Char(c),
                kind: KeyEventKind::Press,
                ..
            }) => {
                if seen_paste {
                    // Some terminals send both Paste and Key events; avoid duplicating content
                    continue;
                }
                if c != '\r' {
                    if c != '\n' {
                        has_non_newline = true;
                    }
                    collected.push(c);
                }
            }
            Event::Key(KeyEvent {
                code: KeyCode::Enter,
                kind: KeyEventKind::Press,
                ..
            }) => {
                if seen_paste {
                    continue;
                }
                collected.push('\n')
            }
            _ => {}
        }
    }

    if has_non_newline {
        Ok(Some(collected))
    } else {
        Ok(None)
    }
}

fn print_final_input(content: &str) {
    if let Some((_, tail)) = content.split_once('\n') {
        // First line is already shown on the prompt; only show the remaining lines
        app_println!();
        if !tail.is_empty() {
            app_println!("{}", tail);
        }
    } else {
        // Single-line: just move to the next line like normal submit
        app_println!();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_input_history() {
        let mut history = InputHistory::new();

        // Test adding entries
        history.add_entry("test1".to_string());
        history.add_entry("test2".to_string());

        assert_eq!(history.entries.len(), 2);
        assert_eq!(history.entries[0], "test1");
        assert_eq!(history.entries[1], "test2");

        // Test navigation
        let current_input = "current";
        let prev = history.navigate_up(current_input);
        assert!(prev.is_some());
        assert_eq!(prev.unwrap(), "test2");

        let next = history.navigate_down();
        assert!(next.is_some());
        assert_eq!(next.unwrap(), current_input);

        // Test that empty entries are not added
        history.add_entry("".to_string());
        assert_eq!(history.entries.len(), 2); // Should not increase

        // Test that duplicates are not added
        history.add_entry("test2".to_string());
        assert_eq!(history.entries.len(), 2); // Should not increase
    }

    #[test]
    fn test_char_boundaries_with_multibyte() {
        let text = "│ a"; // '│' is multibyte
        let cursor_end = normalize_cursor_pos(text, text.len());
        let prev = previous_char_boundary(text, cursor_end);
        let next = next_char_boundary(text, 0);

        assert_eq!(prev, text.len() - 'a'.len_utf8()); // moving left from end lands after the last char
        assert_eq!(next, '│'.len_utf8()); // moving right from start skips the full multibyte char
    }
}

/// Input history management with reverse search support
pub struct InputHistory {
    entries: Vec<String>,
    index: Option<usize>,
    temp_input: String,
    reverse_search: ReverseSearchState,
}

impl InputHistory {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            index: None,
            temp_input: String::new(),
            reverse_search: ReverseSearchState::new(),
        }
    }

    pub fn add_entry(&mut self, entry: String) {
        // Don't add empty entries or duplicates of the last entry
        if entry.trim().is_empty() {
            return;
        }

        if self.entries.is_empty() || self.entries.last() != Some(&entry) {
            self.entries.push(entry);
            // Limit history size to prevent memory issues
            if self.entries.len() > 1000 {
                self.entries.remove(0);
            }
        }

        // Reset navigation state
        self.index = None;
        self.temp_input.clear();
        self.reverse_search.reset();
    }

    pub fn navigate_up(&mut self, current_input: &str) -> Option<String> {
        if self.entries.is_empty() {
            return None;
        }

        match self.index {
            None => {
                // First time pressing up - save current input and go to last entry
                self.temp_input = current_input.to_string();
                self.index = Some(self.entries.len() - 1);
                Some(self.entries[self.entries.len() - 1].clone())
            }
            Some(index) => {
                if index > 0 {
                    // Move to previous entry in history
                    self.index = Some(index - 1);
                    Some(self.entries[index - 1].clone())
                } else {
                    // Already at the oldest entry
                    Some(self.entries[0].clone())
                }
            }
        }
    }

    pub fn navigate_down(&mut self) -> Option<String> {
        match self.index {
            None => None,
            Some(index) => {
                if index < self.entries.len() - 1 {
                    // Move to next entry in history
                    self.index = Some(index + 1);
                    Some(self.entries[index + 1].clone())
                } else {
                    // At the end of history - restore current input
                    self.index = None;
                    Some(self.temp_input.clone())
                }
            }
        }
    }

    pub fn reset_navigation(&mut self) {
        self.index = None;
        self.temp_input.clear();
        self.reverse_search.reset();
    }

    /// Start reverse search mode
    pub fn start_reverse_search(&mut self, current_input: &str) {
        self.temp_input = current_input.to_string();
        self.reverse_search.reset();
        self.reverse_search.search_query = String::new();
        self.index = None;
    }

    /// Update reverse search query and find matches
    pub fn update_reverse_search(&mut self, query: &str) {
        self.reverse_search.search_query = query.to_string();

        if query.is_empty() {
            self.reverse_search.matched_entry = None;
            self.reverse_search.all_matches.clear();
            self.reverse_search.current_match_index = 0;
            return;
        }

        // Find all entries that contain the query (case-insensitive)
        let query_lower = query.to_lowercase();
        self.reverse_search.all_matches = self
            .entries
            .iter()
            .rev() // Start from most recent
            .filter(|entry| entry.to_lowercase().contains(&query_lower))
            .cloned()
            .collect();

        if !self.reverse_search.all_matches.is_empty() {
            self.reverse_search.current_match_index = 0;
            self.reverse_search.matched_entry = Some(self.reverse_search.all_matches[0].clone());
        } else {
            self.reverse_search.matched_entry = None;
        }
    }

    /// Navigate to next match in reverse search
    pub fn reverse_search_next(&mut self) {
        if !self.reverse_search.all_matches.is_empty() {
            self.reverse_search.current_match_index = (self.reverse_search.current_match_index + 1)
                % self.reverse_search.all_matches.len();
            self.reverse_search.matched_entry = Some(
                self.reverse_search.all_matches[self.reverse_search.current_match_index].clone(),
            );
        }
    }

    /// Navigate to previous match in reverse search
    pub fn reverse_search_prev(&mut self) {
        if !self.reverse_search.all_matches.is_empty() {
            self.reverse_search.current_match_index =
                if self.reverse_search.current_match_index == 0 {
                    self.reverse_search.all_matches.len() - 1
                } else {
                    self.reverse_search.current_match_index - 1
                };
            self.reverse_search.matched_entry = Some(
                self.reverse_search.all_matches[self.reverse_search.current_match_index].clone(),
            );
        }
    }

    /// Get the current reverse search state
    pub fn get_reverse_search_state(&self) -> &ReverseSearchState {
        &self.reverse_search
    }

    /// Finish reverse search and return the matched entry
    pub fn finish_reverse_search(&mut self) -> Option<String> {
        let result = self.reverse_search.matched_entry.clone();
        self.reverse_search.reset();
        self.index = None;
        result
    }

    /// Cancel reverse search and restore original input
    pub fn cancel_reverse_search(&mut self) -> String {
        self.reverse_search.reset();
        self.index = None;
        self.temp_input.clone()
    }
}

/// Normalize a cursor position to the nearest valid char boundary within the string.
fn normalize_cursor_pos(text: &str, pos: usize) -> usize {
    if text.is_empty() {
        return 0;
    }

    if pos >= text.len() || text.is_char_boundary(pos) {
        return pos.min(text.len());
    }

    text.char_indices()
        .take_while(|(idx, _)| *idx < pos)
        .map(|(idx, _)| idx)
        .last()
        .unwrap_or(0)
}

/// Find the previous char boundary (moving left) from the given cursor position.
fn previous_char_boundary(text: &str, cursor_pos: usize) -> usize {
    let pos = normalize_cursor_pos(text, cursor_pos);
    if pos == 0 {
        return 0;
    }

    text[..pos]
        .chars()
        .rev()
        .next()
        .map(|c| pos.saturating_sub(c.len_utf8()))
        .unwrap_or(0)
}

/// Find the next char boundary (moving right) from the given cursor position.
fn next_char_boundary(text: &str, cursor_pos: usize) -> usize {
    let pos = normalize_cursor_pos(text, cursor_pos);
    if pos >= text.len() {
        return text.len();
    }

    text[pos..]
        .chars()
        .next()
        .map(|c| pos + c.len_utf8())
        .unwrap_or(pos)
}

/// Read input with autocompletion support, file highlighting, and reverse search
pub fn read_input_with_completion_and_highlighting(
    formatter: Option<&formatter::CodeFormatter>,
    history: &mut InputHistory,
) -> Result<String> {
    // Enable raw mode for keyboard input
    terminal::enable_raw_mode()?;
    io::stdout().execute(EnableBracketedPaste)?;

    let mut input = String::new();
    // Cursor position is tracked as a byte offset that always sits on a char boundary.
    let mut cursor_pos = 0;
    let mut reverse_search_mode = false;

    // Reset render tracking when starting a new prompt
    LAST_RENDERED_LINES.with(|cell| cell.set(1));

    // Clear any previous input and display fresh prompt
    redraw_input_line_with_highlighting(&input, cursor_pos, formatter)?;

    loop {
        match event::read()? {
            Event::Key(KeyEvent {
                code: KeyCode::Char('r'),
                kind: KeyEventKind::Press,
                modifiers: KeyModifiers::CONTROL,
                state: _,
            }) => {
                // Handle Ctrl+R - start reverse search
                if !reverse_search_mode {
                    reverse_search_mode = true;
                    history.start_reverse_search(&input);
                    redraw_reverse_search_prompt(history, formatter)?;
                } else {
                    // If already in reverse search, find next match
                    history.reverse_search_next();
                    redraw_reverse_search_prompt(history, formatter)?;
                }
            }

            Event::Key(KeyEvent {
                code: KeyCode::Char('r'),
                kind: KeyEventKind::Press,
                ..
            }) if reverse_search_mode => {
                // In reverse search mode, 'r' without Ctrl also searches for next match
                history.reverse_search_next();
                redraw_reverse_search_prompt(history, formatter)?;
            }

            Event::Key(KeyEvent {
                code: KeyCode::Up,
                kind: KeyEventKind::Press,
                ..
            }) if reverse_search_mode => {
                // In reverse search mode, up arrow goes to previous match
                history.reverse_search_prev();
                redraw_reverse_search_prompt(history, formatter)?;
            }

            Event::Key(KeyEvent {
                code: KeyCode::Down,
                kind: KeyEventKind::Press,
                ..
            }) if reverse_search_mode => {
                // In reverse search mode, down arrow goes to next match
                history.reverse_search_next();
                redraw_reverse_search_prompt(history, formatter)?;
            }

            Event::Key(KeyEvent {
                code: KeyCode::Enter,
                kind: KeyEventKind::Press,
                ..
            }) if reverse_search_mode => {
                // In reverse search mode, Enter accepts the current match
                if let Some(matched_entry) = history.finish_reverse_search() {
                    input = matched_entry;
                    cursor_pos = input.len();
                }
                reverse_search_mode = false;
                redraw_input_line_with_highlighting(&input, cursor_pos, formatter)?;
            }

            Event::Key(KeyEvent {
                code: KeyCode::Esc,
                kind: KeyEventKind::Press,
                ..
            }) if reverse_search_mode => {
                // In reverse search mode, Esc cancels and restores original input
                input = history.cancel_reverse_search();
                cursor_pos = input.len();
                reverse_search_mode = false;
                redraw_input_line_with_highlighting(&input, cursor_pos, formatter)?;
            }

            Event::Key(KeyEvent {
                code: KeyCode::Backspace,
                kind: KeyEventKind::Press,
                ..
            }) if reverse_search_mode => {
                // In reverse search mode, backspace removes last character from search query
                let state = history.get_reverse_search_state();
                let mut query = state.search_query.clone();
                if !query.is_empty() {
                    query.pop();
                    history.update_reverse_search(&query);
                    redraw_reverse_search_prompt(history, formatter)?;
                }
            }

            Event::Key(KeyEvent {
                code: KeyCode::Char(c),
                kind: KeyEventKind::Press,
                ..
            }) if reverse_search_mode => {
                // In reverse search mode, typing adds to search query
                let state = history.get_reverse_search_state();
                let mut query = state.search_query.clone();
                query.push(c);
                history.update_reverse_search(&query);
                redraw_reverse_search_prompt(history, formatter)?;
            }

            Event::Key(KeyEvent {
                code: KeyCode::Enter,
                kind: KeyEventKind::Press,
                ..
            }) => {
                // If more events are queued immediately, this is likely a paste that includes newlines.
                if let Some(extra) = drain_queued_input_after_enter()? {
                    history.reset_navigation();
                    let extra = extra.replace("\r\n", "\n").replace('\r', "\n");

                    input.insert_str(cursor_pos, &extra);
                    cursor_pos += extra.chars().count();

                    // Finish input immediately to avoid duplicate consumption from stdin
                    let final_input = input.clone();
                    disable_raw_mode_and_bracketed_paste()?;
                    print_final_input(&final_input);
                    return Ok(final_input);
                }

                // Check if this might be the start of multiline input BEFORE disabling raw mode
                let trimmed_input = input.trim();
                if should_start_multiline(trimmed_input) {
                    // Disable raw mode first
                    disable_raw_mode_and_bracketed_paste()?;

                    // Clear the current line completely and move to start
                    io::stdout()
                        .execute(terminal::Clear(terminal::ClearType::CurrentLine))?
                        .execute(cursor::MoveToColumn(0))?
                        .flush()?;

                    // Start multiline input mode with the current input
                    let multiline_result = read_multiline_input(trimmed_input, None); // Don't double-highlight
                    return multiline_result;
                } else {
                    // Normal single line input - add to history
                    let trimmed_input = input.trim().to_string();
                    history.add_entry(trimmed_input.clone());
                    app_println!();
                    disable_raw_mode_and_bracketed_paste()?;
                    return Ok(trimmed_input);
                }
            }

            Event::Key(KeyEvent {
                code: KeyCode::Tab,
                kind: KeyEventKind::Press,
                ..
            }) => {
                // Handle tab completion with cursor position
                if let Some(completion) = autocomplete::handle_tab_completion(&input, cursor_pos) {
                    input = completion;
                    cursor_pos = input.len();

                    // Redraw the line with highlighting after completion
                    redraw_input_line_with_highlighting(&input, cursor_pos, formatter)?;
                }
            }

            Event::Key(KeyEvent {
                code: KeyCode::Backspace,
                kind: KeyEventKind::Press,
                ..
            }) => {
                if !input.is_empty() && cursor_pos > 0 {
                    // Reset history navigation when user edits input
                    history.reset_navigation();

                    let prev_boundary = previous_char_boundary(&input, cursor_pos);
                    input.drain(prev_boundary..cursor_pos);
                    cursor_pos = prev_boundary;

                    // Use fast redraw unless @ symbol is present
                    if input.contains('@') {
                        redraw_input_line_with_highlighting(&input, cursor_pos, formatter)?;
                    } else {
                        redraw_input_line_fast(&input, cursor_pos)?;
                    }
                }
            }

            Event::Key(KeyEvent {
                code: KeyCode::Left,
                kind: KeyEventKind::Press,
                ..
            }) => {
                if cursor_pos > 0 {
                    cursor_pos = previous_char_boundary(&input, cursor_pos);
                    redraw_input_line_fast(&input, cursor_pos)?;
                }
            }

            Event::Key(KeyEvent {
                code: KeyCode::Right,
                kind: KeyEventKind::Press,
                ..
            }) => {
                if cursor_pos < input.len() {
                    cursor_pos = next_char_boundary(&input, cursor_pos);
                    redraw_input_line_fast(&input, cursor_pos)?;
                }
            }

            Event::Key(KeyEvent {
                code: KeyCode::Up,
                kind: KeyEventKind::Press,
                ..
            }) => {
                // Handle up arrow - navigate to previous history entry
                if let Some(new_input) = history.navigate_up(&input) {
                    input = new_input;
                    cursor_pos = input.len();
                    // Use fast redraw for history navigation to avoid regex overhead
                    redraw_input_line_fast(&input, cursor_pos)?;
                }
            }

            Event::Key(KeyEvent {
                code: KeyCode::Down,
                kind: KeyEventKind::Press,
                ..
            }) => {
                // Handle down arrow - navigate to next history entry
                if let Some(new_input) = history.navigate_down() {
                    input = new_input;
                    cursor_pos = input.len();
                    // Use fast redraw for history navigation to avoid regex overhead
                    redraw_input_line_fast(&input, cursor_pos)?;
                }
            }

            Event::Key(KeyEvent {
                code: KeyCode::Char(c),
                kind: KeyEventKind::Press,
                modifiers: KeyModifiers::CONTROL,
                ..
            }) if c == 'c' => {
                // Handle Ctrl+C
                app_println!();
                disable_raw_mode_and_bracketed_paste()?;
                std::process::exit(0);
            }

            Event::Key(KeyEvent {
                code: KeyCode::Esc,
                kind: KeyEventKind::Press,
                ..
            }) => {
                // Handle ESC key - return cancellation signal
                app_println!();
                disable_raw_mode_and_bracketed_paste()?;
                return Err(anyhow::anyhow!("CANCELLED"));
            }

            Event::Paste(pasted) => {
                history.reset_navigation();
                let normalized = pasted.replace("\r\n", "\n").replace('\r', "\n");
                input.insert_str(cursor_pos, &normalized);
                cursor_pos += normalized.chars().count();

                // If the pasted content includes newlines, finish immediately with the full text
                if normalized.contains('\n') {
                    let final_input = input.clone();
                    disable_raw_mode_and_bracketed_paste()?;
                    print_final_input(&final_input);
                    return Ok(final_input);
                }

                // For single-line pastes, redraw appropriately
                if input.contains('@') {
                    redraw_input_line_with_highlighting(&input, cursor_pos, formatter)?;
                } else {
                    redraw_input_line_fast(&input, cursor_pos)?;
                }
            }

            Event::Key(KeyEvent {
                code: KeyCode::Char(c),
                kind: KeyEventKind::Press,
                ..
            }) => {
                // Handle all character input, including spaces
                // Reset history navigation when user starts typing
                history.reset_navigation();

                input.insert(cursor_pos, c);
                cursor_pos += c.len_utf8();

                // Use fast redraw for most typing, only use highlighting when @ symbol is present
                if input.contains('@') {
                    redraw_input_line_with_highlighting(&input, cursor_pos, formatter)?;
                } else {
                    redraw_input_line_fast(&input, cursor_pos)?;
                }
            }

            _ => {}
        }
    }
}

/// Determine if input should start multiline mode
fn should_start_multiline(input: &str) -> bool {
    let trimmed = input.trim();

    // Only start multiline for clear, intentional cases:
    // 1. Input starts with explicit code block marker (```language) but doesn't end with ```
    // 2. Input contains actual newlines
    // 3. Input is an incomplete quoted string with reasonable length (to avoid accidental triggers)

    // Explicit code block start - most reliable multiline indicator
    (trimmed.starts_with("```") && !trimmed.ends_with("```") && trimmed.len() > 3) ||
    // Already contains newlines (shouldn't happen in single line input, but just in case)
    (trimmed.contains('\n')) ||
    // Incomplete quoted strings, but only if they're reasonably long and look intentional
    ((trimmed.starts_with('"') || trimmed.starts_with('\'')) &&
     !trimmed.ends_with('"') && !trimmed.ends_with('\'') &&
     trimmed.len() > 10 && // Only if substantial content
     (trimmed.contains(',') || trimmed.contains('{') || trimmed.contains('('))) // Looks like code/data
}

/// Read multiline input in normal mode
pub fn read_multiline_input(
    initial_line: &str,
    formatter: Option<&formatter::CodeFormatter>,
) -> Result<String> {
    // Start with the first line that was already entered
    let mut lines = vec![initial_line.to_string()];

    // Check if we're in a code block
    let is_code_block = initial_line.trim().starts_with("```");
    let is_quoted = initial_line.trim().starts_with('"') || initial_line.trim().starts_with('\'');

    // If the initial line is complete (not starting a multiline structure), return it immediately
    if !is_code_block && !is_quoted && !initial_line.contains('\n') {
        return Ok(initial_line.to_string());
    }

    loop {
        app_print!("... ");
        std::io::Write::flush(&mut std::io::stdout()).unwrap();

        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) => {
                // EOF - end of input
                break;
            }
            Ok(_) => {
                let line = line.trim_end().to_string();

                // For code blocks, check for ending marker
                if is_code_block && line.trim().ends_with("```") {
                    lines.push(line);
                    break;
                }

                // For quoted strings, check for closing quote
                if is_quoted && line.trim().ends_with('"') {
                    lines.push(line);
                    break;
                }

                // Empty line ends multiline input (unless we're in a code block)
                if line.is_empty() && !is_code_block {
                    break;
                }

                // Add the line and continue
                lines.push(line);
            }
            Err(_) => {
                // Handle EOF or input error gracefully
                app_println!("\n{} End of input", "👋".blue());
                break;
            }
        }
    }

    let final_input = lines.join("\n");

    // Display the complete multiline input with file highlighting if formatter is available
    if let Some(fmt) = formatter {
        let highlighted_input = fmt.format_input_with_file_highlighting(&final_input);
        // Display the multiline input with proper formatting
        let input_lines: Vec<&str> = highlighted_input.lines().collect();
        if input_lines.len() > 1 {
            for (i, line) in input_lines.iter().enumerate() {
                if i == 0 {
                    app_println!("> {}", line);
                } else {
                    app_println!("... {}", line);
                }
            }
        }
    }

    Ok(final_input)
}

fn calculate_line_usage(content_len: usize, prompt_len: usize, terminal_width: usize) -> usize {
    let width = terminal_width.max(1);
    ((prompt_len + content_len).saturating_sub(1) / width) + 1
}

fn clear_previous_render(stdout: &mut impl Write, lines_rendered: usize) -> Result<()> {
    use crossterm::{cursor::MoveToColumn, cursor::MoveUp, terminal::Clear, terminal::ClearType};

    if lines_rendered > 1 {
        let lines_to_clear = lines_rendered.saturating_sub(1).min(u16::MAX as usize) as u16;
        if lines_to_clear > 0 {
            stdout.queue(MoveUp(lines_to_clear))?;
        }
    }
    stdout
        .queue(MoveToColumn(0))?
        .queue(Clear(ClearType::FromCursorDown))?;

    Ok(())
}

fn redraw_input_line(
    input: &str,
    cursor_pos: usize,
    formatter: Option<&formatter::CodeFormatter>,
    use_highlighting: bool,
) -> Result<()> {
    use crossterm::{
        cursor::{MoveToColumn, MoveUp},
        style::{Print, ResetColor},
        terminal,
    };

    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    let terminal_width = terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80)
        .max(1);
    let cursor_pos = normalize_cursor_pos(input, cursor_pos);
    let prompt_length = 2; // "> " length
    let visible_len = input.chars().count();
    let cursor_visible = input[..cursor_pos].chars().count();

    let total_lines = calculate_line_usage(visible_len, prompt_length, terminal_width);
    let cursor_line = (prompt_length + cursor_visible) / terminal_width;
    let cursor_column = (prompt_length + cursor_visible) % terminal_width;

    let previous_lines = LAST_RENDERED_LINES.with(|cell| {
        let prev = cell.get();
        cell.set(total_lines);
        prev.max(1)
    });

    clear_previous_render(&mut stdout, previous_lines)?;

    // Display prompt
    stdout.queue(Print("> "))?;

    if use_highlighting {
        // Only apply highlighting if formatter is available AND input contains @file references
        if let Some(fmt) = formatter {
            if input.contains('@') {
                let highlighted_text = fmt.format_input_with_file_highlighting(input);
                stdout.queue(Print(highlighted_text))?;
            } else {
                stdout.queue(Print(input))?;
            }
        } else {
            stdout.queue(Print(input))?;
        }
    } else {
        stdout.queue(Print(input))?;
    }

    // After printing the full input, we're on the last rendered line.
    // Move up to the correct line and column for the cursor.
    let lines_to_move_up = total_lines
        .saturating_sub(cursor_line + 1)
        .min(u16::MAX as usize);
    if lines_to_move_up > 0 {
        stdout.queue(MoveUp(lines_to_move_up as u16))?;
    }

    stdout
        .queue(MoveToColumn(cursor_column as u16))?
        .queue(ResetColor)?
        .flush()?;

    Ok(())
}

/// Redraw the input line with file highlighting and proper cursor positioning
fn redraw_input_line_with_highlighting(
    input: &str,
    cursor_pos: usize,
    formatter: Option<&formatter::CodeFormatter>,
) -> Result<()> {
    redraw_input_line(input, cursor_pos, formatter, true)
}

/// Fast redraw without highlighting for cursor movements (up/down arrows, etc.)
fn redraw_input_line_fast(input: &str, cursor_pos: usize) -> Result<()> {
    redraw_input_line(input, cursor_pos, None, false)
}

/// Redraw the reverse search prompt
fn redraw_reverse_search_prompt(
    history: &InputHistory,
    _formatter: Option<&formatter::CodeFormatter>,
) -> Result<()> {
    use crossterm::{
        style::{Print, ResetColor},
        terminal,
    };

    let stdout = io::stdout();
    let mut stdout = stdout.lock();

    let _terminal_width = terminal::size()
        .map(|(w, _)| w as usize)
        .unwrap_or(80)
        .max(1);

    let state = history.get_reverse_search_state();
    let _prompt_length = 19; // "(reverse-i-search)`' " length

    // Clear previous lines
    let previous_lines = LAST_RENDERED_LINES.with(|cell| {
        let prev = cell.get();
        cell.set(1);
        prev.max(1)
    });

    clear_previous_render(&mut stdout, previous_lines)?;

    // Display reverse search prompt
    if state.search_query.is_empty() {
        stdout.queue(Print("(reverse-i-search)`' "))?;
    } else if let Some(matched_entry) = &state.matched_entry {
        // Highlight the search query within the matched entry
        let highlighted_match = highlight_search_in_text(matched_entry, &state.search_query);
        stdout.queue(Print("(reverse-i-search)`"))?;
        stdout.queue(Print(state.search_query.yellow().bold()))?;
        stdout.queue(Print("': "))?;

        // Apply highlighting to the matched entry if formatter is available and contains @
        if let Some(fmt) = _formatter {
            if matched_entry.contains('@') {
                let with_file_highlighting =
                    fmt.format_input_with_file_highlighting(&highlighted_match);
                stdout.queue(Print(with_file_highlighting))?;
            } else {
                stdout.queue(Print(highlighted_match))?;
            }
        } else {
            stdout.queue(Print(highlighted_match))?;
        }
    } else {
        stdout.queue(Print("(reverse-i-search)`"))?;
        stdout.queue(Print(state.search_query.yellow().bold()))?;
        stdout.queue(Print("': "))?;
        stdout.queue(Print("(failed)".red()))?;
    }

    stdout.queue(ResetColor)?.flush()?;

    Ok(())
}

/// Highlight the search query within text
fn highlight_search_in_text(text: &str, query: &str) -> String {
    if query.is_empty() {
        return text.to_string();
    }

    let query_lower = query.to_lowercase();
    let mut result = String::new();
    let mut last_end = 0;

    // Find all occurrences of the query (case-insensitive)
    for (start, _part) in text.to_lowercase().match_indices(&query_lower) {
        // Add the part before the match
        result.push_str(&text[last_end..start]);

        // Add the highlighted match
        let match_end = start + query.len();
        result.push_str(&text[start..match_end].yellow().bold().to_string());

        last_end = match_end;
    }

    // Add the remaining part
    result.push_str(&text[last_end..]);

    result
}
