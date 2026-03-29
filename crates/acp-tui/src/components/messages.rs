use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Wrap};
use regex::Regex;
use std::sync::LazyLock;
use unicode_width::UnicodeWidthStr;

use acp_core::channel::{Message, MessageKind, MessageStatus, SystemKind};

use crate::theme;

pub struct MessagesView {
    lines: Vec<MessageLine>,
    scroll_offset: u16,
    total_rendered_lines: u16,
    /// Filter to a specific agent. None = show all.
    pub filter: Option<String>,
    /// Live streaming previews: (agent_name, partial_content, elapsed_secs)
    pub streaming: Vec<(String, String, Option<i64>)>,
    /// Live thinking previews: (agent_name, thinking_content)
    pub thinking: Vec<(String, String)>,
    /// Group member names (set when viewing a group tab, for filtering streaming/thinking)
    pub group_members: Option<Vec<String>>,
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
    system_kind: Option<SystemKind>,
    group: Option<String>,
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
            group_members: None,
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
            system_kind: message.system_kind.clone(),
            group: message.group.clone(),
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
                    Some(f) if f.starts_with("group:") => {
                        // Group tab: show only messages from this group
                        let group_name = &f["group:".len()..];
                        line.group.as_deref() == Some(group_name)
                    }
                    Some(agent) => {
                        // Group messages belong in group tab only
                        if line.group.is_some() {
                            return false;
                        }
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

            // Group badge in system tab (filter=None)
            if self.filter.is_none() {
                if let Some(ref group) = line.group {
                    header.push(Span::styled(
                        format!(" [{group}]"),
                        Style::default().fg(Color::Rgb(180, 140, 255)),
                    ));
                }
            }

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
                let (prefix, style) = match &line.system_kind {
                    Some(SystemKind::AgentOnline) => ("▲ ", theme::SYSTEM_ONLINE),
                    Some(SystemKind::AgentOffline) => ("▼ ", theme::SYSTEM_OFFLINE),
                    Some(SystemKind::AgentComplete) => ("✓ ", theme::SYSTEM_COMPLETE),
                    Some(SystemKind::AgentError) => ("✗ ", theme::SYSTEM_ERROR),
                    Some(SystemKind::QueueNotice) => ("⏳ ", theme::SYSTEM_QUEUE),
                    Some(SystemKind::Routing) => ("→ ", theme::SYSTEM_ROUTE),
                    Some(SystemKind::General) | None => ("· ", theme::SYSTEM_MSG),
                };
                text.push(Line::from(vec![
                    Span::styled(prefix, style),
                    Span::styled(line.content.clone(), style),
                ]));
            } else {
                // Render markdown for agent/user messages
                render_markdown(&line.content, &mut text);
            }

