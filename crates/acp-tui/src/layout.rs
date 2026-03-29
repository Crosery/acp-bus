use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub struct AppLayout {
    pub sidebar: Option<Rect>,
    pub messages: Rect,
    pub input: Rect,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(w: u16, h: u16) -> Rect {
        Rect::new(0, 0, w, h)
    }

    #[test]
    fn collapsed_sidebar_is_none() {
        let layout = AppLayout::new(area(120, 40), 1, true);
        assert!(layout.sidebar.is_none());
    }

    #[test]
    fn collapsed_messages_use_full_width() {
        let layout = AppLayout::new(area(120, 40), 1, true);
        assert_eq!(layout.messages.width, 120);
    }

    #[test]
    fn expanded_sidebar_is_some() {
        let layout = AppLayout::new(area(120, 40), 1, false);
        assert!(layout.sidebar.is_some());
        assert!(layout.sidebar.unwrap().width > 0);
    }

    #[test]
    fn expanded_messages_narrower_than_full() {
        let layout = AppLayout::new(area(120, 40), 1, false);
        assert!(layout.messages.width < 120);
    }

    #[test]
    fn input_area_always_present() {
        let collapsed = AppLayout::new(area(120, 40), 1, true);
        let expanded = AppLayout::new(area(120, 40), 1, false);
        assert!(collapsed.input.height >= 2);
        assert!(expanded.input.height >= 2);
    }
}

impl AppLayout {
    pub fn new(area: Rect, input_lines: u16, sidebar_collapsed: bool) -> Self {
        if sidebar_collapsed {
            let max_input = (area.height / 3).max(2);
            let input_height = (input_lines + 1).clamp(2, max_input);
            let v_chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Min(5), Constraint::Length(input_height)])
                .split(area);
            return Self {
                sidebar: None,
                messages: v_chunks[0],
                input: v_chunks[1],
            };
        }

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
            .constraints([Constraint::Length(sidebar_width), Constraint::Min(30)])
            .split(area);

        let max_input = (h_chunks[1].height / 3).max(2);
        let input_height = (input_lines + 1).clamp(2, max_input);

        let v_chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(5), Constraint::Length(input_height)])
            .split(h_chunks[1]);

        Self {
            sidebar: Some(h_chunks[0]),
            messages: v_chunks[0],
            input: v_chunks[1],
        }
    }
}
