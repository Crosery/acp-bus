use ratatui::prelude::*;

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

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let t: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{t}…")
    }
}

fn active_for_secs(agent: &AgentDisplay) -> Option<i64> {
    let start = agent.prompt_start_time?;
    let now = chrono::Utc::now().timestamp();
    Some((now - start).max(0))
}

fn activity_badge(agent: &AgentDisplay) -> Option<(&'static str, Style)> {
    if agent.status == "error" || agent.status == "disconnected" {
        return Some(("ERR", theme::STATUS_ERROR_BADGE));
    }
    if agent.waiting_reply_from.is_some() {
        let waiting_secs = agent
            .waiting_since
            .map(|ts| (chrono::Utc::now().timestamp() - ts).max(0))
            .unwrap_or(0);
        if waiting_secs >= 30 {
            return Some(("STALL", theme::STATUS_ERROR_BADGE));
        }
        return Some(("WAIT", theme::STATUS_THINKING));
    }

    let activity = agent.activity.as_deref().unwrap_or("");
    if activity == "thinking" {
        return Some(("THINK", theme::STATUS_THINKING));
    }
    if activity.contains("tool") || activity.contains('/') {
        return Some(("TOOL", theme::STATUS_TOOL));
    }
    if agent.status == "streaming" || !activity.is_empty() {
        return Some(("BUSY", theme::STATUS_BUSY));
    }
    None
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
        if agents.is_empty() {
            return;
        }

        let mut spans = Vec::new();

        for (i, agent) in agents.iter().enumerate() {
            let is_selected = i == self.selected;
            let icon = theme::status_icon(&agent.status);
            let status_style = match agent.status.as_str() {
                "idle" => theme::STATUS_IDLE,
                "streaming" => theme::STATUS_STREAMING,
                "connecting" => theme::STATUS_CONNECTING,
                _ => theme::STATUS_DISCONNECTED,
            };

            if is_selected {
                spans.push(Span::styled("【", Style::default().fg(Color::Cyan)));
                spans.push(Span::styled(icon, status_style));
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    &agent.name,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ));
                if let Some(ref adapter) = agent.adapter {
                    spans.push(Span::styled(
                        format!(":{adapter}"),
                        Style::default().fg(Color::DarkGray),
                    ));
                }
                if let Some((label, style)) = activity_badge(agent) {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(format!(" {label} "), style));
                }
                if let Some(ref activity) = agent.activity {
                    let short = truncate(activity, 10);
                    spans.push(Span::styled(
                        format!(" {short}"),
                        Style::default().fg(Color::Yellow),
                    ));
                }
                if let Some(ref waiting_on) = agent.waiting_reply_from {
                    spans.push(Span::styled(
                        format!(" ->await {waiting_on}"),
                        Style::default().fg(Color::LightBlue),
                    ));
                }
                if let Some(seconds) = active_for_secs(agent) {
                    spans.push(Span::styled(
                        format!(" {seconds}s"),
                        Style::default().fg(Color::LightYellow),
                    ));
                }
                if let Some(conv_id) = agent.waiting_conversation_id {
                    spans.push(Span::styled(
                        format!(" #{}", conv_id),
                        Style::default().fg(Color::Gray),
                    ));
                }
                if let Some(ref sid) = agent.session_id {
                    let short = truncate(sid, 8);
                    spans.push(Span::styled(
                        format!(" [{short}]"),
                        Style::default().fg(Color::Rgb(100, 100, 100)),
                    ));
                }
                spans.push(Span::styled("】", Style::default().fg(Color::Cyan)));
            } else {
                spans.push(Span::raw(" "));
                spans.push(Span::styled(icon, status_style));
                spans.push(Span::raw(" "));
                spans.push(Span::styled(
                    &agent.name,
                    Style::default().fg(Color::DarkGray),
                ));
                if let Some((label, style)) = activity_badge(agent) {
                    spans.push(Span::raw(" "));
                    spans.push(Span::styled(format!(" {label} "), style));
                }
                if let Some(ref activity) = agent.activity {
                    let short = truncate(activity, 8);
                    spans.push(Span::styled(
                        format!(" {short}"),
                        Style::default().fg(Color::Rgb(80, 80, 80)),
                    ));
                }
                if let Some(ref waiting_on) = agent.waiting_reply_from {
                    spans.push(Span::styled(
                        format!(" ->{waiting_on}"),
                        Style::default().fg(Color::LightBlue),
                    ));
                }
                if let Some(seconds) = active_for_secs(agent) {
                    spans.push(Span::styled(
                        format!(" {seconds}s"),
                        Style::default().fg(Color::LightYellow),
                    ));
                }
                spans.push(Span::raw(" "));
            }
        }

        let line = Line::from(spans);
        buf.set_line(area.x, area.y, &line, area.width);
    }
}
