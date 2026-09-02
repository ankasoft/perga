//! A single-line text input, and the widget that draws it.
//!
//! One implementation serves every place perga asks for a line of text: the
//! tree filter, the rename prompt, the new-file prompt, and the project search
//! query. Editing goes through [`TextEdit`] so that, like everything else, a
//! keystroke becomes an [`crate::action::Action`] rather than a direct mutation
//! from the input layer.

use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};

/// One edit to a [`TextInput`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEdit {
    /// Type a character at the cursor.
    Insert(char),
    /// Delete the character before the cursor.
    Backspace,
    /// Delete the character under the cursor.
    Delete,
    /// Delete the word before the cursor. `Ctrl+W`.
    DeleteWordBack,
    /// Clear the whole line. `Ctrl+U`.
    Clear,
    /// Move the cursor one character left.
    Left,
    /// Move the cursor one character right.
    Right,
    /// Move the cursor to the start of the line.
    Home,
    /// Move the cursor to the end of the line.
    End,
}

/// A line of text being edited.
///
/// The cursor is a byte offset into `value` and is always on a character
/// boundary, so a prompt holding a path with non-ASCII characters in it cannot
/// be made to panic by pressing Backspace.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextInput {
    value: String,
    cursor: usize,
}

impl TextInput {
    /// An empty input.
    pub fn new() -> Self {
        TextInput::default()
    }

    /// An input pre-filled with a value, with the cursor at the end.
    ///
    /// This is what a rename prompt opens as: the current name, ready to be
    /// edited rather than retyped.
    pub fn with_value(value: impl Into<String>) -> Self {
        let value = value.into();
        let cursor = value.len();
        TextInput { value, cursor }
    }

    /// The text as it stands.
    pub fn value(&self) -> &str {
        &self.value
    }

    /// The cursor's byte offset.
    pub fn cursor(&self) -> usize {
        self.cursor
    }

    /// How many columns of text precede the cursor, for placing the caret.
    pub fn cursor_column(&self) -> u16 {
        use unicode_width::UnicodeWidthStr;
        self.value[..self.cursor].width() as u16
    }

    /// Whether anything has been typed.
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }

    /// Apply one edit.
    pub fn apply(&mut self, edit: TextEdit) {
        match edit {
            TextEdit::Insert(c) => {
                self.value.insert(self.cursor, c);
                self.cursor += c.len_utf8();
            }
            TextEdit::Backspace => {
                if let Some(prev) = self.prev_boundary() {
                    self.value.replace_range(prev..self.cursor, "");
                    self.cursor = prev;
                }
            }
            TextEdit::Delete => {
                if let Some(next) = self.next_boundary() {
                    self.value.replace_range(self.cursor..next, "");
                }
            }
            TextEdit::DeleteWordBack => {
                let start = self.word_start();
                self.value.replace_range(start..self.cursor, "");
                self.cursor = start;
            }
            TextEdit::Clear => {
                self.value.clear();
                self.cursor = 0;
            }
            TextEdit::Left => {
                if let Some(prev) = self.prev_boundary() {
                    self.cursor = prev;
                }
            }
            TextEdit::Right => {
                if let Some(next) = self.next_boundary() {
                    self.cursor = next;
                }
            }
            TextEdit::Home => self.cursor = 0,
            TextEdit::End => self.cursor = self.value.len(),
        }
    }

    /// The character boundary before the cursor.
    fn prev_boundary(&self) -> Option<usize> {
        self.value[..self.cursor]
            .char_indices()
            .next_back()
            .map(|(at, _)| at)
    }

    /// The character boundary after the cursor.
    fn next_boundary(&self) -> Option<usize> {
        self.value[self.cursor..]
            .chars()
            .next()
            .map(|c| self.cursor + c.len_utf8())
    }

    /// Where the word before the cursor starts.
    ///
    /// Trailing whitespace goes with the word, so `Ctrl+W` at the end of
    /// `docs/api ` deletes `api ` rather than nothing.
    fn word_start(&self) -> usize {
        let before = &self.value[..self.cursor];
        let trimmed = before.trim_end();
        match trimmed.rfind(|c: char| c.is_whitespace() || c == '/') {
            Some(at) => at + 1,
            None => 0,
        }
    }
}

