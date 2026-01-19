use crate::formatter;
use crate::input::InputHistory;
use crate::output::{self, OutputSink};
use crate::security::PermissionPrompt;
use ansi_to_tui::IntoText;
use anyhow::Result;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    },
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph},
    Terminal,
};
use std::collections::VecDeque;
use std::io;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MIN_OUTPUT_HEIGHT: usize = 3;
const INPUT_PREFIX_MAIN: &str = "> ";
const INPUT_PREFIX_CONT: &str = "... ";
const RENDER_INTERVAL: Duration = Duration::from_millis(50);

pub struct Tui {
    state: Arc<Mutex<TuiState>>,
    screen: Arc<Mutex<TuiScreen>>,
    formatter: Arc<Mutex<formatter::CodeFormatter>>,
}

struct TuiState {
    output: OutputBuffer,
    input: String,
    cursor_pos: usize,
    history: InputHistory,
    reverse_search_mode: bool,
    queued: VecDeque<String>,
    output_dirty: bool,
    last_render: Instant,
    output_scroll: usize,
    // Selection tracking
    selection_start: Option<TextPosition>,
    selection_end: Option<TextPosition>,
    selection_active: bool,
    // Todo tracking
    todos: Vec<crate::tools::create_todo::TodoItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TextPosition {
    line_idx: usize,
    char_offset: usize,
}

impl TextPosition {
    fn new(line_idx: usize, char_offset: usize) -> Self {
        Self {
            line_idx,
            char_offset,
        }
    }

    fn min_max(self, other: Self) -> (Self, Self) {
        if self.line_idx < other.line_idx
            || (self.line_idx == other.line_idx && self.char_offset <= other.char_offset)
        {
            (self, other)
        } else {
            (other, self)
        }
    }
}

struct OutputBuffer {
    lines: Vec<String>,
    max_lines: usize,
}

struct TuiScreen {
    terminal: Terminal<CrosstermBackend<io::Stdout>>,
}

pub struct TuiSnapshot {
    output_lines: Vec<String>,
    queued: Vec<String>,
    input_display: String,
    input_raw: String,
    cursor_pos: usize,
    output_scroll: usize,
    selection_range: Option<(TextPosition, TextPosition)>,
    todos: Vec<crate::tools::create_todo::TodoItem>,
}

pub enum InputResult {
    Submitted(String),
    Cancelled,
    Exit,
}

struct TuiOutputSink {
    state: Arc<Mutex<TuiState>>,
    screen: Arc<Mutex<TuiScreen>>,
    formatter: Arc<Mutex<formatter::CodeFormatter>>,
}

impl OutputBuffer {
    fn new(max_lines: usize) -> Self {
        Self {
            lines: vec![String::new()],
            max_lines,
        }
    }

    fn push_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }

        let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
        let mut iter = normalized.split('\n').peekable();

        if let Some(first) = iter.next() {
            if self.lines.is_empty() {
                self.lines.push(String::new());
            }
            if let Some(last) = self.lines.last_mut() {
                last.push_str(first);
            }
        }

        for part in iter {
            self.lines.push(part.to_string());
        }

        if self.lines.len() > self.max_lines {
            let overflow = self.lines.len() - self.max_lines;
            self.lines.drain(0..overflow);
        }
    }
}

impl Tui {
    pub fn new(formatter: &formatter::CodeFormatter) -> Result<Self> {
        let mut stdout = io::stdout();
        terminal::enable_raw_mode()?;
        stdout.execute(EnterAlternateScreen)?;
        stdout.execute(EnableBracketedPaste)?;
        stdout.execute(EnableMouseCapture)?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;
        terminal.clear()?;
        terminal.show_cursor()?;

        let state = Arc::new(Mutex::new(TuiState {
            output: OutputBuffer::new(2000),
            input: String::new(),
            cursor_pos: 0,
            history: InputHistory::new(),
            reverse_search_mode: false,
            queued: VecDeque::new(),
            output_dirty: true,
            last_render: Instant::now(),
            output_scroll: 0,
            selection_start: None,
            selection_end: None,
            selection_active: false,
            todos: Vec::new(),
        }));

        let screen = Arc::new(Mutex::new(TuiScreen { terminal }));

        Ok(Self {
            state,
            screen,
            formatter: Arc::new(Mutex::new(formatter.clone())),
        })
    }

    pub fn output_sink(&self) -> Arc<dyn OutputSink> {
        Arc::new(TuiOutputSink {
            state: Arc::clone(&self.state),
            screen: Arc::clone(&self.screen),
            formatter: Arc::clone(&self.formatter),
        })
    }

    pub fn render(&self) -> Result<()> {
        let (snapshot, full_redraw) = {
            let mut guard = self.state.lock().expect("tui state lock");
            let formatter = self.formatter.lock().expect("tui formatter lock");
            let full = guard.output_dirty;
            guard.output_dirty = false;
            (guard.snapshot(&formatter), full)
        };
        if full_redraw {
            self.render_snapshot(&snapshot)
        } else {
            self.render_input_only(&snapshot)
        }
    }

    fn render_snapshot(&self, snapshot: &TuiSnapshot) -> Result<()> {
        let mut screen = self.screen.lock().expect("tui screen lock");
        screen.render_full(snapshot)
    }

    fn render_input_only(&self, snapshot: &TuiSnapshot) -> Result<()> {
        let mut screen = self.screen.lock().expect("tui screen lock");
        screen.render_input_only(snapshot)
    }

    pub fn read_input(&self) -> Result<InputResult> {
        loop {
            // Handle poll errors gracefully - on Windows, rapid clicking can cause transient errors
            let has_event = match event::poll(Duration::from_millis(50)) {
                Ok(ready) => ready,
                Err(_) => continue,
            };

            if has_event {
                // Handle read errors gracefully as well
                let evt = match event::read() {
                    Ok(e) => e,
                    Err(_) => continue,
                };

                match evt {
                    Event::Key(key_event) => {
                        if let Some(result) = self.handle_key_event(key_event)? {
                            return Ok(result);
                        }
                    }
                    Event::Paste(pasted) => {
                        self.handle_paste(&pasted)?;
                    }
                    Event::Mouse(mouse_event) => {
                        // Ignore mouse event errors - they're non-critical
                        let _ = self.handle_mouse_event(mouse_event);
                    }
                    Event::Resize(_, _) => {
                        let mut guard = self.state.lock().expect("tui state lock");
                        guard.output_dirty = true;
                        drop(guard);
                        let _ = self.render();
                    }
                    Event::FocusGained | Event::FocusLost => {
                        // Ignore focus events
                    }
                }
            }
        }
    }

    pub fn set_queue(&self, queued: &VecDeque<String>) -> Result<()> {
        {
            let mut guard = self.state.lock().expect("tui state lock");
            guard.queued = queued.clone();
            guard.output_dirty = true;
        }
        self.render()?;
        Ok(())
    }

    pub fn set_todos(&self, todos: &[crate::tools::create_todo::TodoItem]) -> Result<()> {
        {
            let mut guard = self.state.lock().expect("tui state lock");
            guard.todos = todos.to_vec();
            guard.output_dirty = true;
        }
        self.render()?;
        Ok(())
    }

