//! The outline mode: the active document's headings.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::app::App;
use crate::ui::sidebar::files::scroll_offset;

/// The active document's headings, indented by level.
pub struct OutlineMode<'a> {
    app: &'a App,
}

impl<'a> OutlineMode<'a> {
    /// Draw the outline for the current state.
    pub fn new(app: &'a App) -> Self {
        OutlineMode { app }
    }

    /// The visible lines, scrolled so the selection stays on screen.
    fn lines(&self, height: usize) -> Vec<Line<'static>> {
        let theme = &self.app.theme;

        let Some(doc) = self.app.tab().doc.as_ref() else {
            return vec![Line::styled("no document", theme.sidebar.file_other)];
        };

        if doc.outline.is_empty() {
            return vec![Line::styled("no headings", theme.sidebar.file_other)];
        }

        let selected = self.app.sidebar.outline_selected.min(doc.outline.len() - 1);
        // The heading the reader is actually inside, which moves as they
        // scroll and is not the same thing as the one they have selected.
        let current = self.app.current_heading();
        let first = scroll_offset(Some(selected), doc.outline.len(), height);

        doc.outline
            .iter()
            .enumerate()
            .skip(first)
            .take(height)
            .map(|(at, heading)| {
                // A level-1 heading sits flush; each level below is one step
                // in, so the shape of the document is visible at a glance.
                let indent = "  ".repeat(usize::from(heading.level.saturating_sub(1)));

                let style = match (at == current, heading.level) {
                    (true, _) => theme.sidebar.file_active,
                    (_, 1 | 2) => theme.sidebar.directory,
                    _ => theme.sidebar.file,
                };

                let line = Line::from(vec![
                    Span::styled(indent, style),
                    Span::styled(heading.text.clone(), style),
                ]);

                if at == selected {
                    line.style(theme.ui.selection)
                } else {
                    line
                }
            })
            .collect()
    }
}

impl Widget for OutlineMode<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        if area.height == 0 {
            return;
        }
        Paragraph::new(self.lines(usize::from(area.height))).render(area, buf);
    }
}
