use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub struct AppLayout {
    pub messages: Rect,
    pub status_bar: Rect,
    pub input: Rect,
}

impl AppLayout {
    pub fn new(area: Rect) -> Self {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1), // status bar
                Constraint::Min(5),    // messages
                Constraint::Length(1), // input
            ])
            .split(area);

        Self {
            status_bar: chunks[0],
            messages: chunks[1],
            input: chunks[2],
        }
    }
}