    pub fn prompt_permission(&self, prompt: &PermissionPrompt) -> Option<usize> {
        let mut selected = 0usize;
        let mut buffer = String::new();

        loop {
            let snapshot = {
                let guard = self.state.lock().expect("tui state lock");
                let formatter = self.formatter.lock().expect("tui formatter lock");
                guard.snapshot(&formatter)
            };

            if let Ok(mut screen) = self.screen.lock() {
                let _ = screen.render_permission_in_output(&snapshot, prompt, selected, &buffer);
            }

            let event = event::read().ok()?;
            if let Event::Key(key_event) = event {
                if !matches!(key_event.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                    continue;
                }
                match key_event.code {
                    KeyCode::Esc => return None,
                    KeyCode::Char('c') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                        return None
                    }
                    KeyCode::Enter => {
                        if buffer.is_empty() {
                            return Some(selected);
                        }
                        if let Ok(value) = buffer.parse::<usize>() {
                            if value >= 1 && value <= prompt.options.len() {
                                return Some(value - 1);
                            }
                        }
                        buffer.clear();
                    }
                    KeyCode::Up => {
                        if selected > 0 {
                            selected -= 1;
                        }
                    }
                    KeyCode::Down => {
                        if selected + 1 < prompt.options.len() {
                            selected += 1;
                        }
                    }
                    KeyCode::Backspace => {
                        buffer.pop();
                    }
                    KeyCode::Char(c) if c.is_ascii_digit() => {
                        buffer.push(c);
                        if prompt.options.len() <= 9 {
                            if let Ok(value) = buffer.parse::<usize>() {
                                if value >= 1 && value <= prompt.options.len() {
                                    return Some(value - 1);
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    fn handle_paste(&self, pasted: &str) -> Result<()> {
        let mut guard = self.state.lock().expect("tui state lock");
        guard.history.reset_navigation();
        let normalized = pasted.replace("\r\n", "\n").replace('\r', "\n");
        let cursor_pos = guard.cursor_pos;
        guard.input.insert_str(cursor_pos, &normalized);
        guard.cursor_pos = cursor_pos + normalized.len();
        drop(guard);
        self.render()?;
        Ok(())
    }

    fn handle_mouse_event(&self, event: crossterm::event::MouseEvent) -> Result<()> {
        use crossterm::event::{MouseButton, MouseEventKind};

        let mut guard = self.state.lock().expect("tui state lock");
        let mut changed = false;

        match event.kind {
            MouseEventKind::ScrollUp => {
                guard.output_scroll = guard.output_scroll.saturating_add(3);
                changed = true;
            }
            MouseEventKind::ScrollDown => {
                guard.output_scroll = guard.output_scroll.saturating_sub(3);
                changed = true;
            }
            MouseEventKind::Down(MouseButton::Left) => {
                // Start selection at click position
                if let Some(pos) = self.screen_to_text_position(event.row, event.column, &guard) {
                    guard.selection_start = Some(pos);
                    guard.selection_end = Some(pos);
                    guard.selection_active = true;
                    changed = true;
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                // Extend selection while dragging
                if guard.selection_active {
                    if let Some(pos) = self.screen_to_text_position(event.row, event.column, &guard)
                    {
                        guard.selection_end = Some(pos);
                        changed = true;
                    }
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                // End drag but keep selection visible
                guard.selection_active = false;
            }
            MouseEventKind::Down(_) | MouseEventKind::Up(_) => {
                // Clear selection on other mouse buttons
                if guard.selection_start.is_some() {
                    guard.selection_start = None;
                    guard.selection_end = None;
                    guard.selection_active = false;
                    changed = true;
                }
            }
            _ => {}
        }

        if changed {
            guard.output_dirty = true;
            drop(guard);
            self.render()?;
        }
        Ok(())
    }

    fn handle_key_event(&self, key_event: KeyEvent) -> Result<Option<InputResult>> {
        if !matches!(key_event.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            return Ok(None);
        }
        let mut guard = self.state.lock().expect("tui state lock");

        // Handle Ctrl+C specially - copy selection if present, otherwise exit
        if matches!(
            key_event,
            KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                ..
            }
        ) {
            if guard.selection_start.is_some() {
                // Copy selected text to clipboard
                if let Some(text) = self.get_selected_text(&guard) {
                    // Try to copy to clipboard
                    match arboard::Clipboard::new() {
                        Ok(mut clipboard) => {
                            let _ = clipboard.set_text(text);
                        }
                        Err(_) => {
                            // Clipboard unavailable, silently ignore
                        }
                    }
                }
                // Clear selection after copying
                guard.selection_start = None;
                guard.selection_end = None;
                guard.selection_active = false;
                guard.output_dirty = true;
                drop(guard);
                self.render()?;
                return Ok(None);
            } else {
                // No selection, exit
                return Ok(Some(InputResult::Exit));
            }
        }

        // Clear selection on any other keyboard input
        if guard.selection_start.is_some() {
            guard.selection_start = None;
            guard.selection_end = None;
            guard.selection_active = false;
            guard.output_dirty = true;
        }

        if guard.reverse_search_mode {
            let result = handle_reverse_search_key(&mut guard, key_event);
            drop(guard);
            self.render()?;
            return Ok(result);
        }

        match key_event {
            KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                ..
            } => {
                // This case is now handled above, but keep it for safety
                return Ok(Some(InputResult::Exit));
            }
            KeyEvent {
                code: KeyCode::Char('r'),
                modifiers: KeyModifiers::CONTROL,
                ..
            } => {
                guard.reverse_search_mode = true;
                let current_input = guard.input.clone();
                guard.history.start_reverse_search(&current_input);
            }
            KeyEvent {
                code: KeyCode::Enter,
                modifiers,
                ..
            } => {
                if modifiers.contains(KeyModifiers::SHIFT) {
                    let cursor_pos = guard.cursor_pos;
                    guard.input.insert(cursor_pos, '\n');
                    guard.cursor_pos = cursor_pos + 1;
                } else {
                    if let Some(extra) = drain_queued_input_after_enter()? {
                        guard.history.reset_navigation();
                        let extra = extra.replace("\r\n", "\n").replace('\r', "\n");
                        let cursor_pos = guard.cursor_pos;
                        guard.input.insert_str(cursor_pos, &extra);
                        guard.cursor_pos = cursor_pos + extra.len();
                        drop(guard);
                        self.render()?;
                        return Ok(None);
                    }
                    let submitted = guard.input.clone();
                    if !submitted.trim().is_empty() {
                        guard.history.add_entry(submitted.clone());
                    }
                    guard.input.clear();
                    guard.cursor_pos = 0;
                    drop(guard);
                    self.render()?;
                    return Ok(Some(InputResult::Submitted(submitted)));
                }
            }
            KeyEvent {
                code: KeyCode::Tab, ..
            } => {
                if let Some(completion) =
                    crate::autocomplete::handle_tab_completion(&guard.input, guard.cursor_pos)
                {
                    guard.input = completion;
                    guard.cursor_pos = guard.input.len();
                }
            }
            KeyEvent {
                code: KeyCode::Backspace,
                ..
            } => {
                if guard.cursor_pos > 0 {
                    let prev = previous_char_boundary(&guard.input, guard.cursor_pos);
                    let cursor_pos = guard.cursor_pos;
                    guard.input.drain(prev..cursor_pos);
                    guard.cursor_pos = prev;
                    guard.history.reset_navigation();
                }
            }
            KeyEvent {
                code: KeyCode::Left,
                ..
            } => {
                guard.cursor_pos = previous_char_boundary(&guard.input, guard.cursor_pos);
            }
            KeyEvent {
                code: KeyCode::Right,
                ..
            } => {
                guard.cursor_pos = next_char_boundary(&guard.input, guard.cursor_pos);
            }
            KeyEvent {
                code: KeyCode::Up, ..
            } => {
                let current_input = guard.input.clone();
                if let Some(new_input) = guard.history.navigate_up(&current_input) {
                    guard.input = new_input;
                    guard.cursor_pos = guard.input.len();
                }
            }
            KeyEvent {
                code: KeyCode::Down,
                ..
            } => {
                if let Some(new_input) = guard.history.navigate_down() {
                    guard.input = new_input;
                    guard.cursor_pos = guard.input.len();
                }
            }
            KeyEvent {
                code: KeyCode::Esc, ..
            } => {
                guard.input.clear();
                guard.cursor_pos = 0;
                drop(guard);
                self.render()?;
                return Ok(Some(InputResult::Cancelled));
            }
            KeyEvent {
                code: KeyCode::Char(c),
                ..
            } => {
                let cursor_pos = guard.cursor_pos;
                guard.input.insert(cursor_pos, c);
                guard.cursor_pos = cursor_pos + c.len_utf8();
                guard.history.reset_navigation();
            }
            _ => {}
        }

        drop(guard);
        self.render()?;
        Ok(None)
    }

    /// Convert screen coordinates to text buffer position
    fn screen_to_text_position(
        &self,
        screen_row: u16,
        screen_col: u16,
        guard: &TuiState,
    ) -> Option<TextPosition> {
        // Get terminal size
        let screen = self.screen.lock().expect("tui screen lock");
        let size = screen.terminal.size().ok()?;
        drop(screen);

        // Reconstruct layout (same logic as render_frame)
        let formatter = self.formatter.lock().expect("tui formatter lock");
        let snapshot = guard.snapshot(&formatter);
        drop(formatter);

        let max_input_height = size.height.saturating_sub(3).max(2);
        let input_layout = build_input_layout(&snapshot, size.width as usize);
        let input_height = (input_layout.lines.len() + 2).min(max_input_height as usize) as u16;
        let max_queue_height = size.height.saturating_sub(3 + input_height);
        let (queue_height, _) =
            build_queue_layout(&snapshot, size.width as usize, max_queue_height);
        let max_todo_height = size
            .height
            .saturating_sub(MIN_OUTPUT_HEIGHT as u16 + input_height + queue_height);
        let (todo_height, _) = build_todo_layout(&snapshot, size.width as usize, max_todo_height);
        let output_height = size.height.saturating_sub(
            input_height
                .saturating_add(queue_height)
                .saturating_add(todo_height),
        );

        // Check if click is within output area
        if screen_row >= output_height {
            return None;
        }

        // Map to wrapped line
        let width = size.width as usize;
        let output_lines = build_output_lines(&snapshot.output_lines, width);
        let max_scroll = output_lines.len().saturating_sub(output_height as usize);
        let scroll_from_bottom = snapshot.output_scroll.min(max_scroll);
        let scroll = max_scroll.saturating_sub(scroll_from_bottom);

        let wrapped_line_idx = scroll + (screen_row as usize);
        if wrapped_line_idx >= output_lines.len() {
            return None;
        }

        // Map back to original line
        let mut wrapped_count = 0;
        for (line_idx, orig_line) in snapshot.output_lines.iter().enumerate() {
            let wrapped = wrap_ansi_line(orig_line, width);

            if wrapped_count + wrapped.len() > wrapped_line_idx {
                let wrap_offset = wrapped_line_idx - wrapped_count;
                let wrapped_line = &wrapped[wrap_offset];

                // Convert screen column to char offset in wrapped line
                let char_in_wrapped = screen_col_to_char_offset(wrapped_line, screen_col as usize);

                // Calculate offset in original line
                let char_offset = (wrap_offset * width) + char_in_wrapped;
                let line_char_count = strip_ansi_codes(orig_line).chars().count();
                let clamped_offset = char_offset.min(line_char_count);

                return Some(TextPosition::new(line_idx, clamped_offset));
            }

            wrapped_count += wrapped.len();
        }

        None
    }

    /// Extract selected text from the output buffer
    fn get_selected_text(&self, guard: &TuiState) -> Option<String> {
        let start = guard.selection_start?;
        let end = guard.selection_end?;

        // Order the positions
        let (start, end) = start.min_max(end);

        let mut result = String::new();

        if start.line_idx == end.line_idx {
            // Selection on single line
            if let Some(line) = guard.output.lines.get(start.line_idx) {
                let visible = strip_ansi_codes(line);
                let chars: Vec<char> = visible.chars().collect();
                let selected: String = chars
                    .iter()
                    .skip(start.char_offset)
                    .take(end.char_offset + 1 - start.char_offset)
                    .collect();
                result.push_str(&selected);
            }
        } else {
            // Multi-line selection
            for line_idx in start.line_idx..=end.line_idx {
                if let Some(line) = guard.output.lines.get(line_idx) {
                    let visible = strip_ansi_codes(line);
                    let chars: Vec<char> = visible.chars().collect();

                    if line_idx == start.line_idx {
                        // First line - from start offset to end
                        let selected: String = chars.iter().skip(start.char_offset).collect();
                        result.push_str(&selected);
                    } else if line_idx == end.line_idx {
                        // Last line - from beginning to end offset
                        let selected: String = chars.iter().take(end.char_offset + 1).collect();
                        result.push_str(&selected);
                    } else {
                        // Middle lines - entire line
                        result.push_str(&visible);
                    }

                    // Add newline between lines (but not after the last line)
                    if line_idx < end.line_idx {
                        result.push('\n');
                    }
                }
            }
        }

        if result.is_empty() {
            None
        } else {
            Some(result)
        }
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        output::clear_output_sink();
        let _ = self.screen.lock().map(|mut screen| screen.reset());
        let mut stdout = io::stdout();
        let _ = stdout.execute(DisableBracketedPaste);
        let _ = stdout.execute(DisableMouseCapture);
        let _ = stdout.execute(LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

impl TuiState {
    fn snapshot(&self, formatter: &formatter::CodeFormatter) -> TuiSnapshot {
        let (input_display, input_raw, cursor_pos) = if self.reverse_search_mode {
            let state = self.history.get_reverse_search_state();
            let prompt = format!(
                "(reverse-i-search)`{}`: {}",
                state.search_query,
                state
                    .matched_entry
                    .clone()
                    .unwrap_or_else(|| "".to_string())
            );
            let cursor_pos = prompt.len();
            (prompt.clone(), prompt, cursor_pos)
        } else {
            (
                formatter.format_input_with_file_highlighting(&self.input),
                self.input.clone(),
                self.cursor_pos,
            )
        };

        let selection_range =
            if let (Some(start), Some(end)) = (self.selection_start, self.selection_end) {
                Some(start.min_max(end))
            } else {
                None
            };

        TuiSnapshot {
            output_lines: self.output.lines.clone(),
            queued: self.queued.iter().cloned().collect(),
            input_display,
            input_raw,
            cursor_pos,
            output_scroll: self.output_scroll,
            selection_range,
            todos: self.todos.clone(),
        }
    }
}

impl TuiScreen {
    fn render_full(&mut self, snapshot: &TuiSnapshot) -> Result<()> {
        self.render_frame(snapshot)
    }

    fn render_input_only(&mut self, snapshot: &TuiSnapshot) -> Result<()> {
        self.render_frame(snapshot)
    }

    fn render_frame(&mut self, snapshot: &TuiSnapshot) -> Result<()> {
        self.terminal.draw(|frame| {
            let size = frame.area();
            let max_input_height = size.height.saturating_sub(MIN_OUTPUT_HEIGHT as u16).max(2);
            let input_layout = build_input_layout(snapshot, size.width as usize);
            let input_lines = input_layout.lines.len().max(1);
            let input_height = (input_lines + 2).min(max_input_height as usize) as u16;
            let max_queue_height = size
                .height
                .saturating_sub(MIN_OUTPUT_HEIGHT as u16 + input_height);
            let (queue_height, queue_lines) =
                build_queue_layout(snapshot, size.width as usize, max_queue_height);

            // Calculate todo pane height
            let max_todo_height = size
                .height
                .saturating_sub(MIN_OUTPUT_HEIGHT as u16 + input_height + queue_height);
            let (todo_height, todo_lines) =
                build_todo_layout(snapshot, size.width as usize, max_todo_height);

            let output_height = size.height.saturating_sub(
                input_height
                    .saturating_add(queue_height)
                    .saturating_add(todo_height),
            );

            let chunks = if todo_height > 0 && queue_height > 0 {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(output_height),
                        Constraint::Length(queue_height),
                        Constraint::Length(todo_height),
                        Constraint::Length(input_height),
                    ])
                    .split(size)
            } else if todo_height > 0 {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(output_height),
                        Constraint::Length(todo_height),
                        Constraint::Length(input_height),
                    ])
                    .split(size)
            } else if queue_height > 0 {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(output_height),
                        Constraint::Length(queue_height),
                        Constraint::Length(input_height),
                    ])
                    .split(size)
            } else {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(output_height),
                        Constraint::Length(input_height),
                    ])
                    .split(size)
            };

            let output_rect = chunks[0];

            let (queue_rect, todo_rect, input_rect) = if queue_height > 0 && todo_height > 0 {
                (Some(chunks[1]), Some(chunks[2]), chunks[3])
            } else if todo_height > 0 {
                (None, Some(chunks[1]), chunks[2])
            } else if queue_height > 0 {
                (Some(chunks[1]), None, chunks[2])
            } else {
                (None, None, chunks[1])
            };

            let output_text = build_output_text(snapshot, output_rect);
            let output_para = Paragraph::new(output_text);
            frame.render_widget(output_para, output_rect);

            if let Some(queue_rect) = queue_rect {
                let queue_text = build_queue_text(&queue_lines);
                let title = format!("Queued ({})", snapshot.queued.len());
                let queue_block = Block::default().borders(Borders::NONE).title(title);
                let queue_para = Paragraph::new(queue_text).block(queue_block);
                frame.render_widget(queue_para, queue_rect);
            }

            if let Some(todo_rect) = todo_rect {
                let todo_text = build_todo_text(&todo_lines);
                let title = format!("Todos ({})", snapshot.todos.len());
                let todo_block = Block::default().borders(Borders::NONE).title(title);
                let todo_para = Paragraph::new(todo_text).block(todo_block);
                frame.render_widget(todo_para, todo_rect);
            }

            let (input_text, cursor_row_offset, cursor_col) =
                build_input_text_with_layout(input_rect, &input_layout);
            let input_block = Block::default().borders(Borders::TOP | Borders::BOTTOM);
            let input_para = Paragraph::new(input_text).block(input_block);
            frame.render_widget(input_para, input_rect);

            let cursor_row = input_rect.y + 1 + cursor_row_offset;
            frame.set_cursor_position((input_rect.x + cursor_col, cursor_row));
        })?;

        Ok(())
    }

    fn render_permission_in_output(
        &mut self,
        snapshot: &TuiSnapshot,
        prompt: &PermissionPrompt,
        selected: usize,
        buffer: &str,
    ) -> Result<()> {
        self.terminal.draw(|frame| {
            let size = frame.area();
            let max_input_height = size.height.saturating_sub(MIN_OUTPUT_HEIGHT as u16).max(2);
            let input_layout = build_input_layout(snapshot, size.width as usize);
            let input_lines = input_layout.lines.len().max(1);
            let input_height = (input_lines + 2).min(max_input_height as usize) as u16;
            let max_queue_height = size
                .height
                .saturating_sub(MIN_OUTPUT_HEIGHT as u16 + input_height);
            let (queue_height, queue_lines) =
                build_queue_layout(snapshot, size.width as usize, max_queue_height);

            // Calculate todo pane height
            let max_todo_height = size
                .height
                .saturating_sub(MIN_OUTPUT_HEIGHT as u16 + input_height + queue_height);
            let (todo_height, todo_lines) =
                build_todo_layout(snapshot, size.width as usize, max_todo_height);

            let output_height = size.height.saturating_sub(
                input_height
                    .saturating_add(queue_height)
                    .saturating_add(todo_height),
            );

            let chunks = if todo_height > 0 && queue_height > 0 {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(output_height),
                        Constraint::Length(queue_height),
                        Constraint::Length(todo_height),
                        Constraint::Length(input_height),
                    ])
                    .split(size)
            } else if todo_height > 0 {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(output_height),
                        Constraint::Length(todo_height),
                        Constraint::Length(input_height),
                    ])
                    .split(size)
            } else if queue_height > 0 {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(output_height),
                        Constraint::Length(queue_height),
                        Constraint::Length(input_height),
                    ])
                    .split(size)
            } else {
                Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Length(output_height),
                        Constraint::Length(input_height),
                    ])
                    .split(size)
            };

            let output_rect = chunks[0];
            let (queue_rect, todo_rect, input_rect) = if queue_height > 0 && todo_height > 0 {
                (Some(chunks[1]), Some(chunks[2]), chunks[3])
            } else if todo_height > 0 {
                (None, Some(chunks[1]), chunks[2])
            } else if queue_height > 0 {
                (Some(chunks[1]), None, chunks[2])
            } else {
                (None, None, chunks[1])
            };

            let output_text =
                build_output_text_with_prompt(snapshot, output_rect, prompt, selected, buffer);
            let output_para = Paragraph::new(output_text);
            frame.render_widget(output_para, output_rect);

            if let Some(queue_rect) = queue_rect {
                let queue_text = build_queue_text(&queue_lines);
                let title = format!("Queued ({})", snapshot.queued.len());
                let queue_block = Block::default().borders(Borders::NONE).title(title);
                let queue_para = Paragraph::new(queue_text).block(queue_block);
                frame.render_widget(queue_para, queue_rect);
            }

            if let Some(todo_rect) = todo_rect {
                let todo_text = build_todo_text(&todo_lines);
                let title = format!("Todos ({})", snapshot.todos.len());
                let todo_block = Block::default().borders(Borders::NONE).title(title);
                let todo_para = Paragraph::new(todo_text).block(todo_block);
                frame.render_widget(todo_para, todo_rect);
            }

            let (input_text, cursor_row_offset, cursor_col) =
                build_input_text_with_layout(input_rect, &input_layout);
            let input_block = Block::default().borders(Borders::TOP | Borders::BOTTOM);
            let input_para = Paragraph::new(input_text).block(input_block);
            frame.render_widget(input_para, input_rect);

            let cursor_row = input_rect.y + 1 + cursor_row_offset;
            frame.set_cursor_position((input_rect.x + cursor_col, cursor_row));
        })?;

        Ok(())
    }

    fn reset(&mut self) {
        let _ = self.terminal.clear();
    }
}
impl OutputSink for TuiOutputSink {
    fn write(&self, text: &str, _is_err: bool) {
        let (snapshot, _should_render) = {
            let mut guard = self.state.lock().expect("tui state lock");
            let formatter = self.formatter.lock().expect("tui formatter lock");
            guard.output.push_text(text);
            guard.output_dirty = true;
            guard.last_render = Instant::now();
            if guard.output_scroll > 0 {
                guard.output_scroll = guard.output_scroll.saturating_add(1);
            }
            let should_render = guard.last_render.elapsed() >= RENDER_INTERVAL;
            if should_render {
                guard.output_dirty = false;
                guard.last_render = Instant::now();
            }
            (guard.snapshot(&formatter), should_render)
        };

        if let Ok(mut screen) = self.screen.lock() {
            let _ = screen.render_full(&snapshot);
        }
    }

