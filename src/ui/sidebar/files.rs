//! The files mode: the vault tree.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

use crate::app::App;
use crate::ui::overlay::prompt::PromptLine;
use crate::vault::tree::Row;

/// The marker on the row holding the active document.
const ACTIVE_MARKER: &str = "●";
/// The markers on a closed and an open directory.
const CLOSED: &str = "▸";
const OPEN: &str = "▾";

/// The vault tree, drawn inside the sidebar's content area.
pub struct FilesMode<'a> {
    app: &'a App,
}

impl<'a> FilesMode<'a> {
    /// Draw the tree for the current state.
    pub fn new(app: &'a App) -> Self {
        FilesMode { app }
    }

    /// The line drawn when there are no rows to draw.
    ///
    /// Nothing is said while the walk is still running: the mode header
    /// already reports progress, and a second line saying the same thing only
    /// to be replaced a moment later is noise.
    fn placeholder(&self) -> Vec<Line<'static>> {
        let theme = &self.app.theme;
        let tree = &self.app.vault.tree;

        if !tree.complete {
            return Vec::new();
        }

        let text = if tree.filter().is_some() {
            "no matching entries"
        } else if tree.is_empty() {
            "the vault is empty"
        } else {
            "nothing to show; press `.` or `a`"
        };

        vec![Line::styled(text, theme.sidebar.file_other)]
    }

    /// Build the visible lines, scrolled so the selection stays on screen.
    fn lines(&self, height: usize) -> Vec<Line<'static>> {
        let tree = &self.app.vault.tree;
        let theme = &self.app.theme;
        let rows = tree.rows();

        if rows.is_empty() {
            return self.placeholder();
        }

        let active = self.app.active_path();
        let selected = tree.selected_row(&rows);
        let first = scroll_offset(selected, rows.len(), height);

        rows.iter()
            .enumerate()
            .skip(first)
            .take(height)
            .map(|(at, row)| self.row(row, Some(at) == selected, active.as_deref(), theme))
            .collect()
    }

    /// One tree row.
    fn row(
        &self,
        row: &Row,
        selected: bool,
        active: Option<&std::path::Path>,
        theme: &crate::theme::Theme,
    ) -> Line<'static> {
        let tree = &self.app.vault.tree;
        let node = tree.node(row.node);

        let is_active = active == Some(node.path.as_path());
        let style = if node.is_dir {
            theme.sidebar.directory
        } else if is_active {
            theme.sidebar.file_active
        } else if tree.is_markdown(node) {
            theme.sidebar.file
        } else {
            // A file perga does not render is dimmed: it is still reachable,
            // but it opens somewhere else.
            theme.sidebar.file_other
        };

        let marker = if node.is_dir {
            if node.expanded || tree.filter().is_some() {
                OPEN
            } else {
                CLOSED
            }
        } else if is_active {
            ACTIVE_MARKER
        } else {
            " "
        };

        let mut spans = vec![
            Span::styled("  ".repeat(row.depth), style),
            Span::styled(format!("{marker} "), style),
        ];
        spans.extend(highlight(
            &node.name,
            tree.filter(),
            style,
            theme.sidebar.r#match,
        ));

        let mut line = Line::from(spans);
        if selected {
            // The selection is a background, so the per-entry foreground
            // colours survive it and a directory still reads as a directory.
            line = line.style(theme.ui.selection);
        }
        line
    }
}

impl Widget for FilesMode<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        if area.height == 0 {
            return;
        }

        let theme = &self.app.theme;
        let mut rows = area;

        // The filter line sits at the top of the mode, above the tree it is
        // filtering, so what is typed and what it matches are visible at once.
        if let Some(input) = &self.app.sidebar.filter {
            let line = Rect { height: 1, ..area };
            PromptLine::new(input, "/", theme.sidebar.file, theme.ui.selection).render(line, buf);

            rows = Rect {
                y: area.y + 1,
                height: area.height - 1,
                ..area
            };
        }

        Paragraph::new(self.lines(usize::from(rows.height))).render(rows, buf);
    }
}

/// Highlight the part of a name that the filter matched.
fn highlight(name: &str, filter: Option<&str>, style: Style, matched: Style) -> Vec<Span<'static>> {
    let Some(filter) = filter.filter(|f| !f.is_empty()) else {
        return vec![Span::styled(name.to_string(), style)];
    };

    let Some(at) = name.to_lowercase().find(&filter.to_lowercase()) else {
        return vec![Span::styled(name.to_string(), style)];
    };

    // The match is found in the lower-cased name but sliced out of the
    // original, so the row shows the name as it is on disk.
    let end = at + filter.len();
    vec![
        Span::styled(name[..at].to_string(), style),
        Span::styled(name[at..end].to_string(), matched),
        Span::styled(name[end..].to_string(), style),
    ]
}

/// Which row the visible window starts at.
///
/// The selection is kept on screen with as little movement as possible: the
/// list only scrolls when the selection would otherwise leave it.
fn scroll_offset(selected: Option<usize>, total: usize, height: usize) -> usize {
    let Some(selected) = selected else { return 0 };
    if height == 0 || total <= height {
        return 0;
    }

    // Centre the selection when it is far enough into the list for centring to
    // be possible, and pin it to either end otherwise.
    selected
        .saturating_sub(height / 2)
        .min(total.saturating_sub(height))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_list_is_never_scrolled() {
        assert_eq!(scroll_offset(Some(3), 5, 10), 0);
        assert_eq!(scroll_offset(None, 100, 10), 0);
    }

    #[test]
    fn the_window_follows_the_selection_and_stops_at_the_end() {
        assert_eq!(scroll_offset(Some(0), 100, 10), 0);
        assert_eq!(scroll_offset(Some(20), 100, 10), 15);
        assert_eq!(scroll_offset(Some(99), 100, 10), 90);
    }

    #[test]
    fn a_filter_match_is_sliced_out_of_the_original_name() {
        let spans = highlight("README.md", Some("readme"), Style::new(), Style::new());
        let text: Vec<&str> = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, ["", "README", ".md"]);
    }

    #[test]
    fn a_name_that_does_not_match_is_one_span() {
        let spans = highlight("notes.md", Some("zzz"), Style::new(), Style::new());
        assert_eq!(spans.len(), 1);
    }
}
