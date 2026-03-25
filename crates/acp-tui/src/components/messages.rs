use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Wrap};
use regex::Regex;
use std::sync::LazyLock;
use unicode_width::UnicodeWidthStr;

use acp_core::channel::{Message, MessageKind, MessageStatus};

use crate::theme;

static MENTION_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"@([a-zA-Z0-9_-]+)").unwrap());

pub struct MessagesView {
    lines: Vec<MessageLine>,
    scroll_offset: u16,
    total_rendered_lines: u16,
    /// Filter to a specific agent. None = show all.
    pub filter: Option<String>,
    /// Live streaming previews: (agent_name, partial_content)
    pub streaming: Vec<(String, String)>,
    /// Live thinking previews: (agent_name, thinking_content)
    pub thinking: Vec<(String, String)>,
    /// Auto-scroll to bottom on new messages (disabled when user scrolls up)
    auto_scroll: bool,
    /// Last known visible height (updated during render)
    visible_height: u16,
}

#[derive(Clone)]
struct MessageLine {
    from: String,
    to: Option<String>,
    content: String,
    kind: MessageKind,
    status: MessageStatus,
    error: Option<String>,
    timestamp: String,
    gap: Option<String>,
}

impl Default for MessagesView {
    fn default() -> Self {
        Self::new()
    }
}

impl MessagesView {
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            scroll_offset: 0,
            total_rendered_lines: 0,
            filter: None,
            streaming: Vec::new(),
            thinking: Vec::new(),
            auto_scroll: true,
            visible_height: 40,
        }
    }

    pub fn push(&mut self, message: &Message, gap: Option<i64>) {
        let ts = chrono::DateTime::from_timestamp(message.timestamp, 0)
            .map(|dt| dt.format("%H:%M:%S").to_string())
            .unwrap_or_default();

        let gap_str = gap.and_then(|g| {
            if g >= 60 {
                Some(format!("+{}m{}s", g / 60, g % 60))
            } else if g >= 2 {
                Some(format!("+{g}s"))
            } else {
                None
            }
        });

        self.lines.push(MessageLine {
            from: message.from.clone(),
            to: message.to.clone(),
            content: message.content.clone(),
            kind: message.kind.clone(),
            status: message.status.clone(),
            error: message.error.clone(),
            timestamp: ts,
            gap: gap_str,
        });
    }

    pub fn scroll_down(&mut self, n: u16) {
        self.scroll_offset = self.scroll_offset.saturating_add(n);
        // Re-enable auto-scroll if we're at or near the bottom
        let max_offset = self
            .total_rendered_lines
            .saturating_sub(self.visible_height);
        if self.scroll_offset >= max_offset {
            self.auto_scroll = true;
        }
    }

    pub fn scroll_up(&mut self, n: u16) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
        self.auto_scroll = false;
    }

    pub fn scroll_to_bottom(&mut self, _visible_height: u16) {
        // Now handled in render() after total_rendered_lines is updated.
        // This method is kept for API compatibility but is a no-op.
        // auto_scroll flag controls the behavior in render().
    }

    pub fn snap_to_bottom(&mut self) {
        self.auto_scroll = true;
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
        self.auto_scroll = false;
    }

    pub fn render(&mut self, area: Rect, buf: &mut Buffer) {
        // No border — messages fill the area for maximum chat space
        let inner = Rect {
            x: area.x + 1,
            y: area.y,
            width: area.width.saturating_sub(2),
            height: area.height,
        };

        self.visible_height = inner.height;
        let text = self.build_text(inner.width);

        // Calculate actual rendered lines accounting for word wrap and CJK double-width
        let wrapped_lines: u16 = if inner.width > 0 {
            text.iter()
                .map(|line| {
                    let w: usize = line.spans.iter().map(|s| s.content.width()).sum();
                    if w == 0 {
                        1u16
                    } else {
                        (w as u16).div_ceil(inner.width).max(1)
                    }
                })
                .sum()
        } else {
            text.len() as u16
        };
        self.total_rendered_lines = wrapped_lines;

        // Auto-scroll: keep latest content visible (like Claude Code)
        if self.auto_scroll {
            if self.total_rendered_lines > inner.height {
                self.scroll_offset = self.total_rendered_lines - inner.height;
            } else {
                self.scroll_offset = 0;
            }
        }

        let paragraph = Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0));
        paragraph.render(inner, buf);
    }

    fn build_text(&self, _width: u16) -> Vec<Line<'static>> {
        let mut text: Vec<Line<'static>> = Vec::new();

        // Filter lines based on selected agent
        let filtered: Vec<&MessageLine> = self
            .lines
            .iter()
            .filter(|line| {
                match &self.filter {
                    None => true, // System tab: show ALL messages
                    Some(agent) => {
                        // System messages (grey) — ONLY in System tab
                        if line.kind == MessageKind::System || line.kind == MessageKind::Audit {
                            return false;
                        }
                        // Messages FROM this agent
                        if line.from == *agent {
                            return true;
                        }
                        // Messages directed TO this agent
                        if line.to.as_deref() == Some(agent.as_str()) {
                            return true;
                        }
                        // Broadcast messages that @mention this agent
                        if line.to.is_none() && line.content.contains(&format!("@{agent}")) {
                            return true;
                        }
                        false
                    }
                }
            })
            .collect();

        for (i, line) in filtered.iter().enumerate() {
            // Blank line between messages (cleaner than heavy separators)
            if i > 0 {
                text.push(Line::from(""));
            }

            // Header: name + direction + timestamp (compact single line)
            let name_style = if line.status == MessageStatus::Failed {
                Style::default().fg(Color::LightRed)
            } else if line.from == "系统" {
                theme::SYSTEM_MSG
            } else if line.from == "you" || line.from == "你" {
                theme::USER_MSG
            } else if line.kind == MessageKind::Task {
                Style::default().fg(Color::LightCyan)
            } else {
                theme::AGENT_MSG
            };

            let mut header = vec![];

            // Name with direction arrow
            let name_text = match &line.to {
                Some(to) => format!("{} → {}", line.from, to),
                None => line.from.clone(),
            };
            header.push(Span::styled(
                name_text,
                name_style.add_modifier(Modifier::BOLD),
            ));

            // Timestamp (dimmer, right after name)
            if !line.timestamp.is_empty() {
                let ts_text = if let Some(ref gap) = line.gap {
                    format!("  {} · {}", line.timestamp, gap)
                } else {
                    format!("  {}", line.timestamp)
                };
                header.push(Span::styled(ts_text, theme::TIMESTAMP));
            }
            text.push(Line::from(header));

            // Content
            if line.kind == MessageKind::System || line.kind == MessageKind::Audit {
                text.push(Line::from(Span::styled(
                    line.content.clone(),
                    theme::SYSTEM_MSG,
                )));
            } else {
                for content_line in line.content.lines() {
                    text.push(Line::from(highlight_mentions(content_line)));
                }
            }

            if let Some(error) = &line.error {
                text.push(Line::from(Span::styled(
                    format!("✗ {error}"),
                    Style::default().fg(Color::Red),
                )));
            }
        }

        // Append live streaming previews
        for (name, buf) in &self.streaming {
            if buf.is_empty() {
                continue;
            }
            // Apply filter
            if let Some(ref f) = self.filter {
                if name != f {
                    continue;
                }
            }

            if !text.is_empty() {
                text.push(Line::from(""));
            }

            text.push(Line::from(vec![
                Span::styled(name.clone(), theme::AGENT_MSG.add_modifier(Modifier::BOLD)),
                Span::styled("  ▌", Style::default().fg(Color::Yellow)),
            ]));
            for content_line in buf.lines() {
                text.push(Line::from(highlight_mentions(content_line)));
            }
        }

        // Append live thinking previews (only when not already streaming text)
        for (name, buf) in &self.thinking {
            if buf.is_empty() {
                continue;
            }
            // Skip if this agent already has a streaming preview
            if self
                .streaming
                .iter()
                .any(|(n, b)| n == name && !b.is_empty())
            {
                continue;
            }
            // Apply filter
            if let Some(ref f) = self.filter {
                if name != f {
                    continue;
                }
            }

            if !text.is_empty() {
                text.push(Line::from(""));
            }

            text.push(Line::from(vec![
                Span::styled(name.clone(), theme::AGENT_MSG.add_modifier(Modifier::BOLD)),
                Span::styled(
                    "  ...".to_string(),
                    Style::default().fg(Color::Rgb(120, 100, 160)),
                ),
            ]));
            let lines: Vec<&str> = buf.lines().collect();
            let start = lines.len().saturating_sub(5);
            for line in &lines[start..] {
                text.push(Line::from(format_thinking_line(line)));
            }
        }

        text
    }
}

