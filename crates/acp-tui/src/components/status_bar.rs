use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders};

use crate::i18n;

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
    pub context_tokens: Option<(u64, u64)>,
}

pub struct GroupDisplay {
    pub name: String,
    pub member_count: usize,
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarMode {
    Agents,
    Groups,
}

pub struct StatusBar {
    pub selected: usize,
    pub mode: SidebarMode,
}

/// Render keybinding hints vertically at the bottom of the sidebar.
fn render_sidebar_hints(x: u16, max_y: u16, w: u16, buf: &mut Buffer) {
    let hints: &[(&str, &str)] = i18n::SIDEBAR_HINTS;
    let rows_needed = hints.len() as u16 + 1; // +1 for separator
    let start_y = max_y.saturating_sub(rows_needed);
    if start_y < 6 {
        return; // not enough space
    }
    // Separator
    let sep: String = "─".repeat(w as usize);
    buf.set_string(x, start_y, &sep, Style::default().fg(Color::Rgb(40, 50, 65)));
    // One hint per row
    let key_style = Style::default().fg(Color::Rgb(90, 190, 210));
    let desc_style = Style::default().fg(Color::Rgb(100, 110, 130));
    for (i, (key, desc)) in hints.iter().enumerate() {
        let y = start_y + 1 + i as u16;
        if y >= max_y {
            break;
        }
        buf.set_line(
            x,
            y,
            &Line::from(vec![
                Span::styled(format!(" {key:<7}"), key_style),
                Span::styled(*desc, desc_style),
            ]),
            w,
        );
    }
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

/// Return a status label and its style for display in sidebar.
fn status_label(agent: &AgentDisplay) -> (String, Style) {
    if agent.status == "error" {
        return (i18n::STATUS_ERROR.to_string(), Style::default().fg(Color::Red));
    }
    if agent.status == "disconnected" {
        return (i18n::STATUS_DISCONNECTED.to_string(), Style::default().fg(Color::DarkGray));
    }
    if agent.status == "connecting" {
        return (i18n::STATUS_CONNECTING.to_string(), Style::default().fg(Color::Yellow));
    }
    if let Some(ref target) = agent.waiting_reply_from {
        return (i18n::status_waiting(target), Style::default().fg(Color::Cyan));
    }
    match agent.activity.as_deref() {
        Some("thinking") => (
            i18n::STATUS_THINKING.to_string(),
            Style::default().fg(Color::Rgb(120, 100, 160)),
        ),
        Some("typing") => (i18n::STATUS_TYPING.to_string(), Style::default().fg(Color::Green)),
        Some("receiving") => (i18n::STATUS_READY.to_string(), Style::default().fg(Color::Yellow)),
        Some(tool_name) => (
            tool_name.to_string(),
            Style::default().fg(Color::Rgb(180, 160, 100)),
        ),
        None => (
            i18n::STATUS_IDLE.to_string(),
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
        Self {
            selected: 0,
            mode: SidebarMode::Agents,
        }
    }

    pub fn toggle_mode(&mut self) {
        self.mode = match self.mode {
            SidebarMode::Agents => SidebarMode::Groups,
            SidebarMode::Groups => SidebarMode::Agents,
        };
        self.selected = 0;
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

    pub fn render(
        &self,
        agents: &[AgentDisplay],
        groups: &[GroupDisplay],
        cwd: &str,
        area: Rect,
        buf: &mut Buffer,
    ) {
        let block = Block::default()
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(Color::Rgb(40, 50, 70)));
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.width < 4 {
            return;
        }

        let mut y = inner.y;
        let max_y = inner.y + inner.height;
        let w = inner.width;

        // Project path at top (breadcrumb style)
        {
            let home = std::env::var("HOME").unwrap_or_default();
            let display = if !home.is_empty() && cwd.starts_with(&home) {
                format!("~{}", &cwd[home.len()..])
            } else {
                cwd.to_string()
            };
            let sep_style = Style::default().fg(Color::Rgb(50, 60, 80));
            let seg_style = Style::default().fg(Color::Rgb(140, 155, 180));
            let last_style = Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD);
            let parts: Vec<&str> = display.split('/').filter(|s| !s.is_empty()).collect();
            let mut spans = vec![Span::raw(" ")];
            let last_idx = parts.len().saturating_sub(1);
            for (i, part) in parts.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::styled("/", sep_style));
                }
                let style = if i == last_idx { last_style } else { seg_style };
                spans.push(Span::styled(*part, style));
            }
            buf.set_line(inner.x, y, &Line::from(spans), w);
            y += 1;
        }