/// A prompt line: a prefix, the text, and a block cursor.
pub struct PromptLine<'a> {
    input: &'a TextInput,
    prefix: &'a str,
    style: Style,
    cursor_style: Style,
}

impl<'a> PromptLine<'a> {
    /// Draw `input` behind `prefix`.
    pub fn new(input: &'a TextInput, prefix: &'a str, style: Style, cursor_style: Style) -> Self {
        PromptLine {
            input,
            prefix,
            style,
            cursor_style,
        }
    }
}

impl Widget for PromptLine<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let value = self.input.value();
        let at = self.input.cursor();

        // The caret is drawn rather than placed with the terminal's own cursor:
        // the prompt can appear inside the sidebar, and one owner of the real
        // cursor (edit mode) is enough.
        let under = value[at..].chars().next();
        let (head, tail) = value.split_at(at);
        let tail = under.map_or(tail, |c| &tail[c.len_utf8()..]);

        let mut spans = vec![
            Span::styled(self.prefix.to_string(), self.style),
            Span::styled(head.to_string(), self.style),
            Span::styled(under.unwrap_or(' ').to_string(), self.cursor_style),
        ];
        if !tail.is_empty() {
            spans.push(Span::styled(tail.to_string(), self.style));
        }

        Paragraph::new(Line::from(spans))
            .style(self.style)
            .render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(value: &str) -> TextInput {
        TextInput::with_value(value)
    }

    #[test]
    fn typing_inserts_at_the_cursor() {
        let mut text = TextInput::new();
        for c in "docs".chars() {
            text.apply(TextEdit::Insert(c));
        }
        assert_eq!(text.value(), "docs");

        text.apply(TextEdit::Home);
        text.apply(TextEdit::Insert('/'));
        assert_eq!(text.value(), "/docs");
        assert_eq!(text.cursor(), 1);
    }

    #[test]
    fn backspace_at_the_start_does_nothing() {
        let mut text = input("a");
        text.apply(TextEdit::Home);
        text.apply(TextEdit::Backspace);
        assert_eq!(text.value(), "a");
    }

    #[test]
    fn editing_a_multibyte_line_stays_on_character_boundaries() {
        let mut text = input("ışık");

        text.apply(TextEdit::Backspace);
        assert_eq!(text.value(), "ışı");

        text.apply(TextEdit::Left);
        text.apply(TextEdit::Delete);
        assert_eq!(text.value(), "ış");

        text.apply(TextEdit::Home);
        text.apply(TextEdit::Right);
        assert_eq!(text.cursor(), "ı".len());
    }

    #[test]
    fn ctrl_w_deletes_a_word_and_its_trailing_space() {
        let mut text = input("open docs ");
        text.apply(TextEdit::DeleteWordBack);
        assert_eq!(text.value(), "open ");

        text.apply(TextEdit::DeleteWordBack);
        assert_eq!(text.value(), "");
    }

    #[test]
    fn ctrl_w_stops_at_a_path_separator() {
        let mut text = input("docs/api/auth");
        text.apply(TextEdit::DeleteWordBack);
        assert_eq!(text.value(), "docs/api/");
    }

    #[test]
    fn ctrl_u_clears_the_line() {
        let mut text = input("a long query");
        text.apply(TextEdit::Clear);
        assert!(text.is_empty());
        assert_eq!(text.cursor(), 0);
    }

    #[test]
    fn the_cursor_column_counts_display_width() {
        let text = input("日本");
        assert_eq!(text.cursor_column(), 4);
    }
}