/// Highlight @mentions in yellow, rest in default color
fn highlight_mentions(text: &str) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut last_end = 0;

    for mat in MENTION_RE.find_iter(text) {
        if mat.start() > last_end {
            spans.push(Span::raw(text[last_end..mat.start()].to_string()));
        }
        spans.push(Span::styled(mat.as_str().to_string(), theme::MENTION));
        last_end = mat.end();
    }

    if last_end < text.len() {
        spans.push(Span::raw(text[last_end..].to_string()));
    }

    if spans.is_empty() {
        spans.push(Span::raw(text.to_string()));
    }

    spans
}

/// Format a thinking line with ┊ prefix and dim italic style.
/// Unlike normal messages, thinking lines do NOT highlight @mentions.
fn format_thinking_line(text: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled("┊ ".to_string(), theme::THINKING_PREFIX),
        Span::styled(text.to_string(), theme::THINKING_TEXT),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spans_text(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    #[test]
    fn format_thinking_line_adds_prefix() {
        let spans = format_thinking_line("让我分析一下");
        let text = spans_text(&spans);
        assert!(text.starts_with("┊ "));
        assert!(text.contains("让我分析一下"));
    }

    #[test]
    fn format_thinking_line_empty_input() {
        let spans = format_thinking_line("");
        let text = spans_text(&spans);
        assert_eq!(text, "┊ ");
    }

    #[test]
    fn format_thinking_line_preserves_content_exactly() {
        let spans = format_thinking_line("hello @world");
        let text = spans_text(&spans);
        assert_eq!(text, "┊ hello @world");
    }
}