    fn flush(&self) {
        if let Ok(mut screen) = self.screen.lock() {
            let snapshot = {
                let mut guard = self.state.lock().expect("tui state lock");
                let formatter = self.formatter.lock().expect("tui formatter lock");
                guard.output_dirty = false;
                guard.last_render = Instant::now();
                guard.snapshot(&formatter)
            };
            let _ = screen.render_full(&snapshot);
        }
    }
}

fn handle_reverse_search_key(guard: &mut TuiState, key_event: KeyEvent) -> Option<InputResult> {
    match key_event {
        KeyEvent {
            code: KeyCode::Enter,
            ..
        } => {
            if let Some(matched_entry) = guard.history.finish_reverse_search() {
                guard.input = matched_entry;
                guard.cursor_pos = guard.input.len();
            }
            guard.reverse_search_mode = false;
            None
        }
        KeyEvent {
            code: KeyCode::Esc, ..
        } => {
            guard.input = guard.history.cancel_reverse_search();
            guard.cursor_pos = guard.input.len();
            guard.reverse_search_mode = false;
            None
        }
        KeyEvent {
            code: KeyCode::Backspace,
            ..
        } => {
            let state = guard.history.get_reverse_search_state();
            let mut query = state.search_query.clone();
            if !query.is_empty() {
                query.pop();
                guard.history.update_reverse_search(&query);
            }
            None
        }
        KeyEvent {
            code: KeyCode::Char('r'),
            modifiers: KeyModifiers::CONTROL,
            ..
        }
        | KeyEvent {
            code: KeyCode::Char('r'),
            ..
        } => {
            guard.history.reverse_search_next();
            None
        }
        KeyEvent {
            code: KeyCode::Up, ..
        } => {
            guard.history.reverse_search_prev();
            None
        }
        KeyEvent {
            code: KeyCode::Down,
            ..
        } => {
            guard.history.reverse_search_next();
            None
        }
        KeyEvent {
            code: KeyCode::Char(c),
            ..
        } => {
            let state = guard.history.get_reverse_search_state();
            let mut query = state.search_query.clone();
            query.push(c);
            guard.history.update_reverse_search(&query);
            None
        }
        _ => None,
    }
}

