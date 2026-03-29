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

// System message sub-type styles
pub const SYSTEM_ONLINE: Style = Style::new().fg(Color::Green).add_modifier(Modifier::ITALIC);
pub const SYSTEM_OFFLINE: Style = Style::new()
    .fg(Color::DarkGray)
    .add_modifier(Modifier::ITALIC);
pub const SYSTEM_COMPLETE: Style = Style::new().fg(Color::Rgb(80, 180, 80));
pub const SYSTEM_ERROR: Style = Style::new().fg(Color::Red);
pub const SYSTEM_QUEUE: Style = Style::new()
    .fg(Color::Yellow)
    .add_modifier(Modifier::ITALIC);
pub const SYSTEM_ROUTE: Style = Style::new()
    .fg(Color::Rgb(100, 100, 140))
    .add_modifier(Modifier::DIM);

// Input box styles
pub const INPUT_PLACEHOLDER: Style = Style::new()
    .fg(Color::Rgb(70, 70, 80))
    .add_modifier(Modifier::ITALIC);

// Hint bar (bottom keybinding hints)
pub const HINT_BAR_BG: Style = Style::new().fg(Color::Rgb(50, 55, 65));
pub const HINT_KEY: Style = Style::new().fg(Color::Rgb(80, 180, 200));
pub const HINT_SEP: Style = Style::new().fg(Color::Rgb(35, 40, 50));

// Context token display
pub const CONTEXT_NORMAL: Style = Style::new().fg(Color::Rgb(70, 120, 160));
pub const CONTEXT_HIGH: Style = Style::new().fg(Color::Yellow);
pub const CONTEXT_CRITICAL: Style = Style::new().fg(Color::Red);

// Sidebar project path
pub const SIDEBAR_PATH: Style = Style::new().fg(Color::Rgb(70, 80, 100));

pub fn status_icon(status: &str) -> &'static str {
    match status {
        "idle" => "◉",
        "streaming" => "●",
        "connecting" => "◌",
        "disconnected" | "error" => "○",
        _ => "?",
    }
}
