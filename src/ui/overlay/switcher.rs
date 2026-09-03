//! Pick-one-of-many overlays: the wiki-link disambiguation list, and in
//! Section 9.7 the fuzzy quick switcher.
//!
//! Both are the same shape (a title, a list, one selected row) so they share
//! a widget rather than growing two that drift apart.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Widget};

use crate::theme::Theme;

/// A list to choose one row from.
pub struct Picker<'a> {
    theme: &'a Theme,
    title: String,
    hint: &'a str,
    rows: Vec<Line<'static>>,
    selected: usize,
}

impl<'a> Picker<'a> {
    /// Build a picker over already-styled rows.
    pub fn new(
        theme: &'a Theme,
        title: impl Into<String>,
        hint: &'a str,
        rows: Vec<Line<'static>>,
        selected: usize,
    ) -> Self {
        Picker {
            theme,
            title: title.into(),
            hint,
            rows,
            selected,
        }
    }
}

impl Widget for Picker<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        // Whatever is underneath has to go first or it shows through the gaps.
        Clear.render(area, buf);

        let block = Block::bordered()
            .border_style(self.theme.ui.border_focused)
            .style(self.theme.ui.background)
            .title(Span::styled(
                format!(" {} ", self.title),
                self.theme.ui.title,
            ));
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height == 0 {
            return;
        }

        // The hint sits on the last row, so the list never runs into it.
        let list_height = inner.height.saturating_sub(1).max(1);
        let first = self
            .selected
            .saturating_sub(usize::from(list_height) / 2)
            .min(self.rows.len().saturating_sub(usize::from(list_height)));

        let rows: Vec<Line<'static>> = self
            .rows
            .into_iter()
            .enumerate()
            .skip(first)
            .take(usize::from(list_height))
            .map(|(at, row)| {
                if at == self.selected {
                    row.style(self.theme.ui.selection)
                } else {
                    row
                }
            })
            .collect();

        Paragraph::new(rows).render(
            Rect {
                height: list_height,
                ..inner
            },
            buf,
        );

        if inner.height > 1 {
            Paragraph::new(Line::styled(
                self.hint.to_string(),
                self.theme.ui.logo_subtitle,
            ))
            .render(
                Rect {
                    y: inner.bottom() - 1,
                    height: 1,
                    ..inner
                },
                buf,
            );
        }
    }
}
