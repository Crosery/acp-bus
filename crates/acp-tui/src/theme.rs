use ratatui::style::{Color, Modifier, Style};

pub const BORDER: Style = Style::new().fg(Color::DarkGray);
pub const TITLE: Style = Style::new().fg(Color::White).add_modifier(Modifier::BOLD);
pub const SYSTEM_MSG: Style = Style::new()
    .fg(Color::DarkGray)
    .add_modifier(Modifier::ITALIC);
pub const USER_MSG: Style = Style::new().fg(Color::Cyan);
pub const AGENT_MSG: Style = Style::new().fg(Color::Green);
pub const MENTION: Style = Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD);
pub const TIMESTAMP: Style = Style::new().fg(Color::DarkGray);

pub const STATUS_IDLE: Style = Style::new().fg(Color::Green);
pub const STATUS_STREAMING: Style = Style::new().fg(Color::Yellow);
pub const STATUS_CONNECTING: Style = Style::new().fg(Color::Blue);
pub const STATUS_DISCONNECTED: Style = Style::new().fg(Color::Red);
pub const STATUS_BUSY: Style = Style::new()
    .fg(Color::Black)
    .bg(Color::Yellow)
    .add_modifier(Modifier::BOLD);
pub const STATUS_THINKING: Style = Style::new()
    .fg(Color::Black)
    .bg(Color::LightBlue)
    .add_modifier(Modifier::BOLD);
pub const STATUS_TOOL: Style = Style::new()
    .fg(Color::Black)
    .bg(Color::LightGreen)
    .add_modifier(Modifier::BOLD);
pub const STATUS_ERROR_BADGE: Style = Style::new()
    .fg(Color::White)
    .bg(Color::Red)
    .add_modifier(Modifier::BOLD);

pub const THINKING_PREFIX: Style = Style::new()
    .fg(Color::Rgb(120, 100, 160))
    .add_modifier(Modifier::DIM);
pub const THINKING_TEXT: Style = Style::new()
    .fg(Color::Rgb(140, 130, 170))
    .add_modifier(Modifier::ITALIC);

pub fn status_icon(status: &str) -> &'static str {
    match status {
        "idle" => "◉",
        "streaming" => "●",
        "connecting" => "◌",
        "disconnected" | "error" => "○",
        _ => "?",
    }
}
