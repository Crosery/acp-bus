use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders};

pub struct ToolCallDisplay {
    pub name: String,
    pub running: bool,
}

pub struct AgentDisplay {
    pub name: String,
    pub status: String,
    pub activity: Option<String>,
    pub adapter: Option<String>,
    pub session_id: Option<String>,
    pub prompt_start_time: Option<i64>,
    pub waiting_reply_from: Option<String>,
    pub waiting_since: Option<i64>,
    pub waiting_conversation_id: Option<u64>,
    pub tool_calls: Vec<ToolCallDisplay>,
}

pub struct StatusBar {
    pub selected: usize,
}

fn active_for_secs(agent: &AgentDisplay) -> Option<i64> {
    let start = agent.prompt_start_time?;
    let now = chrono::Utc::now().timestamp();
    Some((now - start).max(0))
}

fn status_char(agent: &AgentDisplay) -> (&'static str, Style) {
    if agent.status == "error" || agent.status == "disconnected" {
        ("✗", Style::default().fg(Color::Red))
    } else if agent.status == "streaming" || agent.activity.is_some() {
        ("●", Style::default().fg(Color::Green))
    } else if agent.status == "connecting" {
        ("◌", Style::default().fg(Color::Yellow))
    } else if agent.waiting_reply_from.is_some() {
        ("◎", Style::default().fg(Color::Cyan))
    } else {
        ("○", Style::default().fg(Color::Rgb(80, 90, 110)))
    }
}

impl StatusBar {
    pub fn new() -> Self {
        Self { selected: 0 }
    }

    pub fn select_next(&mut self, total: usize) {
        if total > 0 {
            self.selected = (self.selected + 1) % total;
        }
    }

    pub fn select_prev(&mut self, total: usize) {
        if total > 0 {
            self.selected = self.selected.checked_sub(1).unwrap_or(total - 1);
        }
    }

    pub fn render(&self, agents: &[AgentDisplay], area: Rect, buf: &mut Buffer) {
        let block = Block::default()
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(Color::Rgb(40, 50, 70)));
        let inner = block.inner(area);
        block.render(area, buf);

        if agents.is_empty() || inner.width < 4 {
            return;
        }

        let mut y = inner.y + 1; // top padding
        let max_y = inner.y + inner.height;
        let w = inner.width;

        for (i, agent) in agents.iter().enumerate() {
            if y >= max_y {
                break;
            }

            let is_selected = i == self.selected;
            let (icon, icon_style) = status_char(agent);

            // Agent line
            let mut spans = vec![];
            if is_selected {
                spans.push(Span::styled(" ▸ ", Style::default().fg(Color::Cyan)));
            } else {
                spans.push(Span::raw("   "));
            }
            spans.push(Span::styled(format!("{icon} "), icon_style));

            let name_style = if is_selected {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(160, 170, 190))
            };
            let max_name = (w as usize).saturating_sub(6);
            let name: String = agent.name.chars().take(max_name).collect();
            spans.push(Span::styled(name, name_style));

            buf.set_line(inner.x, y, &Line::from(spans), w);
            y += 1;

            // Timer + current activity (second line, indented)
            let has_detail = active_for_secs(agent).is_some()
                || agent.waiting_reply_from.is_some()
                || agent.tool_calls.iter().any(|tc| tc.running);

            if has_detail && y < max_y {
                let mut detail = vec![Span::raw("     ")]; // indent

                if let Some(secs) = active_for_secs(agent) {
                    let t = if secs >= 60 {
                        format!("{}m{}s", secs / 60, secs % 60)
                    } else {
                        format!("{secs}s")
                    };
                    detail.push(Span::styled(t, Style::default().fg(Color::Yellow)));
                }

                // Show current running tool
                if let Some(tc) = agent.tool_calls.iter().find(|tc| tc.running) {
                    let max_tool = (w as usize).saturating_sub(12);
                    let tool_name: String = tc.name.chars().take(max_tool).collect();
                    detail.push(Span::styled(
                        format!(" {tool_name}"),
                        Style::default().fg(Color::Rgb(180, 160, 100)),
                    ));
                }

                // Show waiting target
                if let Some(ref target) = agent.waiting_reply_from {
                    let max_t = (w as usize).saturating_sub(10);
                    let t: String = target.chars().take(max_t).collect();
                    detail.push(Span::styled(
                        format!(" →{t}"),
                        Style::default().fg(Color::Cyan),
                    ));
                }

                buf.set_line(inner.x, y, &Line::from(detail), w);
                y += 1;
            }

            // No extra spacing — keep sidebar compact
        }
    }
}