struct InputLayout {
    lines: Vec<String>,
    cursor_row: usize,
    cursor_col: usize,
}

/// Strip ANSI escape codes from text
fn strip_ansi_codes(text: &str) -> String {
    let mut result = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if let Some('[') = chars.peek().copied() {
                chars.next();
                while let Some(next) = chars.next() {
                    if next == 'm' {
                        break;
                    }
                }
            }
            continue;
        }
        result.push(ch);
    }
    result
}

/// Convert screen column to character offset, skipping ANSI codes
fn screen_col_to_char_offset(ansi_line: &str, screen_col: usize) -> usize {
    let mut visible_count = 0;
    let mut char_count = 0;
    let mut chars = ansi_line.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            if let Some('[') = chars.peek().copied() {
                chars.next();
                while let Some(next) = chars.next() {
                    if next == 'm' {
                        break;
                    }
                }
            }
            continue;
        }

        if visible_count >= screen_col {
            break;
        }

        visible_count += 1;
        char_count += 1;
    }

    char_count
}

#[derive(Clone)]
struct WrappedLineInfo {
    line_idx: usize,
    char_start: usize,
    char_end: usize,
}

fn build_line_mapping(lines: &[String], width: usize) -> Vec<WrappedLineInfo> {
    let mut mapping = Vec::new();

    for (line_idx, line) in lines.iter().enumerate() {
        let wrapped = wrap_ansi_line(line, width);
        let mut char_start = 0;

        for segment in wrapped {
            let visible = strip_ansi_codes(&segment);
            let char_end = char_start + visible.chars().count();
            mapping.push(WrappedLineInfo {
                line_idx,
                char_start,
                char_end,
            });
            char_start = char_end;
        }
    }

    mapping
}

