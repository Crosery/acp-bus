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

/// Return a Chinese status label and its style for display in sidebar.
fn status_label(agent: &AgentDisplay) -> (String, Style) {
    if agent.status == "error" {
        return ("错误".to_string(), Style::default().fg(Color::Red));
    }
    if agent.status == "disconnected" {
        return ("断开".to_string(), Style::default().fg(Color::DarkGray));
    }
    if agent.status == "connecting" {
        return ("连接中".to_string(), Style::default().fg(Color::Yellow));
    }
    if let Some(ref target) = agent.waiting_reply_from {
        return (format!("等待 {target}"), Style::default().fg(Color::Cyan));
    }
    match agent.activity.as_deref() {
        Some("thinking") => (
            "思考中".to_string(),
            Style::default().fg(Color::Rgb(120, 100, 160)),
        ),
        Some("typing") => ("输出中".to_string(), Style::default().fg(Color::Green)),
        Some("receiving") => ("就绪".to_string(), Style::default().fg(Color::Yellow)),
        Some(tool_name) => (
            tool_name.to_string(),
            Style::default().fg(Color::Rgb(180, 160, 100)),
        ),
        None => (
            "空闲".to_string(),
            Style::default().fg(Color::Rgb(80, 90, 110)),
        ),
    }
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
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
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(160, 170, 190))
            };
            let max_name = (w as usize).saturating_sub(6);
            let name: String = agent.name.chars().take(max_name).collect();
            spans.push(Span::styled(name, name_style));

            buf.set_line(inner.x, y, &Line::from(spans), w);
            y += 1;

            // Status label line (always shown for non-System agents)
            if agent.name != "System" && y < max_y {
                let (label, label_style) = status_label(agent);
                let mut detail = vec![Span::raw("     ")]; // indent

                // Timer (if actively prompting)
                if let Some(secs) = active_for_secs(agent) {
                    let t = if secs >= 60 {
                        format!("{}m{}s ", secs / 60, secs % 60)
                    } else {
                        format!("{secs}s ")
                    };
                    detail.push(Span::styled(t, Style::default().fg(Color::Yellow)));
                }

                detail.push(Span::styled(label, label_style));

                buf.set_line(inner.x, y, &Line::from(detail), w);
                y += 1;
            }

            // No extra spacing — keep sidebar compact
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_agent(status: &str, activity: Option<&str>, waiting: Option<&str>) -> AgentDisplay {
        AgentDisplay {
            name: "test".to_string(),
            status: status.to_string(),
            activity: activity.map(|s| s.to_string()),
            adapter: None,
            session_id: None,
            prompt_start_time: None,
            waiting_reply_from: waiting.map(|s| s.to_string()),
            waiting_since: None,
            waiting_conversation_id: None,
            tool_calls: Vec::new(),
        }
    }

    #[test]
    fn status_label_idle() {
        let agent = make_agent("idle", None, None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, "空闲");
    }

    #[test]
    fn status_label_thinking() {
        let agent = make_agent("streaming", Some("thinking"), None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, "思考中");
    }

    #[test]
    fn status_label_typing() {
        let agent = make_agent("streaming", Some("typing"), None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, "输出中");
    }

    #[test]
    fn status_label_tool() {
        let agent = make_agent("streaming", Some("Read"), None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, "Read");
    }

    #[test]
    fn status_label_waiting() {
        let agent = make_agent("idle", None, Some("bob"));
        let (label, _) = status_label(&agent);
        assert_eq!(label, "等待 bob");
    }

    #[test]
    fn status_label_connecting() {
        let agent = make_agent("connecting", None, None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, "连接中");
    }

    #[test]
    fn status_label_error() {
        let agent = make_agent("error", None, None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, "错误");
    }

    #[test]
    fn status_label_disconnected() {
        let agent = make_agent("disconnected", None, None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, "断开");
    }

    #[test]
    fn status_label_receiving() {
        let agent = make_agent("streaming", Some("receiving"), None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, "就绪");
    }
}
