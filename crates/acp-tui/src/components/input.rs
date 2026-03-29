use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear};
use unicode_width::UnicodeWidthStr;

use crate::i18n;
use crate::theme;

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
    /// Current agent context for status display
    pub agent_name: Option<String>,
    pub agent_status: Option<String>,
    pub agent_activity: Option<String>,
    pub active_secs: Option<i64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_line_idle() {
        let mut input = InputBox::new();
        input.agent_name = Some("main".into());
        input.agent_status = Some("idle".into());
        let (text, _style) = input.format_status_line("main");
        assert!(text.contains("main"));
        assert!(text.contains(i18n::INPUT_STATUS_IDLE));
    }

    #[test]
    fn status_line_streaming() {
        let mut input = InputBox::new();
        input.agent_name = Some("w1".into());
        input.agent_status = Some("streaming".into());
        input.agent_activity = Some("typing".into());
        let (text, _) = input.format_status_line("w1");
        assert!(text.contains("w1"));
        assert!(text.contains(i18n::INPUT_STATUS_TYPING));
    }

    #[test]
    fn status_line_thinking_with_elapsed() {
        let mut input = InputBox::new();
        input.agent_name = Some("w1".into());
        input.agent_status = Some("streaming".into());
        input.agent_activity = Some("thinking".into());
        input.active_secs = Some(5);
        let (text, _) = input.format_status_line("w1");
        assert!(text.contains(i18n::INPUT_STATUS_THINKING));
        assert!(text.contains("5s"));
    }

    #[test]
    fn status_line_tool_call() {
        let mut input = InputBox::new();
        input.agent_name = Some("w1".into());
        input.agent_status = Some("streaming".into());
        input.agent_activity = Some("Read".into());
        let (text, _) = input.format_status_line("w1");
        assert!(text.contains("Read"));
    }

    #[test]
    fn status_line_error() {
        let mut input = InputBox::new();
        input.agent_name = Some("w1".into());
        input.agent_status = Some("error".into());
        let (text, style) = input.format_status_line("w1");
        assert!(text.contains("w1"));
        // error style should have red foreground
        assert_eq!(style.fg, Some(Color::Red));
    }

    #[test]
    fn placeholder_system_tab() {
        let input = InputBox::new();
        let text = input.placeholder_text();
        assert!(text.contains("输入消息"));
    }

    #[test]
    fn placeholder_agent_tab() {
        let mut input = InputBox::new();
        input.agent_name = Some("w1".into());
        let text = input.placeholder_text();
        assert!(text.contains("w1"));
    }
}

/// Commands with descriptions for the auto-complete popup.
static COMMANDS: &[(&str, &str)] = &[
    ("/add", i18n::CMD_ADD),
    ("/remove", i18n::CMD_REMOVE),
    ("/list", i18n::CMD_LIST),
    ("/adapters", i18n::CMD_ADAPTERS),
    ("/cancel", i18n::CMD_CANCEL),
    ("/group", i18n::CMD_GROUP),
    ("/save", i18n::CMD_SAVE),
    ("/help", i18n::CMD_HELP),
    ("/quit", i18n::CMD_QUIT),
];

impl Default for InputBox {
    fn default() -> Self {
        Self::new()
    }
}