fn render_line_with_selection(
    info: &WrappedLineInfo,
    wrapped_line: &str,
    sel_start: TextPosition,
    sel_end: TextPosition,
) -> Text<'static> {
    use ratatui::style::{Modifier, Style};
    use ratatui::text::{Line, Span};

    // Determine highlight range within this wrapped segment
    let segment_len = info.char_end.saturating_sub(info.char_start);
    let (mut hl_start, mut hl_end) =
        if info.line_idx == sel_start.line_idx && info.line_idx == sel_end.line_idx {
            // Selection on same line
            (
                sel_start.char_offset.saturating_sub(info.char_start),
                (sel_end.char_offset + 1)
                    .saturating_sub(info.char_start)
                    .min(info.char_end - info.char_start),
            )
        } else if info.line_idx == sel_start.line_idx {
            // Selection starts here
            (
                sel_start.char_offset.saturating_sub(info.char_start),
                info.char_end - info.char_start,
            )
        } else if info.line_idx == sel_end.line_idx {
            // Selection ends here
            (
                0,
                (sel_end.char_offset + 1)
                    .saturating_sub(info.char_start)
                    .min(info.char_end - info.char_start),
            )
        } else {
            // Entire line selected
            (0, info.char_end - info.char_start)
        };
    hl_start = hl_start.min(segment_len);
    hl_end = hl_end.min(segment_len);
    if hl_end < hl_start {
        hl_end = hl_start;
    }

    // Build spans with selection highlight
    let visible = strip_ansi_codes(wrapped_line);
    let chars: Vec<char> = visible.chars().collect();

    let before: String = chars.iter().take(hl_start).collect();
    let selected: String = chars
        .iter()
        .skip(hl_start)
        .take(hl_end - hl_start)
        .collect();
    let after: String = chars.iter().skip(hl_end).collect();

    let mut spans = Vec::new();
    if !before.is_empty() {
        spans.push(Span::raw(before));
    }
    if !selected.is_empty() {
        spans.push(Span::styled(
            selected,
            Style::default().add_modifier(Modifier::REVERSED),
        ));
    }
    if !after.is_empty() {
        spans.push(Span::raw(after));
    }

    Text::from(Line::from(spans))
}

fn build_output_text(snapshot: &TuiSnapshot, rect: Rect) -> Text<'static> {
    if rect.height == 0 || rect.width == 0 {
        return Text::default();
    }

    let width = rect.width as usize;
    let height = rect.height as usize;
    let output_lines = build_output_lines(&snapshot.output_lines, width);
    let max_scroll = output_lines.len().saturating_sub(height);
    let scroll_from_bottom = snapshot.output_scroll.min(max_scroll);
    let scroll = max_scroll.saturating_sub(scroll_from_bottom);

    let mut text = Text::default();

    // Build line-to-position mapping for selection
    let line_map = if snapshot.selection_range.is_some() {
        build_line_mapping(&snapshot.output_lines, width)
    } else {
        vec![]
    };

    for (wrapped_idx, line) in output_lines.into_iter().skip(scroll).enumerate() {
        let absolute_wrapped_idx = scroll + wrapped_idx;

        if let Some((start, end)) = snapshot.selection_range {
            if let Some(info) = line_map.get(absolute_wrapped_idx) {
                if info.line_idx >= start.line_idx && info.line_idx <= end.line_idx {
                    // This line may contain selection
                    let line_text = render_line_with_selection(info, &line, start, end);
                    text.lines.extend(line_text.lines);
                    continue;
                }
            }
        }

        // No selection on this line
        let line_text = line
            .into_text()
            .unwrap_or_else(|_| Text::from(line.clone()));
        text.lines.extend(line_text.lines);
    }

    text
}

fn build_queue_layout(snapshot: &TuiSnapshot, width: usize, max_height: u16) -> (u16, Vec<String>) {
    if snapshot.queued.is_empty() || max_height < 3 || width == 0 {
        return (0, Vec::new());
    }

    let inner_height = max_height.saturating_sub(2) as usize;
    if inner_height == 0 {
        return (0, Vec::new());
    }

    let queue_lines = build_queue_lines(&snapshot.queued, width, inner_height);
    if queue_lines.is_empty() {
        return (0, Vec::new());
    }

    let height = (queue_lines.len() + 2).min(max_height as usize) as u16;
    (height, queue_lines)
}

fn build_queue_lines(queue: &[String], width: usize, max_lines: usize) -> Vec<String> {
    if width == 0 || max_lines == 0 {
        return Vec::new();
    }

    let mut lines = Vec::new();
    let mut items_rendered = 0usize;
    let mut truncated = false;

    for (idx, item) in queue.iter().enumerate() {
        let prefix = format!("{}) ", idx + 1);
        let available = width.saturating_sub(prefix.len()).max(1);
        let wrapped = wrap_ansi_line(item, available);
        let padding = " ".repeat(prefix.len());
        for (wrap_idx, segment) in wrapped.iter().enumerate() {
            if lines.len() >= max_lines {
                truncated = true;
                break;
            }
            let prefix_used = if wrap_idx == 0 {
                prefix.as_str()
            } else {
                padding.as_str()
            };
            lines.push(format!("{}{}", prefix_used, segment));
        }
        if truncated {
            break;
        }
        items_rendered += 1;
        if lines.len() >= max_lines {
            break;
        }
    }

    if truncated && !lines.is_empty() {
        let remaining = queue.len().saturating_sub(items_rendered);
        let indicator = format!("... ({} more)", remaining.max(1));
        let trimmed: String = indicator.chars().take(width).collect();
        let last_idx = lines.len() - 1;
        lines[last_idx] = trimmed;
    }

    lines
}