        // Mode tabs: [私聊] [群组]
        {
            let agents_style = if self.mode == SidebarMode::Agents {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(100, 110, 130))
            };
            let groups_style = if self.mode == SidebarMode::Groups {
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Rgb(180, 140, 255))
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(100, 110, 130))
            };
            buf.set_line(
                inner.x,
                y,
                &Line::from(vec![
                    Span::styled(i18n::TAB_DM, agents_style),
                    Span::raw(" "),
                    Span::styled(i18n::TAB_GROUPS, groups_style),
                ]),
                w,
            );
            y += 1;
            // Separator
            let sep = "─".repeat(w as usize);
            buf.set_string(
                inner.x,
                y,
                &sep,
                Style::default().fg(Color::Rgb(50, 60, 80)),
            );
            y += 1;
        }

        match self.mode {
            SidebarMode::Agents => {
                self.render_agents(agents, inner.x, &mut y, max_y, w, buf);
            }
            SidebarMode::Groups => {
                self.render_groups(groups, inner.x, &mut y, max_y, w, buf);
            }
        }

        // Keybinding hints at sidebar bottom
        render_sidebar_hints(inner.x, max_y, w, buf);
    }

    fn render_agents(
        &self,
        agents: &[AgentDisplay],
        x: u16,
        y: &mut u16,
        max_y: u16,
        w: u16,
        buf: &mut Buffer,
    ) {
        for (i, agent) in agents.iter().enumerate() {
            if *y >= max_y {
                break;
            }

            let is_selected = i == self.selected;
            let (icon, icon_style) = status_char(agent);

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

            buf.set_line(x, *y, &Line::from(spans), w);
            *y += 1;

            // Status label (skip for System)
            if agent.name != "System" && *y < max_y {
                let (label, label_style) = status_label(agent);
                let mut detail = vec![Span::raw("     ")];

                if let Some(secs) = active_for_secs(agent) {
                    let t = if secs >= 60 {
                        format!("{}m{}s ", secs / 60, secs % 60)
                    } else {
                        format!("{secs}s ")
                    };
                    detail.push(Span::styled(t, Style::default().fg(Color::Yellow)));
                }

                detail.push(Span::styled(label, label_style));

                // Append context token usage inline
                if let Some((input, max)) = agent.context_tokens {
                    if max > 0 {
                        let pct = (input * 100 / max) as u8;
                        let style = if pct >= 95 {
                            crate::theme::CONTEXT_CRITICAL
                        } else if pct >= 80 {
                            crate::theme::CONTEXT_HIGH
                        } else {
                            crate::theme::CONTEXT_NORMAL
                        };
                        detail.push(Span::styled(
                            format!(" {}%", pct),
                            style,
                        ));
                    }
                }

                buf.set_line(x, *y, &Line::from(detail), w);
                *y += 1;
            }
        }
    }

    fn render_groups(
        &self,
        groups: &[GroupDisplay],
        x: u16,
        y: &mut u16,
        max_y: u16,
        w: u16,
        buf: &mut Buffer,
    ) {
        if groups.is_empty() {
            buf.set_line(
                x,
                *y,
                &Line::from(Span::styled(
                    format!("   {}", i18n::NO_GROUPS),
                    Style::default().fg(Color::Rgb(80, 90, 110)),
                )),
                w,
            );
            *y += 1;
            buf.set_line(
                x,
                *y,
                &Line::from(Span::styled(
                    format!("   {}", i18n::NO_GROUPS_HINT),
                    Style::default().fg(Color::Rgb(60, 70, 90)),
                )),
                w,
            );
            return;
        }

        for (gi, group) in groups.iter().enumerate() {
            if *y >= max_y {
                break;
            }
            let is_selected = gi == self.selected;

            let mut spans = vec![];
            if is_selected {
                spans.push(Span::styled(" ▸ ", Style::default().fg(Color::Cyan)));
            } else {
                spans.push(Span::raw("   "));
            }
            spans.push(Span::styled(
                "◈ ",
                Style::default().fg(Color::Rgb(180, 140, 255)),
            ));
            let name_style = if is_selected {
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Rgb(180, 140, 255))
            };
            spans.push(Span::styled(group.name.clone(), name_style));
            spans.push(Span::styled(
                format!(" ({})", group.member_count),
                Style::default().fg(Color::Rgb(100, 100, 120)),
            ));
            buf.set_line(x, *y, &Line::from(spans), w);
            *y += 1;

            // Show members when selected
            if is_selected {
                for member in &group.members {
                    if *y >= max_y {
                        break;
                    }
                    buf.set_line(
                        x,
                        *y,
                        &Line::from(Span::styled(
                            format!("     · {member}"),
                            Style::default().fg(Color::Rgb(120, 130, 150)),
                        )),
                        w,
                    );
                    *y += 1;
                }
            }
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
            context_tokens: None,
        }
    }

    #[test]
    fn status_label_idle() {
        let agent = make_agent("idle", None, None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, i18n::STATUS_IDLE);
    }

    #[test]
    fn status_label_thinking() {
        let agent = make_agent("streaming", Some("thinking"), None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, i18n::STATUS_THINKING);
    }

    #[test]
    fn status_label_typing() {
        let agent = make_agent("streaming", Some("typing"), None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, i18n::STATUS_TYPING);
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
        assert_eq!(label, i18n::status_waiting("bob"));
    }

    #[test]
    fn status_label_connecting() {
        let agent = make_agent("connecting", None, None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, i18n::STATUS_CONNECTING);
    }

    #[test]
    fn status_label_error() {
        let agent = make_agent("error", None, None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, i18n::STATUS_ERROR);
    }

    #[test]
    fn status_label_disconnected() {
        let agent = make_agent("disconnected", None, None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, i18n::STATUS_DISCONNECTED);
    }

    #[test]
    fn status_label_receiving() {
        let agent = make_agent("streaming", Some("receiving"), None);
        let (label, _) = status_label(&agent);
        assert_eq!(label, i18n::STATUS_READY);
    }

    #[test]
    fn toggle_mode_switches() {
        let mut sb = StatusBar::new();
        assert_eq!(sb.mode, SidebarMode::Agents);
        sb.toggle_mode();
        assert_eq!(sb.mode, SidebarMode::Groups);
        sb.toggle_mode();
        assert_eq!(sb.mode, SidebarMode::Agents);
    }

    #[test]
    fn toggle_mode_resets_selection() {
        let mut sb = StatusBar::new();
        sb.selected = 3;
        sb.toggle_mode();
        assert_eq!(sb.selected, 0);
    }
}
