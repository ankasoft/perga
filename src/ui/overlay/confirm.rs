//! The confirmation dialog.
//!
//! Used wherever perga is about to do something the reader cannot undo:
//! discarding edits, overwriting a file that changed underneath them, creating
//! a page from a broken wiki-link, and restoring text an earlier run left
//! behind.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Widget, Wrap};

use crate::theme::Theme;

/// A question with a fixed set of answers.
pub struct Confirm<'a> {
    theme: &'a Theme,
    question: &'a str,
    choices: &'a [(char, String)],
}

impl<'a> Confirm<'a> {
    /// Ask `question`, offering `choices` as `(key, label)` pairs.
    pub fn new(theme: &'a Theme, question: &'a str, choices: &'a [(char, String)]) -> Self {
        Confirm {
            theme,
            question,
            choices,
        }
    }

    /// The row of answers.
    fn answers(&self) -> Line<'static> {
        let mut spans = Vec::new();

        for (key, label) in self.choices {
            spans.push(Span::styled(format!(" {key} "), self.theme.ui.status_mode));
            spans.push(Span::styled(
                format!(" {label}   "),
                self.theme.ui.status_bar,
            ));
        }

        // `Esc` always means "do nothing", and saying so is cheaper than
        // making the reader guess, unless the dialog already offered a
        // cancel of its own, in which case repeating it is noise.
        if !self.choices.iter().any(|(_, label)| label == "cancel") {
            spans.push(Span::styled(" Esc ", self.theme.ui.status_mode));
            spans.push(Span::styled(" cancel", self.theme.ui.status_bar));
        }

        Line::from(spans)
    }
}

impl Widget for Confirm<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        Clear.render(area, buf);

        let block = Block::bordered()
            .border_style(self.theme.ui.status_warning)
            .style(self.theme.ui.background);
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.height == 0 {
            return;
        }

        // The question wraps; the answers sit on the last row so they are
        // always in the same place however long the question is.
        let question_height = inner.height.saturating_sub(2).max(1);

        Paragraph::new(Line::styled(self.question.to_string(), self.theme.ui.title))
            .wrap(Wrap { trim: true })
            .render(
                Rect {
                    height: question_height,
                    ..inner
                },
                buf,
            );

        if inner.height > 1 {
            Paragraph::new(self.answers()).render(
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

/// Where a confirmation is drawn: centred, and only as tall as it needs.
pub fn area(frame: Rect) -> Rect {
    let width = (frame.width * 3 / 4).max(20).min(frame.width);
    let height = 6.min(frame.height);

    Rect {
        x: frame.x + (frame.width - width) / 2,
        y: frame.y + (frame.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}