fn build_queue_text(lines: &[String]) -> Text<'static> {
    let mut text = Text::default();
    for line in lines {
        let line_text = line
            .clone()
            .into_text()
            .unwrap_or_else(|_| Text::from(line.clone()));
        text.lines.extend(line_text.lines);
    }
    text
}

fn build_todo_layout(snapshot: &TuiSnapshot, width: usize, max_height: u16) -> (u16, Vec<String>) {
    if snapshot.todos.is_empty()
        || snapshot.todos.iter().all(|todo| todo.completed)
        || max_height < 3
        || width == 0
    {
        return (0, Vec::new());
    }

    let inner_height = max_height.saturating_sub(2) as usize;
    if inner_height == 0 {
        return (0, Vec::new());
    }

    let todo_lines = build_todo_lines(&snapshot.todos, width, inner_height);
    if todo_lines.is_empty() {
        return (0, Vec::new());
    }

    let height = (todo_lines.len() + 2).min(max_height as usize) as u16;
    (height, todo_lines)
}

fn build_todo_lines(
    todos: &[crate::tools::create_todo::TodoItem],
    width: usize,
    max_lines: usize,
) -> Vec<String> {
    if width == 0 || max_lines == 0 {
        return Vec::new();
    }

    let mut lines = Vec::new();
    let mut pending = Vec::new();
    let mut completed = Vec::new();
    for todo in todos {
        if todo.completed {
            completed.push(todo);
        } else {
            pending.push(todo);
        }
    }
    let mut ordered = Vec::with_capacity(todos.len());
    ordered.extend(pending);
    ordered.extend(completed);
    let item_limit = 10usize.min(ordered.len());

    for todo in ordered.iter().take(item_limit) {
        let checkbox = if todo.completed { "[✓]" } else { "[ ]" };
        let prefix = format!("  {} ", checkbox);
        let available = width.saturating_sub(prefix.len()).max(1);
        let wrapped = wrap_ansi_line(&todo.description, available);
        let padding = " ".repeat(prefix.len());

        for (wrap_idx, segment) in wrapped.iter().enumerate() {
            if lines.len() >= max_lines {
                break;
            }
            let prefix_used = if wrap_idx == 0 {
                prefix.as_str()
            } else {
                padding.as_str()
            };
            lines.push(format!("{}{}", prefix_used, segment));
        }

        if lines.len() >= max_lines {
            break;
        }
    }

    let remaining = ordered.len().saturating_sub(item_limit);
    if remaining > 0 && lines.len() < max_lines {
        let prefix = "  ";
        let available = width.saturating_sub(prefix.len()).max(1);
        let wrapped = wrap_ansi_line(&format!("...({} more)...", remaining), available);
        for segment in wrapped {
            if lines.len() >= max_lines {
                break;
            }
            lines.push(format!("{}{}", prefix, segment));
        }
    }

    lines
}

fn build_todo_text(lines: &[String]) -> Text<'static> {
    let mut text = Text::default();
    for line in lines {
        if line.is_empty() {
            text.lines.push(Line::from(""));
            continue;
        }
        let base_style = Style::default().add_modifier(Modifier::BOLD);
        let completed_prefix = "  [✓] ";

        if line.starts_with(completed_prefix) {
            let rest = &line[completed_prefix.len()..];
            let completed_style = base_style.add_modifier(Modifier::DIM);
            let completed_check_style = completed_style.fg(Color::Green);
            text.lines.push(Line::from(vec![
                Span::styled("  [", completed_style),
                Span::styled("✓", completed_check_style),
                Span::styled("] ", completed_style),
                Span::styled(rest.to_string(), completed_style),
            ]));
        } else {
            text.lines
                .push(Line::from(Span::styled(line.clone(), base_style)));
        }
    }
    text
}

fn build_output_text_with_prompt(
    snapshot: &TuiSnapshot,
    rect: Rect,
    prompt: &PermissionPrompt,
    selected: usize,
    buffer: &str,
) -> Text<'static> {
    if rect.height == 0 || rect.width == 0 {
        return Text::default();
    }

    let width = rect.width as usize;
    let height = rect.height as usize;
    let output_lines = build_output_lines(&snapshot.output_lines, width);
    let mut lines = Vec::new();
    for line in output_lines {
        let line_text = line
            .into_text()
            .unwrap_or_else(|_| Text::from(line.clone()));
        lines.extend(line_text.lines);
    }

    lines.push(Line::from(""));
    lines.extend(build_permission_lines(prompt, selected, buffer, width));

    let start = lines.len().saturating_sub(height);
    let mut text = Text::default();
    text.lines = lines.into_iter().skip(start).collect();
    text
}

fn build_input_text_with_layout(
    rect: Rect,
    input_layout: &InputLayout,
) -> (Text<'static>, u16, u16) {
    if rect.height < 2 || rect.width == 0 {
        return (Text::default(), 0, 0);
    }

    let width = rect.width as usize;
    let inner_height = rect.height.saturating_sub(2) as usize;

    let mut input_scroll = if input_layout.lines.len() > inner_height {
        input_layout.lines.len() - inner_height
    } else {
        0
    };
    if input_layout.cursor_row >= input_scroll + inner_height {
        input_scroll = input_layout
            .cursor_row
            .saturating_sub(inner_height.saturating_sub(1));
    }
    if input_layout.cursor_row < input_scroll {
        input_scroll = input_layout.cursor_row;
    }

    let mut visible_text = Text::default();
    for line in input_layout
        .lines
        .iter()
        .skip(input_scroll)
        .take(inner_height)
    {
        let line_text = line
            .clone()
            .into_text()
            .unwrap_or_else(|_| Text::from(line.clone()));
        visible_text.lines.extend(line_text.lines);
    }

    let cursor_row_offset = input_layout
        .cursor_row
        .saturating_sub(input_scroll)
        .min(inner_height.saturating_sub(1)) as u16;
    let cursor_col = input_layout.cursor_col.min(width.saturating_sub(1)) as u16;

    (visible_text, cursor_row_offset, cursor_col)
}

fn build_input_layout(snapshot: &TuiSnapshot, width: usize) -> InputLayout {
    let mut lines = Vec::new();
    let mut cursor_row = 0;
    let mut cursor_col = 0;

    let input_lines: Vec<&str> = snapshot.input_display.split('\n').collect();
    let raw_lines: Vec<&str> = snapshot.input_raw.split('\n').collect();

    let (cursor_line_idx, cursor_col_in_line) =
        cursor_position_in_lines(&snapshot.input_raw, snapshot.cursor_pos);

    let mut total_rows = 0usize;

    for (idx, line) in input_lines.iter().enumerate() {
        let prefix = if idx == 0 {
            INPUT_PREFIX_MAIN
        } else {
            INPUT_PREFIX_CONT
        };
        let available = width.saturating_sub(prefix.len()).max(1);

        let wrapped = wrap_ansi_line(line, available);
        let padding = " ".repeat(prefix.len());
        for (wrap_idx, segment) in wrapped.iter().enumerate() {
            let prefix_used = if wrap_idx == 0 {
                prefix
            } else {
                padding.as_str()
            };
            lines.push(format!("{}{}", prefix_used, segment));
        }

        let raw_line_len = raw_lines.get(idx).map(|v| v.chars().count()).unwrap_or(0);
        let wraps = if raw_line_len == 0 {
            0
        } else {
            raw_line_len.saturating_sub(1) / available
        };

        if idx == cursor_line_idx {
            cursor_row = total_rows + (cursor_col_in_line / available);
            let col_in_wrap = cursor_col_in_line % available;
            cursor_col = prefix.len() + col_in_wrap;
        }

        total_rows += wraps + 1;
    }

    if lines.is_empty() {
        lines.push(format!("{}{}", INPUT_PREFIX_MAIN, ""));
        cursor_row = 0;
        cursor_col = INPUT_PREFIX_MAIN.len();
    }

    InputLayout {
        lines,
        cursor_row,
        cursor_col,
    }
}

