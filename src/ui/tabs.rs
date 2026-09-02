//! The tab bar.
//!
//! Hidden entirely when only one tab is open and `ui.always_show_tabs` is off,
//! which is the default: a row of vertical space is the scarcest resource in a
//! terminal.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::app::App;

/// The tab bar.
pub struct TabBar<'a> {
    app: &'a App,
}

impl<'a> TabBar<'a> {
    /// Build the tab bar for the current state.
    pub fn new(app: &'a App) -> Self {
        TabBar { app }
    }

    /// The line the tab bar draws.
    pub fn line(&self) -> Line<'static> {
        let theme = &self.app.theme;
        let mut spans = Vec::new();

        for (index, tab) in self.app.tabs.iter().enumerate() {
            let style = if index == self.app.active_tab {
                theme.tabs.active
            } else {
                theme.tabs.inactive
            };
            spans.push(Span::styled(format!(" {} ", tab.label()), style));
            spans.push(Span::styled("│", theme.ui.border));
        }

        spans.push(Span::styled(" + ", theme.tabs.inactive));
        Line::from(spans)
    }
}

impl Widget for TabBar<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        Paragraph::new(self.line()).render(area, buf);
    }
}