impl InputBox {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor_pos: 0,
            completions: Vec::new(),
            candidates: Vec::new(),
            selected: None,
            popup_visible: false,
            agent_name: None,
            agent_status: None,
            agent_activity: None,
            active_secs: None,
        }
    }

    /// Update dynamic completions (agent names, adapter names)
    pub fn set_completions(&mut self, agent_names: Vec<String>, adapter_names: Vec<String>) {
        self.completions.clear();
        // Commands
        for (cmd, _) in COMMANDS {
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
        self.update_popup();
    }

    pub fn insert_str(&mut self, s: &str) {
        self.text.insert_str(self.cursor_pos, s);
        self.cursor_pos += s.len();
        self.update_popup();
    }

    /// Backspace: if cursor is inside or right after an `[Image-N]` marker,
    /// delete the entire marker at once. Otherwise delete one character.
    /// Returns the 1-based image index if a marker was removed, so the caller
    /// can also drop the corresponding PendingImage.
    pub fn backspace(&mut self) -> Option<usize> {
        if self.cursor_pos == 0 {
            return None;
        }
        // Check if cursor sits inside or right after an [Image-N] marker
        if let Some((start, end, idx)) = self.find_image_marker_at_cursor() {
            self.text.replace_range(start..end, "");
            self.cursor_pos = start;
            self.update_popup();
            return Some(idx);
        }
        // Normal single-char backspace
        let prev = self.text[..self.cursor_pos]
            .char_indices()
            .last()
            .map(|(i, _)| i)
            .unwrap_or(0);
        self.text.remove(prev);
        self.cursor_pos = prev;
        self.update_popup();
        None
    }

    /// Find an `[Image-N]` marker that the cursor is inside or right after.
    /// Returns (byte_start, byte_end, image_index_1based).
    fn find_image_marker_at_cursor(&self) -> Option<(usize, usize, usize)> {
        // Search backwards from cursor for '[Image-'
        let search_start = self.cursor_pos.saturating_sub(20); // markers are at most ~12 chars
        let slice = &self.text[search_start..self.cursor_pos];
        // Find the last '[Image-' in the slice before cursor
        if let Some(rel_pos) = slice.rfind("[Image-") {
            let abs_start = search_start + rel_pos;
            // Find the closing ']'
            if let Some(close_rel) = self.text[abs_start..].find(']') {
                let abs_end = abs_start + close_rel + 1;
                // Cursor must be within or at the end of the marker
                if self.cursor_pos <= abs_end {
                    // Extract the number
                    let inner = &self.text[abs_start + 7..abs_start + close_rel]; // after "[Image-"
                    if let Ok(idx) = inner.parse::<usize>() {
                        return Some((abs_start, abs_end, idx));
                    }
                }
            }
        }
        None
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

    /// Number of visual lines considering wrap at `width`.
    /// `width` is the text area width (excluding prompt).
    pub fn visual_line_count(&self, width: u16) -> u16 {
        if width == 0 {
            return 1;
        }
        let w = width as usize;
        let mut lines: u16 = 0;
        for logical_line in self.text.split('\n') {
            let line_w = logical_line.width();
            if line_w == 0 {
                lines += 1;
            } else {
                lines += line_w.div_ceil(w) as u16;
            }
        }
        lines.max(1)
    }

    pub fn dismiss_popup(&mut self) {
        self.popup_visible = false;
        self.candidates.clear();
        self.selected = None;
    }

    /// Whether the popup is currently active.
    pub fn popup_active(&self) -> bool {
        self.popup_visible && !self.candidates.is_empty()
    }

    /// Select next candidate in popup (Ctrl+N).
    pub fn select_next(&mut self) {
        if !self.popup_active() {
            return;
        }
        let idx = match self.selected {
            Some(i) => (i + 1) % self.candidates.len(),
            None => 0,
        };
        self.selected = Some(idx);
    }

    /// Select previous candidate in popup (Ctrl+P).
    pub fn select_prev(&mut self) {
        if !self.popup_active() {
            return;
        }
        let idx = match self.selected {
            Some(0) | None => self.candidates.len() - 1,
            Some(i) => i - 1,
        };
        self.selected = Some(idx);
    }

    /// Confirm selected candidate (Enter when popup active).
    /// Returns true if a selection was applied.
    pub fn confirm_selection(&mut self) -> bool {
        if !self.popup_active() {
            return false;
        }
        if let Some(idx) = self.selected {
            self.apply_candidate(idx);
        }
        self.dismiss_popup();
        true
    }

    /// Auto-update popup based on current input.
    fn update_popup(&mut self) {
        let word = self.current_word();
        if word.is_empty() {
            self.dismiss_popup();
            return;
        }
        self.candidates.clear();
        // Match commands and @agent completions
        for comp in &self.completions {
            if comp.starts_with(&word) && comp != &word {
                self.candidates.push(comp.clone());
            }
        }
        if self.candidates.is_empty() {
            self.dismiss_popup();
        } else {
            self.popup_visible = true;
            self.selected = Some(0);
        }
    }

    fn current_word(&self) -> String {
        let before_cursor = &self.text[..self.cursor_pos];
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
        let candidate = self.candidates[idx].clone();
        let before_cursor = &self.text[..self.cursor_pos];
        let start = before_cursor
            .rfind(|c: char| c.is_whitespace())
            .map(|i| i + 1)
            .unwrap_or(0);
        let after_cursor = self.text[self.cursor_pos..].to_string();

        let mut new_text = self.text[..start].to_string();
        new_text.push_str(&candidate);
        if candidate.starts_with('/') {
            new_text.push(' ');
        }
        let new_cursor = new_text.len();
        new_text.push_str(after_cursor.trim_start());

        self.text = new_text;
        self.cursor_pos = new_cursor;
    }

    /// Split text into visual rows with soft wrapping at `wrap_w` display columns.
    /// Returns Vec of (row_text, byte_start) for each visual row.
    fn wrap_lines(&self, wrap_w: usize) -> Vec<(String, usize)> {
        let mut rows: Vec<(String, usize)> = Vec::new();
        if wrap_w == 0 {
            rows.push((self.text.clone(), 0));
            return rows;
        }
        let mut byte_offset: usize = 0;
        for logical_line in self.text.split('\n') {
            if logical_line.is_empty() {
                rows.push((String::new(), byte_offset));
            } else {
                let mut row = String::new();
                let mut col: usize = 0;
                let mut row_start = byte_offset;
                for ch in logical_line.chars() {
                    let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
                    if col + cw > wrap_w && !row.is_empty() {
                        rows.push((std::mem::take(&mut row), row_start));
                        col = 0;
                        row_start = byte_offset;
                    }
                    row.push(ch);
                    col += cw;
                    byte_offset += ch.len_utf8();
                }
                if !row.is_empty() {
                    rows.push((row, row_start));
                }
            }
            byte_offset += 1; // '\n'
        }
        if rows.is_empty() {
            rows.push((String::new(), 0));
        }
        rows
    }

    /// Visual (row, col) of cursor considering soft wrap.
    fn cursor_visual_pos(&self, wrap_w: usize) -> (u16, u16) {
        let rows = self.wrap_lines(wrap_w);
        for (i, (_, byte_start)) in rows.iter().enumerate().rev() {
            if self.cursor_pos >= *byte_start {
                let col = self.text[*byte_start..self.cursor_pos].width();
                return (i as u16, col as u16);
            }
        }
        (0, 0)
    }

    /// Format agent status for display in the input border.
    pub fn format_status_line(&self, name: &str) -> (String, Style) {
        let status = self.agent_status.as_deref().unwrap_or("idle");
        let icon = match status {
            "streaming" => "●",
            "connecting" => "◌",
            "error" | "disconnected" => "✗",
            _ => "○",
        };
        let label = match self.agent_activity.as_deref() {
            Some("thinking") => i18n::INPUT_STATUS_THINKING,
            Some("typing") | Some("receiving") => i18n::INPUT_STATUS_TYPING,
            Some(tool) => tool,
            None if status == "streaming" => i18n::INPUT_STATUS_TYPING,
            None if status == "connecting" => i18n::INPUT_STATUS_CONNECTING,
            None if status == "error" => i18n::INPUT_STATUS_ERROR,
            _ => i18n::INPUT_STATUS_IDLE,
        };
        let elapsed = self
            .active_secs
            .filter(|&s| s > 0)
            .map(|s| format!(" {s}s"))
            .unwrap_or_default();
        let text = format!(" {icon} {name} · {label}{elapsed} ");
        let style = match status {
            "error" | "disconnected" => Style::default().fg(Color::Red),
            "streaming" => match self.agent_activity.as_deref() {
                Some("thinking") => Style::default().fg(Color::Rgb(140, 130, 170)),
                _ => Style::default().fg(Color::Yellow),
            },
            _ if self.agent_activity.is_some() => Style::default().fg(Color::Yellow),
            _ => Style::default().fg(Color::Rgb(80, 100, 80)),
        };
        (text, style)
    }

    /// Placeholder text when input is empty.
    pub fn placeholder_text(&self) -> String {
        match self.agent_name.as_deref() {
            Some(name) if name != "system" => i18n::placeholder_agent(name),
            _ => i18n::PLACEHOLDER_SYSTEM.into(),
        }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        let border_style = Style::default().fg(Color::Rgb(60, 80, 120));
        let block = Block::default()
            .borders(Borders::TOP)
            .border_style(border_style);
        let inner = block.inner(area);
        block.render(area, buf);

        // Agent status on the top border (right-aligned)
        if let Some(ref name) = self.agent_name {
            let (status_text, style) = self.format_status_line(name);
            let sw = status_text.width() as u16;
            let x = area.x + area.width.saturating_sub(sw + 1);
            buf.set_string(x, area.y, &status_text, style);
        }

        let prompt = "❯ ";
        let prompt_w = 2u16;
        let text_w = inner.width.saturating_sub(prompt_w) as usize;
        let rows = self.wrap_lines(text_w);

        if self.text.is_empty() {
            // Placeholder
            buf.set_string(
                inner.x,
                inner.y,
                prompt,
                Style::default().fg(Color::Rgb(100, 180, 255)),
            );
            buf.set_string(
                inner.x + prompt_w,
                inner.y,
                self.placeholder_text(),
                theme::INPUT_PLACEHOLDER,
            );
            return;
        }

        for (i, (row_text, _)) in rows.iter().enumerate() {
            let y = inner.y + i as u16;
            if y >= inner.y + inner.height {
                break;
            }
            let pfx = if i == 0 { "❯ " } else { "  " };
            buf.set_string(
                inner.x,
                y,
                pfx,
                Style::default().fg(Color::Rgb(100, 180, 255)),
            );
            buf.set_string(inner.x + prompt_w, y, row_text, Style::default());
        }
    }

    /// Render the completion popup above the input area
    pub fn render_popup(&self, input_area: Rect, buf: &mut Buffer) {
        if !self.popup_visible || self.candidates.is_empty() {
            return;
        }

        let max_visible = 10;
        let popup_height = self.candidates.len().min(max_visible) as u16 + 2; // +2 for border

        // Calculate width: command + description
        let max_content_w = self
            .candidates
            .iter()
            .map(|c| {
                let desc = command_desc(c);
                let desc_w = if desc.is_empty() {
                    0
                } else {
                    unicode_width::UnicodeWidthStr::width(desc) + 2 // " — " prefix simplified
                };
                c.len() + desc_w
            })
            .max()
            .unwrap_or(10);
        let popup_width = (max_content_w as u16 + 4).min(input_area.width);

        // Position popup above input
        let x = input_area.x + 2;
        let y = input_area.y.saturating_sub(popup_height);
        let popup_area = Rect::new(x, y, popup_width, popup_height);

        Clear.render(popup_area, buf);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(50, 60, 80)));
        let inner = block.inner(popup_area);
        block.render(popup_area, buf);

        for (i, candidate) in self
            .candidates
            .iter()
            .take(max_visible)
            .enumerate()
        {
            let row_y = inner.y + i as u16;
            if row_y >= inner.y + inner.height {
                break;
            }
            let is_sel = self.selected == Some(i);
            let desc = command_desc(candidate);

            if is_sel {
                // Fill row background
                for col in inner.x..inner.x + inner.width {
                    buf.set_string(col, row_y, " ", Style::default().bg(Color::Rgb(30, 50, 70)));
                }
                let cmd_style = Style::default()
                    .fg(Color::Cyan)
                    .bg(Color::Rgb(30, 50, 70))
                    .add_modifier(Modifier::BOLD);
                let desc_style = Style::default()
                    .fg(Color::Rgb(140, 150, 170))
                    .bg(Color::Rgb(30, 50, 70));
                buf.set_string(inner.x + 1, row_y, candidate, cmd_style);
                if !desc.is_empty() {
                    let dx = inner.x + 1 + candidate.len() as u16;
                    buf.set_string(dx, row_y, &format!(" {desc}"), desc_style);
                }
            } else {
                let cmd_style = Style::default().fg(Color::Rgb(160, 175, 200));
                let desc_style = Style::default().fg(Color::Rgb(80, 90, 110));
                buf.set_string(inner.x + 1, row_y, candidate, cmd_style);
                if !desc.is_empty() {
                    let dx = inner.x + 1 + candidate.len() as u16;
                    buf.set_string(dx, row_y, &format!(" {desc}"), desc_style);
                }
            }
        }
    }

    pub fn cursor_position(&self, area: Rect) -> (u16, u16) {
        let prompt_w: u16 = 2; // "> "
        let inner_y = area.y + 1; // below top border
        let text_w = area.width.saturating_sub(prompt_w + 1) as usize; // -1 for border
        let (row, col) = self.cursor_visual_pos(text_w);
        let x = area.x + prompt_w + col;
        let y = inner_y + row;
        (x, y)
    }
}

/// Look up a command's description from the COMMANDS table.
fn command_desc(candidate: &str) -> &'static str {
    COMMANDS
        .iter()
        .find(|(cmd, _)| *cmd == candidate)
        .map(|(_, desc)| *desc)
        .unwrap_or("")
}
