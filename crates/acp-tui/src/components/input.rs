use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear};
use unicode_width::UnicodeWidthStr;

pub struct InputBox {
    pub text: String,
    pub cursor_pos: usize,
    /// Available completions (commands, agent names)
    completions: Vec<String>,
    /// Currently visible completion candidates
    candidates: Vec<String>,
    /// Selected candidate index
    selected: Option<usize>,
    /// Whether popup is visible
    popup_visible: bool,
}

static COMMANDS: &[&str] = &[
    "/add",
    "/remove",
    "/list",
    "/adapters",
    "/cancel",
    "/help",
    "/quit",
    "/save",
];

impl InputBox {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor_pos: 0,
            completions: Vec::new(),
            candidates: Vec::new(),
            selected: None,
            popup_visible: false,
        }
    }

    /// Update dynamic completions (agent names, adapter names)
    pub fn set_completions(&mut self, agent_names: Vec<String>, adapter_names: Vec<String>) {
        self.completions.clear();
        // Commands
        for cmd in COMMANDS {
            self.completions.push(cmd.to_string());
        }
        // @agent completions
        for name in &agent_names {
            self.completions.push(format!("@{name}"));
        }
        // Adapter names (for /add completion)
        for name in &adapter_names {
            self.completions.push(name.clone());
        }
    }

    pub fn insert(&mut self, c: char) {
        self.text.insert(self.cursor_pos, c);
        self.cursor_pos += c.len_utf8();
        self.dismiss_popup();
    }

    pub fn insert_str(&mut self, s: &str) {
        self.text.insert_str(self.cursor_pos, s);
        self.cursor_pos += s.len();
        self.dismiss_popup();
    }

    pub fn backspace(&mut self) {
        if self.cursor_pos > 0 {
            let prev = self.text[..self.cursor_pos]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.text.remove(prev);
            self.cursor_pos = prev;
            self.dismiss_popup();
        }
    }

    pub fn delete(&mut self) {
        if self.cursor_pos < self.text.len() {
            self.text.remove(self.cursor_pos);
        }
    }

    pub fn move_left(&mut self) {
        if self.cursor_pos > 0 {
            self.cursor_pos = self.text[..self.cursor_pos]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    pub fn move_right(&mut self) {
        if self.cursor_pos < self.text.len() {
            self.cursor_pos = self.text[self.cursor_pos..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor_pos + i)
                .unwrap_or(self.text.len());
        }
    }

    pub fn move_home(&mut self) {
        self.cursor_pos = 0;
    }

    pub fn move_end(&mut self) {
        self.cursor_pos = self.text.len();
    }

    pub fn take(&mut self) -> String {
        self.dismiss_popup();
        let text = std::mem::take(&mut self.text);
        self.cursor_pos = 0;
        text
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn dismiss_popup(&mut self) {
        self.popup_visible = false;
        self.candidates.clear();
        self.selected = None;
    }

    /// Handle Tab key: trigger or cycle completions
    pub fn tab(&mut self) {
        if self.popup_visible && !self.candidates.is_empty() {
            // Cycle to next candidate
            let idx = match self.selected {
                Some(i) => (i + 1) % self.candidates.len(),
                None => 0,
            };
            self.selected = Some(idx);
            self.apply_candidate(idx);
        } else {
            // Build candidates from current input
            self.build_candidates();
            if self.candidates.len() == 1 {
                // Single match: apply directly
                self.apply_candidate(0);
                self.dismiss_popup();
            } else if !self.candidates.is_empty() {
                self.popup_visible = true;
                self.selected = Some(0);
                self.apply_candidate(0);
            }
        }
    }

    /// Handle Shift+Tab: cycle backwards
    pub fn shift_tab(&mut self) {
        if self.popup_visible && !self.candidates.is_empty() {
            let idx = match self.selected {
                Some(0) | None => self.candidates.len() - 1,
                Some(i) => i - 1,
            };
            self.selected = Some(idx);
            self.apply_candidate(idx);
        }
    }

    fn build_candidates(&mut self) {
        self.candidates.clear();
        let input = &self.text;

        if input.is_empty() {
            return;
        }

        // Get the current word being typed
        let current_word = self.current_word();

        if current_word.is_empty() {
            return;
        }

        // Match against completions
        for comp in &self.completions {
            if comp.starts_with(&current_word) && comp != &current_word {
                self.candidates.push(comp.clone());
            }
        }

        // For /add command, second arg = agent name, third arg = adapter
        let parts: Vec<&str> = input.split_whitespace().collect();
        if parts.len() >= 1 && parts[0] == "/add" && parts.len() == 2 {
            // Completing adapter name - already handled by completions
        }
    }

    fn current_word(&self) -> String {
        let before_cursor = &self.text[..self.cursor_pos];
        // Find start of current word
        let start = before_cursor
            .rfind(|c: char| c.is_whitespace())
            .map(|i| i + 1)
            .unwrap_or(0);
        before_cursor[start..].to_string()
    }

    fn apply_candidate(&mut self, idx: usize) {
        if idx >= self.candidates.len() {
            return;
        }
        let candidate = &self.candidates[idx];
        let before_cursor = &self.text[..self.cursor_pos];
        let start = before_cursor
            .rfind(|c: char| c.is_whitespace())
            .map(|i| i + 1)
            .unwrap_or(0);
        let after_cursor = &self.text[self.cursor_pos..];

        let mut new_text = self.text[..start].to_string();
        new_text.push_str(candidate);
        // Add space after completion if it's a command
        if candidate.starts_with('/') {
            new_text.push(' ');
        }
        let new_cursor = new_text.len();
        new_text.push_str(after_cursor.trim_start());

        self.text = new_text;
        self.cursor_pos = new_cursor;
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        // Keep the old plain input style, but reserve a clean line so it stays visible.
        buf.set_string(
            area.x,
            area.y,
            " ".repeat(area.width as usize),
            Style::default(),
        );
        if area.height > 1 {
            buf.set_string(
                area.x,
                area.y + 1,
                " ".repeat(area.width as usize),
                Style::default(),
            );
        }
        let prompt = Span::styled("> ", Style::default().fg(Color::DarkGray));
        let text = Span::raw(self.text.as_str());
        let line = Line::from(vec![prompt, text]);
        buf.set_line(area.x, area.y, &line, area.width);
    }

    /// Render the completion popup above the input area
    pub fn render_popup(&self, input_area: Rect, buf: &mut Buffer) {
        if !self.popup_visible || self.candidates.is_empty() {
            return;
        }

        let popup_height = self.candidates.len().min(6) as u16;
        let popup_width = self
            .candidates
            .iter()
            .map(|s| s.len())
            .max()
            .unwrap_or(10)
            .max(10) as u16
            + 4;

        // Position popup above input
        let x = input_area.x + 1;
        let y = input_area.y.saturating_sub(popup_height + 1);
        let popup_area = Rect::new(x, y, popup_width.min(input_area.width), popup_height);

        // Clear background
        Clear.render(popup_area, buf);

        // Draw popup block
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray));
        let inner = block.inner(popup_area);
        block.render(popup_area, buf);

        // Render candidates
        for (i, candidate) in self
            .candidates
            .iter()
            .take(popup_height as usize)
            .enumerate()
        {
            let style = if self.selected == Some(i) {
                Style::default().fg(Color::Black).bg(Color::Cyan)
            } else {
                Style::default().fg(Color::White)
            };
            let y = inner.y + i as u16;
            if y < inner.y + inner.height {
                buf.set_string(inner.x, y, candidate, style);
            }
        }
    }

    pub fn cursor_position(&self, area: Rect) -> (u16, u16) {
        // +2 for "> " prompt, use display width for CJK
        let display_width = self.text[..self.cursor_pos].width() as u16;
        let x = area.x + 2 + display_width;
        let y = area.y;
        (x, y)
    }
}
