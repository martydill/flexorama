use crate::formatter;
use crate::input::InputHistory;
use crate::output::{self, OutputSink};
use crate::security::PermissionPrompt;
use ansi_to_tui::IntoText;
use anyhow::Result;
use crossterm::{
    event::{
        self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
        Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind,
    },
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
    ExecutableCommand,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
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
            if event::poll(Duration::from_millis(50))? {
                match event::read()? {
                    Event::Key(key_event) => {
                        if let Some(result) = self.handle_key_event(key_event)? {
                            return Ok(result);
                        }
                    }
                    Event::Paste(pasted) => {
                        self.handle_paste(&pasted)?;
                    }
                    Event::Mouse(mouse_event) => {
                        self.handle_mouse_event(mouse_event.kind)?;
                    }
                    Event::Resize(_, _) => {
                        let mut guard = self.state.lock().expect("tui state lock");
                        guard.output_dirty = true;
                        drop(guard);
                        self.render()?;
                    }
                    _ => {}
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

    fn handle_mouse_event(&self, kind: MouseEventKind) -> Result<()> {
        let mut guard = self.state.lock().expect("tui state lock");
        let mut changed = false;
        match kind {
            MouseEventKind::ScrollUp => {
                guard.output_scroll = guard.output_scroll.saturating_add(3);
                changed = true;
            }
            MouseEventKind::ScrollDown => {
                guard.output_scroll = guard.output_scroll.saturating_sub(3);
                changed = true;
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

        TuiSnapshot {
            output_lines: self.output.lines.clone(),
            queued: self.queued.iter().cloned().collect(),
            input_display,
            input_raw,
            cursor_pos,
            output_scroll: self.output_scroll,
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
            let output_height = size
                .height
                .saturating_sub(input_height.saturating_add(queue_height));

            let chunks = if queue_height > 0 {
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
            let (queue_rect, input_rect) = if queue_height > 0 {
                (Some(chunks[1]), chunks[2])
            } else {
                (None, chunks[1])
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
            let output_height = size
                .height
                .saturating_sub(input_height.saturating_add(queue_height));

            let chunks = if queue_height > 0 {
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
            let (queue_rect, input_rect) = if queue_height > 0 {
                (Some(chunks[1]), chunks[2])
            } else {
                (None, chunks[1])
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
    for line in output_lines.into_iter().skip(scroll) {
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
