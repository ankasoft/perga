//! The find-in-document bar.
//!
//! Drawn as one line along the bottom of the viewport rather than as a centred
//! panel: it is the matches behind it that the reader is looking at, and a
//! dialog in the middle of the document would cover them.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use unicode_width::UnicodeWidthStr;

use crate::search::in_doc::FindState;
use crate::theme::Theme;

/// The find bar.
pub struct FindBar<'a> {
    find: &'a FindState,
    theme: &'a Theme,
}

impl<'a> FindBar<'a> {
    /// Draw a find state.
    pub fn new(find: &'a FindState, theme: &'a Theme) -> Self {
        FindBar { find, theme }
    }
}

impl Widget for FindBar<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        if area.height == 0 || area.width == 0 {
            return;
        }

        let style = self.theme.ui.status_bar;
        let line = Rect {
            y: area.bottom().saturating_sub(1),
            height: 1,
            ..area
        };

        let position = self.find.position();
        let query = self.find.input.value();

        // The caret is drawn rather than placed with the terminal's own
        // cursor, which edit mode owns.
        let at = self.find.input.cursor();
        let under = query[at..].chars().next();
        let (head, tail) = query.split_at(at);
        let tail = under.map_or(tail, |c| &tail[c.len_utf8()..]);

        let left = Line::from(vec![
            Span::styled("/", self.theme.ui.status_mode),
            Span::styled(head.to_string(), style),
            Span::styled(under.unwrap_or(' ').to_string(), self.theme.ui.selection),
            Span::styled(tail.to_string(), style),
        ]);

        let left_width = left.width() as u16;
        Paragraph::new(left).style(style).render(line, buf);

        // The count is right-aligned, and dropped rather than overlapped when
        // the query has grown long enough to reach it.
        let needed = position.width() as u16 + 1;
        if !position.is_empty() && line.width > needed + left_width {
            let at = line.right() - needed;
            buf.set_line(
                at,
                line.y,
                &Line::from(Span::styled(
                    position,
                    if self.find.count() == 0 {
                        self.theme.ui.status_warning
                    } else {
                        style
                    },
                )),
                needed,
            );
        }
    }
}