fn cursor_position_in_lines(text: &str, cursor_pos: usize) -> (usize, usize) {
    let mut line_idx = 0usize;
    let mut col = 0usize;
    let bounded = cursor_pos.min(text.len());

    for ch in text[..bounded].chars() {
        if ch == '\n' {
            line_idx += 1;
            col = 0;
        } else {
            col += 1;
        }
    }

    (line_idx, col)
}

fn build_output_lines(lines: &[String], width: usize) -> Vec<String> {
    let mut output_lines = Vec::new();
    let width = width.max(1);

    for line in lines {
        let wrapped = wrap_ansi_line(line, width);
        output_lines.extend(wrapped);
    }

    if output_lines.is_empty() {
        output_lines.push(String::new());
    }

    output_lines
}

fn build_permission_lines(
    prompt: &PermissionPrompt,
    selected: usize,
    buffer: &str,
    width: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let summary_style = Style::default().add_modifier(Modifier::BOLD);
    for segment in wrap_ansi_line(&prompt.summary, width) {
        lines.push(Line::from(Span::styled(segment, summary_style)));
    }

    if !prompt.detail.is_empty() {
        lines.push(Line::from(""));
        for line in prompt.detail.lines() {
            for segment in wrap_ansi_line(line, width) {
                lines.push(Line::from(segment));
            }
        }
    }

    lines.push(Line::from(""));
    for (idx, option) in prompt.options.iter().enumerate() {
        let prefix = if idx == selected { "> " } else { "  " };
        let label = format!("{}. {}", idx + 1, option);
        let available = width.saturating_sub(prefix.len()).max(1);
        let wrapped = wrap_ansi_line(&label, available);
        let style = if idx == selected {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        let padding = " ".repeat(prefix.len());
        for (wrap_idx, segment) in wrapped.iter().enumerate() {
            let prefix_used = if wrap_idx == 0 {
                prefix
            } else {
                padding.as_str()
            };
            let text = format!("{}{}", prefix_used, segment);
            lines.push(Line::from(Span::styled(text, style)));
        }
    }

    if !buffer.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(format!("Selection: {}", buffer)));
    }

    lines
}

/// When bracketed paste is unavailable, a multiline paste arrives as a burst of Key events.
/// If Enter is pressed and more events are queued immediately, treat it as a paste newline.
fn drain_queued_input_after_enter() -> Result<Option<String>> {
    let mut collected = String::from("\n");
    let mut has_non_newline = false;
    let mut seen_paste = false;

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
                collected.push('\n');
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

fn wrap_ansi_line(line: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }

    let mut segments = Vec::new();
    let mut current = String::new();
    let mut visible = 0usize;
    let mut chars = line.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch == '\x1b' {
            current.push(ch);
            if let Some('[') = chars.peek().copied() {
                current.push('[');
                chars.next();
                while let Some(next) = chars.next() {
                    current.push(next);
                    if next == 'm' {
                        break;
                    }
                }
            }
            continue;
        }

        current.push(ch);
        visible += 1;
        if visible >= width {
            segments.push(current);
            current = String::new();
            visible = 0;
        }
    }

    if !current.is_empty() || segments.is_empty() {
        segments.push(current);
    }

    segments
}

fn normalize_cursor_pos(text: &str, cursor_pos: usize) -> usize {
    if cursor_pos > text.len() {
        return text.len();
    }

    if text.is_char_boundary(cursor_pos) {
        return cursor_pos;
    }

    let mut pos = cursor_pos;
    while pos > 0 && !text.is_char_boundary(pos) {
        pos -= 1;
    }
    pos
}

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