            if let Some(error) = &line.error {
                text.push(Line::from(Span::styled(
                    format!("✗ {error}"),
                    Style::default().fg(Color::Red),
                )));
            }
        }

        // Append live streaming previews
        for (name, buf, elapsed) in &self.streaming {
            if buf.is_empty() {
                continue;
            }
            // Apply filter
            if let Some(ref f) = self.filter {
                if f.starts_with("group:") {
                    if !self
                        .group_members
                        .as_ref()
                        .is_some_and(|m| m.iter().any(|member| member == name))
                    {
                        continue;
                    }
                } else if name != f {
                    continue;
                }
            }

            if !text.is_empty() {
                text.push(Line::from(""));
            }

            // Header with name + elapsed
            let elapsed_str = elapsed
                .filter(|&s| s > 0)
                .map(|s| format!(" ({s}s)"))
                .unwrap_or_default();
            text.push(Line::from(vec![
                Span::styled(
                    format!("{name}{elapsed_str}"),
                    theme::AGENT_MSG.add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" {}", "━".repeat(16)),
                    Style::default().fg(Color::Rgb(60, 60, 40)),
                ),
            ]));

            // Render streaming content with markdown, append cursor to last line
            let before = text.len();
            render_markdown(buf, &mut text);
            if text.len() > before {
                if let Some(last) = text.last_mut() {
                    last.spans
                        .push(Span::styled("▌", Style::default().fg(Color::Yellow)));
                }
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
                .any(|(n, b, _)| n == name && !b.is_empty())
            {
                continue;
            }
            // Apply filter
            if let Some(ref f) = self.filter {
                if f.starts_with("group:") {
                    if !self
                        .group_members
                        .as_ref()
                        .is_some_and(|m| m.iter().any(|member| member == name))
                    {
                        continue;
                    }
                } else if name != f {
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

        // Bottom padding — prevents last message from being clipped by input box
        text.push(Line::from(""));

        text
    }
}

/// Render markdown content to styled lines.
/// Pre-processes headers and horizontal rules (tui-markdown doesn't handle ATX headers),
/// then passes content blocks to tui-markdown for inline formatting, lists, code blocks, etc.
fn render_markdown(content: &str, out: &mut Vec<Line<'static>>) {
    let mut block_buf = String::new();
    let mut in_code_block = false;
    let code_style = Style::default().fg(Color::Rgb(180, 190, 160));

    for raw_line in content.lines() {
        let trimmed = raw_line.trim_start();

        // Code block fences
        if trimmed.starts_with("```") {
            if in_code_block {
                // Closing fence — end code block, skip fence line
                in_code_block = false;
                continue;
            } else {
                // Opening fence — flush previous block, start code block
                flush_md_block(&block_buf, out);
                block_buf.clear();
                in_code_block = true;
                continue;
            }
        }

        if in_code_block {
            // Inside code block — render with code style, no markdown processing
            out.push(Line::from(Span::styled(
                format!("  {raw_line}"),
                code_style,
            )));
            continue;
        }

        // ATX headers: # through ######
        let header = parse_header(trimmed);

        if let Some((text, style)) = header {
            flush_md_block(&block_buf, out);
            block_buf.clear();
            out.push(Line::from(Span::styled(text.to_string(), style)));
        } else if trimmed == "---" || trimmed == "***" {
            flush_md_block(&block_buf, out);
            block_buf.clear();
            out.push(Line::from(Span::styled(
                "─".repeat(40),
                Style::default().fg(Color::DarkGray),
            )));
        } else {
            if !block_buf.is_empty() {
                block_buf.push('\n');
            }
            block_buf.push_str(raw_line);
        }
    }

    flush_md_block(&block_buf, out);
}

fn parse_header(line: &str) -> Option<(&str, Style)> {
    // Match # through ###### (ATX headers)
    let h1_style = Style::default()
        .fg(Color::White)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let h2_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD | Modifier::UNDERLINED);
    let h3_style = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let h_minor = Style::default()
        .fg(Color::Rgb(120, 180, 180))
        .add_modifier(Modifier::BOLD);

    // Check from most specific (######) to least (#)
    for (prefix, style) in [
        ("###### ", h_minor),
        ("##### ", h_minor),
        ("#### ", h_minor),
        ("### ", h3_style),
        ("## ", h2_style),
        ("# ", h1_style),
    ] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return Some((rest, style));
        }
    }
    None
}

