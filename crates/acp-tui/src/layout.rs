use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub struct AppLayout {
    pub sidebar: Rect,
    pub messages: Rect,
    pub input: Rect,
}

impl AppLayout {
    pub fn new(area: Rect) -> Self {
        // Sidebar width: wider for tool call tree display
        let sidebar_width = if area.width > 100 {
            24
        } else if area.width > 60 {
            20
        } else {
            16
        };

        let h_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Length(sidebar_width), // sidebar
                Constraint::Min(30),               // chat area
            ])
            .split(area);

        // Chat area: messages + input
        let v_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Min(5),    // messages
                Constraint::Length(3), // input (with border)
            ])
            .split(h_chunks[1]);

        Self {
            sidebar: h_chunks[0],
            messages: v_chunks[0],
            input: v_chunks[1],
        }
    }
}