pub fn init_tui_output(formatter: &formatter::CodeFormatter) -> Result<Tui> {
    let tui = Tui::new(formatter)?;
    output::set_output_sink(tui.output_sink());
    tui.render()?;
    Ok(tui)
}

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
            todos: vec![],
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
            todos: vec![],
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
            todos: vec![],
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
            todos: vec![],
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
            todos: vec![],
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
            todos: vec![],
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

    // =============================================================================
    // Additional Edge Cases and Integration Tests
    // =============================================================================

    #[test]
    fn test_output_buffer_mixed_line_endings() {
        let mut buffer = OutputBuffer::new(100);
        buffer.push_text("Line 1\r\nLine 2\nLine 3\rLine 4");
        assert_eq!(buffer.lines.len(), 4);
        assert_eq!(buffer.lines[0], "Line 1");
        assert_eq!(buffer.lines[1], "Line 2");
        assert_eq!(buffer.lines[2], "Line 3");
        assert_eq!(buffer.lines[3], "Line 4");
    }

    #[test]
    fn test_output_buffer_consecutive_newlines() {
        let mut buffer = OutputBuffer::new(100);
        buffer.push_text("Line 1\n\n\nLine 2");
        assert_eq!(buffer.lines.len(), 4);
        assert_eq!(buffer.lines[0], "Line 1");
        assert_eq!(buffer.lines[1], "");
        assert_eq!(buffer.lines[2], "");
        assert_eq!(buffer.lines[3], "Line 2");
    }

    #[test]
    fn test_output_buffer_only_newlines() {
        let mut buffer = OutputBuffer::new(100);
        buffer.push_text("\n\n\n");
        assert_eq!(buffer.lines.len(), 4);
        for line in &buffer.lines {
            assert_eq!(line, "");
        }
    }

    #[test]
    fn test_output_buffer_max_lines_exact() {
        let mut buffer = OutputBuffer::new(3);
        buffer.push_text("Line 1\nLine 2\nLine 3");
        assert_eq!(buffer.lines.len(), 3);
        assert_eq!(buffer.lines[0], "Line 1");
        assert_eq!(buffer.lines[1], "Line 2");
        assert_eq!(buffer.lines[2], "Line 3");
    }

    #[test]
    fn test_text_position_equality() {
        let pos1 = TextPosition::new(5, 10);
        let pos2 = TextPosition::new(5, 10);
        assert_eq!(pos1, pos2);
    }

    #[test]
    fn test_text_position_min_max_boundary() {
        let pos1 = TextPosition::new(0, 0);
        let pos2 = TextPosition::new(1000, 1000);
        let (min, max) = pos1.min_max(pos2);
        assert_eq!(min.line_idx, 0);
        assert_eq!(min.char_offset, 0);
        assert_eq!(max.line_idx, 1000);
        assert_eq!(max.char_offset, 1000);
    }

    #[test]
    fn test_strip_ansi_codes_complex_sequence() {
        let input = "\x1b[1;31;40mBold Red on Black\x1b[0m";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "Bold Red on Black");
    }

    #[test]
    fn test_strip_ansi_codes_multiple_resets() {
        let input = "\x1b[31mRed\x1b[0m\x1b[0m\x1b[32mGreen\x1b[0m\x1b[0m";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "RedGreen");
    }

    #[test]
    fn test_strip_ansi_codes_unterminated() {
        let input = "\x1b[31mRed text";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "Red text");
    }

    #[test]
    fn test_screen_col_to_char_offset_edge_cases() {
        let line = "abc";
        assert_eq!(screen_col_to_char_offset(line, 0), 0);
        assert_eq!(screen_col_to_char_offset(line, 1), 1);
        assert_eq!(screen_col_to_char_offset(line, 2), 2);
        assert_eq!(screen_col_to_char_offset(line, 3), 3);
        // Beyond end should return length
        assert_eq!(screen_col_to_char_offset(line, 100), 3);
    }

    #[test]
    fn test_wrap_ansi_line_single_character() {
        let line = "A";
        let result = wrap_ansi_line(line, 1);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "A");
    }

    #[test]
    fn test_wrap_ansi_line_with_only_ansi() {
        let line = "\x1b[31m\x1b[0m";
        let result = wrap_ansi_line(line, 10);
        // Should return empty or minimal wrapping
        assert!(!result.is_empty());
    }

    #[test]
    fn test_normalize_cursor_pos_at_boundary() {
        let text = "Hello";
        assert_eq!(normalize_cursor_pos(text, 5), 5);
        assert_eq!(normalize_cursor_pos(text, 6), 5);
    }

    #[test]
    fn test_previous_char_boundary_at_start() {
        let text = "Hello";
        assert_eq!(previous_char_boundary(text, 0), 0);
    }

    #[test]
    fn test_next_char_boundary_at_end() {
        let text = "Hello";
        assert_eq!(next_char_boundary(text, 5), 5);
    }

    #[test]
    fn test_cursor_position_in_lines_trailing_newline() {
        let text = "Line 1\n";
        assert_eq!(cursor_position_in_lines(text, 6), (0, 6));
        assert_eq!(cursor_position_in_lines(text, 7), (1, 0));
    }

    #[test]
    fn test_build_output_lines_with_long_lines() {
        let lines = vec!["A".repeat(1000)];
        let result = build_output_lines(&lines, 10);
        assert_eq!(result.len(), 100); // Should wrap into 100 lines
    }

    #[test]
    fn test_build_queue_lines_single_item() {
        let queue = vec!["Single item".to_string()];
        let result = build_queue_lines(&queue, 50, 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "1) Single item");
    }

    #[test]
    fn test_build_queue_lines_with_newlines() {
        let queue = vec!["Item\nwith\nnewlines".to_string()];
        let result = build_queue_lines(&queue, 50, 10);
        assert!(result.len() >= 1);
        assert!(result[0].starts_with("1) "));
    }

    #[test]
    fn test_build_input_layout_cursor_at_end() {
        let snapshot = TuiSnapshot {
            output_lines: vec![],
            queued: vec![],
            input_display: "Test".to_string(),
            input_raw: "Test".to_string(),
            cursor_pos: 4,
            output_scroll: 0,
            selection_range: None,
            todos: vec![],
        };
        let layout = build_input_layout(&snapshot, 80);
        assert_eq!(layout.cursor_col, 6); // 2 for "> " + 4 for "Test"
    }

    #[test]
    fn test_build_input_layout_with_unicode() {
        let text = "Hello 世界";
        let snapshot = TuiSnapshot {
            output_lines: vec![],
            queued: vec![],
            input_display: text.to_string(),
            input_raw: text.to_string(),
            cursor_pos: text.len(), // Use actual byte length
            output_scroll: 0,
            selection_range: None,
            todos: vec![],
        };
        let layout = build_input_layout(&snapshot, 80);
        assert!(layout.lines.len() >= 1);
        assert!(layout.lines[0].contains("Hello 世界"));
    }

    #[test]
    fn test_output_buffer_incremental_push() {
        let mut buffer = OutputBuffer::new(100);
        buffer.push_text("First");
        assert_eq!(buffer.lines.len(), 1);
        assert_eq!(buffer.lines[0], "First");

        buffer.push_text(" Second");
        assert_eq!(buffer.lines.len(), 1);
        assert_eq!(buffer.lines[0], "First Second");

        buffer.push_text("\nThird");
        assert_eq!(buffer.lines.len(), 2);
        assert_eq!(buffer.lines[1], "Third");
    }

    #[test]
    fn test_text_position_min_max_same_line_reverse() {
        let pos1 = TextPosition::new(5, 20);
        let pos2 = TextPosition::new(5, 10);
        let (min, max) = pos1.min_max(pos2);
        assert_eq!(min.line_idx, 5);
        assert_eq!(min.char_offset, 10);
        assert_eq!(max.line_idx, 5);
        assert_eq!(max.char_offset, 20);
    }

    #[test]
    fn test_cursor_position_in_lines_only_newlines() {
        let text = "\n\n\n";
        assert_eq!(cursor_position_in_lines(text, 0), (0, 0));
        assert_eq!(cursor_position_in_lines(text, 1), (1, 0));
        assert_eq!(cursor_position_in_lines(text, 2), (2, 0));
        assert_eq!(cursor_position_in_lines(text, 3), (3, 0));
    }

    #[test]
    fn test_normalize_cursor_pos_with_multibyte() {
        let text = "Hello 世界 World";
        // Test various cursor positions
        let pos = normalize_cursor_pos(text, 0);
        assert_eq!(pos, 0);
        let pos = normalize_cursor_pos(text, 6);
        assert_eq!(pos, 6);
        let pos = normalize_cursor_pos(text, text.len());
        assert_eq!(pos, text.len());
        let pos = normalize_cursor_pos(text, text.len() + 100);
        assert_eq!(pos, text.len());
    }

    #[test]
    fn test_wrap_ansi_line_exact_boundary() {
        let line = "12345";
        let result = wrap_ansi_line(line, 5);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "12345");
    }

    #[test]
    fn test_wrap_ansi_line_one_over_boundary() {
        let line = "123456";
        let result = wrap_ansi_line(line, 5);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "12345");
        assert_eq!(result[1], "6");
    }

    #[test]
    fn test_build_output_lines_single_empty_line() {
        let lines = vec![String::new()];
        let result = build_output_lines(&lines, 80);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], "");
    }

    #[test]
    fn test_build_queue_lines_very_wide_width() {
        let queue = vec!["Item 1".to_string(), "Item 2".to_string()];
        let result = build_queue_lines(&queue, 1000, 10);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], "1) Item 1");
        assert_eq!(result[1], "2) Item 2");
    }

    #[test]
    fn test_strip_ansi_complex_nesting() {
        let input = "\x1b[1m\x1b[31m\x1b[4mBold Red Underline\x1b[0m";
        let result = strip_ansi_codes(input);
        assert_eq!(result, "Bold Red Underline");
    }

    #[test]
    fn test_previous_char_boundary_multibyte_boundary() {
        let text = "世界";
        // Each character is 3 bytes
        assert_eq!(previous_char_boundary(text, 6), 3);
        assert_eq!(previous_char_boundary(text, 3), 0);
        assert_eq!(previous_char_boundary(text, 0), 0);
    }

    #[test]
    fn test_next_char_boundary_multibyte_boundary() {
        let text = "世界";
        // Each character is 3 bytes
        assert_eq!(next_char_boundary(text, 0), 3);
        assert_eq!(next_char_boundary(text, 3), 6);
        assert_eq!(next_char_boundary(text, 6), 6);
    }

    #[test]
    fn test_output_buffer_very_long_line() {
        let mut buffer = OutputBuffer::new(100);
        let long_line = "A".repeat(10000);
        buffer.push_text(&long_line);
        assert_eq!(buffer.lines.len(), 1);
        assert_eq!(buffer.lines[0].len(), 10000);
    }

    #[test]
    fn test_cursor_position_single_char() {
        let text = "A";
        assert_eq!(cursor_position_in_lines(text, 0), (0, 0));
        assert_eq!(cursor_position_in_lines(text, 1), (0, 1));
    }

    #[test]
    fn test_build_input_layout_with_very_long_input() {
        let snapshot = TuiSnapshot {
            output_lines: vec![],
            queued: vec![],
            input_display: "A".repeat(500),
            input_raw: "A".repeat(500),
            cursor_pos: 250,
            output_scroll: 0,
            selection_range: None,
            todos: vec![],
        };
        let layout = build_input_layout(&snapshot, 80);
        assert!(layout.lines.len() > 1);
    }
}
