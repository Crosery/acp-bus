use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders};

use crate::theme;

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
}

pub struct StatusBar {
    pub selected: usize,
}

fn active_for_secs(agent: &AgentDisplay) -> Option<i64> {
    let start = agent.prompt_start_time?;
    let now = chrono::Utc::now().timestamp();
    Some((now - start).max(0))
}

fn status_label(agent: &AgentDisplay) -> (&'static str, Style) {
    if agent.status == "error" || agent.status == "disconnected" {
        return ("ERR", theme::STATUS_ERROR_BADGE);
    }
    if agent.waiting_reply_from.is_some() {
        return ("WAIT", theme::STATUS_THINKING);
    }
    let activity = agent.activity.as_deref().unwrap_or("");
    if activity == "thinking" {
        return ("…", Style::default().fg(Color::Yellow));
    }
    if agent.status == "streaming" || !activity.is_empty() {
        return ("●", Style::default().fg(Color::Green));
    }
    if agent.status == "connecting" {
        return ("◌", Style::default().fg(Color::DarkGray));
    }
    if agent.status == "idle" {
        return ("○", Style::default().fg(Color::DarkGray));
    }
    ("?", Style::default().fg(Color::DarkGray))
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

        for (i, agent) in agents.iter().enumerate() {
            if i as u16 >= inner.height {
                break;
            }
            let y = inner.y + i as u16;
            let is_selected = i == self.selected;

            let (icon, icon_style) = status_label(agent);

            let mut spans = vec![];

            // Selection indicator
            if is_selected {
                spans.push(Span::styled("▸", Style::default().fg(Color::Cyan)));
            } else {
                spans.push(Span::raw(" "));
            }

            // Status icon
            spans.push(Span::styled(icon, icon_style));

            // Agent name
            let name_style = if is_selected {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(140, 150, 170))
            };
            let max_name = (inner.width as usize).saturating_sub(3);
            let name: String = agent.name.chars().take(max_name).collect();
            spans.push(Span::styled(name, name_style));

            // Timer (if active, show on same line if space permits)
            if let Some(secs) = active_for_secs(agent) {
                let timer = format!(" {secs}s");
                if spans.iter().map(|s| s.content.len()).sum::<usize>() + timer.len() <= inner.width as usize {
                    spans.push(Span::styled(timer, Style::default().fg(Color::Yellow)));
                }
            }

            let line = Line::from(spans);
            buf.set_line(inner.x, y, &line, inner.width);
        }
    }
}