/// Pre-process a content block: convert list items and links, then pass to tui-markdown.
fn flush_md_block(block: &str, out: &mut Vec<Line<'static>>) {
    if block.trim().is_empty() {
        return;
    }

    // Pre-process: convert list bullets and numbered lists
    let mut processed = String::with_capacity(block.len());
    for line in block.lines() {
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let prefix = &line[..indent];

        if let Some(rest) = trimmed.strip_prefix("- ") {
            processed.push_str(prefix);
            processed.push_str("  • ");
            processed.push_str(rest);
        } else if let Some(rest) = trimmed.strip_prefix("* ") {
            processed.push_str(prefix);
            processed.push_str("  • ");
            processed.push_str(rest);
        } else if trimmed
            .find(". ")
            .and_then(|pos| trimmed[..pos].parse::<u32>().ok())
            .is_some()
        {
            // Numbered list: keep the number but add indent
            processed.push_str(prefix);
            processed.push_str("  ");
            processed.push_str(trimmed);
        } else {
            processed.push_str(line);
        }
        processed.push('\n');
    }

    // Pre-process: convert [text](url) to just the text (underlined by tui-markdown)
    static LINK_RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\[([^\]]+)\]\(([^)]+)\)").unwrap());
    let processed = LINK_RE.replace_all(&processed, "$1");

    let md_text = tui_markdown::from_str(&processed);
    for line in md_text.lines {
        let owned: Vec<Span<'static>> = line
            .spans
            .into_iter()
            .map(|s| Span::styled(s.content.into_owned(), s.style))
            .collect();
        out.push(Line::from(owned));
    }
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

    fn lines_text(lines: &[Line]) -> String {
        lines
            .iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn md_header_stripped() {
        let mut out = Vec::new();
        render_markdown("## Header Two\n\nBody text", &mut out);
        let text = lines_text(&out);
        assert!(!text.contains("##"), "## should be stripped, got: {text}");
        assert!(text.contains("Header Two"));
        assert!(text.contains("Body text"));
    }

    #[test]
    fn md_bold_styled() {
        let mut out = Vec::new();
        render_markdown("normal **bold** normal", &mut out);
        let has_bold = out.iter().any(|l| {
            l.spans
                .iter()
                .any(|s| s.style.add_modifier.contains(Modifier::BOLD))
        });
        assert!(has_bold, "bold text should have BOLD modifier");
    }

    #[test]
    fn md_list_rendered_as_bullet() {
        let mut out = Vec::new();
        render_markdown("- item one\n- item two", &mut out);
        let text = lines_text(&out);
        // List rendered by tui-markdown should use bullet char, not raw "-"
        assert!(text.contains("item one"), "text missing: {text}");
        // When passed as a block, tui-markdown converts "- " to "• " or similar
        assert!(
            !text.starts_with("- "),
            "raw dash prefix should be converted: {text}"
        );
    }

    #[test]
    fn md_link_hides_url() {
        let mut out = Vec::new();
        render_markdown("[Click here](https://example.com)", &mut out);
        let text = lines_text(&out);
        // Link text should be visible, raw []() syntax should be gone
        assert!(
            !text.contains("]("),
            "raw link syntax should be hidden: {text}"
        );
    }

    #[test]
    fn md_multiline_preserves_structure() {
        let mut out = Vec::new();
        render_markdown(
            "# Title\n\nParagraph.\n\n- a\n- b\n\n```\ncode\n```",
            &mut out,
        );
        let text = lines_text(&out);
        assert!(text.contains("Title"));
        assert!(text.contains("Paragraph"));
        assert!(text.contains("code"));
    }

    #[test]
    fn md_hr_rendered() {
        let mut out = Vec::new();
        render_markdown("above\n\n---\n\nbelow", &mut out);
        let text = lines_text(&out);
        assert!(text.contains("─"), "horizontal rule should render as ─");
    }

    #[test]
    fn md_code_block_hides_fences() {
        let mut out = Vec::new();
        render_markdown("```rust\nfn main() {}\n```", &mut out);
        let text = lines_text(&out);
        assert!(
            !text.contains("```"),
            "code fence markers should be hidden: {text}"
        );
        assert!(text.contains("fn main()"));
    }

    #[test]
    fn md_h4_stripped() {
        let mut out = Vec::new();
        render_markdown("#### H4 Title", &mut out);
        let text = lines_text(&out);
        assert!(!text.contains("####"), "h4 should be stripped: {text}");
        assert!(text.contains("H4 Title"));
    }

    // -- Filter tests --

    fn make_message(from: &str, to: Option<&str>, content: &str, group: Option<&str>) -> Message {
        Message {
            id: 1,
            conversation_id: 0,
            reply_to: None,
            from: from.to_string(),
            to: to.map(|s| s.to_string()),
            content: content.to_string(),
            kind: MessageKind::Chat,
            transport: acp_core::channel::MessageTransport::Ui,
            status: MessageStatus::Delivered,
            error: None,
            timestamp: 1000,
            system_kind: None,
            group: group.map(|s| s.to_string()),
        }
    }

    fn make_system_message(content: &str) -> Message {
        Message {
            id: 1,
            conversation_id: 0,
            reply_to: None,
            from: "系统".to_string(),
            to: None,
            content: content.to_string(),
            kind: MessageKind::System,
            transport: acp_core::channel::MessageTransport::Internal,
            status: MessageStatus::Delivered,
            error: None,
            timestamp: 1000,
            system_kind: Some(SystemKind::General),
            group: None,
        }
    }

    fn view_with_messages(msgs: &[Message], filter: Option<&str>) -> MessagesView {
        let mut view = MessagesView::new();
        view.filter = filter.map(|s| s.to_string());
        for msg in msgs {
            view.push(msg, None);
        }
        view
    }

    fn build_text_content(view: &MessagesView) -> String {
        let text = view.build_text(80);
        text.iter()
            .map(|l| {
                l.spans
                    .iter()
                    .map(|s| s.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn dm_filter_excludes_group_messages() {
        let msgs = vec![
            make_message("alice", None, "DM from alice", None),
            make_message("alice", None, "group msg from alice", Some("team")),
        ];
        let view = view_with_messages(&msgs, Some("alice"));
        let text = build_text_content(&view);
        assert!(
            text.contains("DM from alice"),
            "DM should be visible: {text}"
        );
        assert!(
            !text.contains("group msg from alice"),
            "group message should NOT show in DM tab: {text}"
        );
    }

    #[test]
    fn group_filter_shows_only_group_messages() {
        let msgs = vec![
            make_message("alice", None, "DM from alice", None),
            make_message("alice", None, "group msg", Some("team")),
            make_message("bob", None, "bob group msg", Some("team")),
            make_message("carol", None, "other group msg", Some("other")),
        ];
        let view = view_with_messages(&msgs, Some("group:team"));
        let text = build_text_content(&view);
        assert!(
            text.contains("group msg"),
            "team group msg should show: {text}"
        );
        assert!(
            text.contains("bob group msg"),
            "bob team msg should show: {text}"
        );
        assert!(
            !text.contains("DM from alice"),
            "DM should NOT show in group tab: {text}"
        );
        assert!(
            !text.contains("other group msg"),
            "other group's msg should NOT show: {text}"
        );
    }

    #[test]
    fn system_tab_shows_all_messages() {
        let msgs = vec![
            make_message("alice", None, "dm msg", None),
            make_message("bob", None, "group msg", Some("team")),
            make_system_message("alice 已上线"),
        ];
        let view = view_with_messages(&msgs, None);
        let text = build_text_content(&view);
        assert!(text.contains("dm msg"), "DM visible in system tab: {text}");
        assert!(
            text.contains("group msg"),
            "group visible in system tab: {text}"
        );
        assert!(text.contains("已上线"), "system msg visible: {text}");
    }

    #[test]
    fn system_tab_shows_group_badge() {
        let msgs = vec![make_message("alice", None, "group hello", Some("team"))];
        let view = view_with_messages(&msgs, None);
        let text = build_text_content(&view);
        assert!(
            text.contains("[team]"),
            "group badge should show in system tab: {text}"
        );
    }

    #[test]
    fn group_filter_shows_streaming_from_members() {
        let mut view = MessagesView::new();
        view.filter = Some("group:team".to_string());
        view.group_members = Some(vec!["alice".to_string(), "bob".to_string()]);
        view.streaming = vec![
            ("alice".to_string(), "alice typing...".to_string(), Some(3)),
            ("carol".to_string(), "carol typing...".to_string(), Some(2)),
        ];
        let text = build_text_content(&view);
        assert!(
            text.contains("alice typing"),
            "member streaming should show in group tab: {text}"
        );
        assert!(
            !text.contains("carol typing"),
            "non-member streaming should NOT show in group tab: {text}"
        );
    }

    #[test]
    fn group_filter_shows_thinking_from_members() {
        let mut view = MessagesView::new();
        view.filter = Some("group:team".to_string());
        view.group_members = Some(vec!["alice".to_string()]);
        view.thinking = vec![
            ("alice".to_string(), "thinking about it".to_string()),
            ("bob".to_string(), "also thinking".to_string()),
        ];
        let text = build_text_content(&view);
        assert!(
            text.contains("thinking about it"),
            "member thinking should show: {text}"
        );
        assert!(
            !text.contains("also thinking"),
            "non-member thinking should NOT show: {text}"
        );
    }

    #[test]
    fn dm_filter_shows_directed_messages() {
        let msgs = vec![
            make_message("you", Some("alice"), "hello alice", None),
            make_message("alice", Some("you"), "hello back", None),
        ];
        let view = view_with_messages(&msgs, Some("alice"));
        let text = build_text_content(&view);
        assert!(text.contains("hello alice"), "to-alice msg visible: {text}");
        assert!(
            text.contains("hello back"),
            "from-alice msg visible: {text}"
        );
    }
}
