//! The search mode: the results of the last project-wide search.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::app::App;
use crate::search::content::Hit;
use crate::ui::sidebar::files::scroll_offset;

/// Search results, grouped by the file they were found in.
pub struct SearchMode<'a> {
    app: &'a App,
}

impl<'a> SearchMode<'a> {
    /// Draw the results for the current state.
    pub fn new(app: &'a App) -> Self {
        SearchMode { app }
    }

    /// The visible lines, scrolled so the selected hit stays on screen.
    fn lines(&self, height: usize) -> Vec<Line<'static>> {
        let theme = &self.app.theme;
        let search = &self.app.search;

        if let Some(error) = &search.error {
            return vec![Line::styled(error.clone(), theme.ui.status_error)];
        }

        if search.query.is_empty() {
            return vec![Line::styled(
                "press Ctrl+G to search the vault",
                theme.sidebar.file_other,
            )];
        }

        if search.hits.is_empty() {
            let text = if search.running {
                "searching…"
            } else {
                "no hits"
            };
            return vec![Line::styled(text, theme.sidebar.file_other)];
        }

        // Rows are the groups and their hits interleaved, so the scrolling
        // window is computed over what is actually drawn rather than over the
        // hits alone.
        let mut rows: Vec<(Option<usize>, Line<'static>)> = Vec::new();

        for (path, range) in search.groups() {
            rows.push((
                None,
                Line::styled(path.display().to_string(), theme.sidebar.directory),
            ));

            for at in range {
                rows.push((Some(at), self.hit_row(&search.hits[at])));
            }
        }

        let selected_row = rows.iter().position(|(at, _)| *at == Some(search.selected));
        let first = scroll_offset(selected_row, rows.len(), height);

        rows.into_iter()
            .enumerate()
            .skip(first)
            .take(height)
            .map(|(at, (_, line))| {
                if Some(at) == selected_row {
                    line.style(theme.ui.selection)
                } else {
                    line
                }
            })
            .collect()
    }

    /// One hit: its line number, and the line with the match picked out.
    fn hit_row(&self, hit: &Hit) -> Line<'static> {
        let theme = &self.app.theme;
        let (start, end) = hit.span;

        let mut spans = vec![Span::styled(
            format!("{:>5} ", hit.line),
            theme.sidebar.line_number,
        )];

        // The span is a byte range into the line, and a hit whose match ran
        // off the end of what `grep` reported is drawn plain rather than
        // sliced at a byte that is not a boundary.
        let usable = start < end
            && end <= hit.text.len()
            && hit.text.is_char_boundary(start)
            && hit.text.is_char_boundary(end);

        if !usable {
            spans.push(Span::styled(hit.text.clone(), theme.sidebar.file));
            return Line::from(spans);
        }

        spans.push(Span::styled(
            hit.text[..start].to_string(),
            theme.sidebar.file,
        ));
        spans.push(Span::styled(
            hit.text[start..end].to_string(),
            theme.sidebar.r#match,
        ));
        spans.push(Span::styled(
            hit.text[end..].to_string(),
            theme.sidebar.file,
        ));

        Line::from(spans)
    }
}

impl Widget for SearchMode<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        if area.height == 0 {
            return;
        }
        Paragraph::new(self.lines(usize::from(area.height))).render(area, buf);
    }
}
