//! The document viewport: windowed block rendering and the scrollbar.
//!
//! A tab with no document open shows the welcome screen instead.

use ratatui::layout::Rect;
use ratatui::widgets::{Block, Widget};

use crate::app::{App, Focus};
use crate::ui::welcome::Welcome;

/// The document viewport.
pub struct Viewport<'a> {
    app: &'a App,
}

impl<'a> Viewport<'a> {
    /// Build the viewport for the current state.
    pub fn new(app: &'a App) -> Self {
        Viewport { app }
    }
}

impl Widget for Viewport<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let theme = &self.app.theme;
        let focused = self.app.focus == Focus::Viewport;

        let block = Block::bordered().border_style(if focused {
            theme.ui.border_focused
        } else {
            theme.ui.border
        });
        let inner = block.inner(area);
        block.render(area, buf);

        Welcome::new(theme, &self.app.keymap).render(inner, buf);
    }
}
