//! Print mode: render a document to stdout with ANSI styling and exit.
//!
//! This serves `perga README.md | less -R`, quick previews, and scripting, and
//! it is what users of `glow` and `mdcat` expect a Markdown tool to do. It runs
//! the same block pipeline as the TUI, so what is printed is what would have
//! been shown.
//!
//! No alternate screen, no cursor control, no mouse, no input.

use std::io::{self, Write};

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;

use crate::doc::document::Document;
use crate::doc::highlight::Highlighter;
use crate::doc::render::{RenderedDocument, Renderer};
use crate::theme::Theme;

/// The width used when there is no terminal to ask.
pub const DEFAULT_WIDTH: u16 = 80;

/// Render a document and write it to `out`.
///
/// `colour` is false under `NO_COLOR`, in which case the text is written with
/// no escape sequences at all.
pub fn print(
    document: &Document,
    theme: &Theme,
    width: u16,
    colour: bool,
    out: &mut impl Write,
) -> io::Result<()> {
    // No frame budget to protect here, so the syntax sets are loaded up front
    // rather than in the background: printing plain code and exiting before the
    // loader finished would be worse than waiting for it.
    let highlighter = Highlighter::new();
    highlighter.load_blocking();

    let renderer = Renderer::new(theme, highlighter, width);
    let mut layout = RenderedDocument::new();

    while !layout.resolve_all(document, &renderer) {}
    let total = layout
        .total_lines(document)
        .expect("the document is fully measured");

    let lines = layout.window(
        document,
        &renderer,
        0,
        u16::try_from(total).unwrap_or(u16::MAX),
    );

    for line in &lines {
        write_line(line, colour, out)?;
    }

    out.flush()
}

/// Write one line, with or without styling.
fn write_line(line: &Line<'static>, colour: bool, out: &mut impl Write) -> io::Result<()> {
    for span in &line.spans {
        if span.content.is_empty() {
            continue;
        }
        let style = line.style.patch(span.style);

        if colour {
            write!(out, "{}", sgr(style))?;
            write!(out, "{}", span.content)?;
            if style != Style::default() {
                write!(out, "\x1b[0m")?;
            }
        } else {
            write!(out, "{}", span.content)?;
        }
    }

    writeln!(out)
}

/// The SGR escape sequence for a style.
///
/// Written by hand rather than through a terminal backend: print mode must not
/// touch the terminal, and `crossterm`'s writers assume they own it.
fn sgr(style: Style) -> String {
    let mut codes: Vec<String> = Vec::new();

    for (modifier, code) in [
        (Modifier::BOLD, "1"),
        (Modifier::DIM, "2"),
        (Modifier::ITALIC, "3"),
        (Modifier::UNDERLINED, "4"),
        (Modifier::REVERSED, "7"),
        (Modifier::CROSSED_OUT, "9"),
    ] {
        if style.add_modifier.contains(modifier) {
            codes.push(code.to_string());
        }
    }

    if let Some(fg) = style.fg {
        codes.extend(colour_codes(fg, true));
    }
    if let Some(bg) = style.bg {
        codes.extend(colour_codes(bg, false));
    }

    if codes.is_empty() {
        String::new()
    } else {
        format!("\x1b[{}m", codes.join(";"))
    }
}

/// The SGR parameters for one colour.
fn colour_codes(colour: Color, foreground: bool) -> Vec<String> {
    let base = if foreground { 30 } else { 40 };
    let bright = if foreground { 90 } else { 100 };

    let ansi = |offset: u8| vec![(base + u16::from(offset)).to_string()];
    let ansi_bright = |offset: u8| vec![(bright + u16::from(offset)).to_string()];

    match colour {
        Color::Reset => vec![if foreground { "39" } else { "49" }.to_string()],
        Color::Black => ansi(0),
        Color::Red => ansi(1),
        Color::Green => ansi(2),
        Color::Yellow => ansi(3),
        Color::Blue => ansi(4),
        Color::Magenta => ansi(5),
        Color::Cyan => ansi(6),
        Color::Gray => ansi(7),
        Color::DarkGray => ansi_bright(0),
        Color::LightRed => ansi_bright(1),
        Color::LightGreen => ansi_bright(2),
        Color::LightYellow => ansi_bright(3),
        Color::LightBlue => ansi_bright(4),
        Color::LightMagenta => ansi_bright(5),
        Color::LightCyan => ansi_bright(6),
        Color::White => ansi_bright(7),
        Color::Rgb(r, g, b) => vec![
            if foreground { "38" } else { "48" }.to_string(),
            "2".to_string(),
            r.to_string(),
            g.to_string(),
            b.to_string(),
        ],
        Color::Indexed(i) => vec![
            if foreground { "38" } else { "48" }.to_string(),
            "5".to_string(),
            i.to_string(),
        ],
    }
}

/// A `Buffer` is never used in print mode; this keeps the unused import honest
/// if it is ever reintroduced.
#[allow(dead_code)]
fn _assert_no_buffer(_: Buffer, _: Rect) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn printed(source: &str, colour: bool) -> String {
        let document = Document::scratch(source);
        let mut out = Vec::new();
        print(&document, &Theme::dark(), 40, colour, &mut out).unwrap();
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn plain_output_has_no_escape_sequences() {
        let text = printed("# Title\n\nSome **bold** prose.\n", false);
        assert!(!text.contains('\x1b'), "{text:?}");
        assert!(text.contains("Title"));
        assert!(text.contains("bold"));
    }

    #[test]
    fn styled_output_has_no_tui_control_sequences() {
        let text = printed("# Title\n\nProse.\n", true);

        assert!(text.contains('\x1b'), "expected styling");
        // The sequences a TUI writes and a pipe must never see.
        for forbidden in [
            "\x1b[?1049h", // alternate screen
            "\x1b[?1049l",
            "\x1b[?25l",   // cursor hide
            "\x1b[?1000h", // mouse
            "\x1b[2J",     // clear screen
            "\x1b[H",      // cursor home
        ] {
            assert!(!text.contains(forbidden), "{forbidden:?} in {text:?}");
        }
    }

    #[test]
    fn every_styled_run_is_reset_afterwards() {
        let text = printed("**bold** and *italic*\n", true);
        let opens = text.matches("\x1b[").count() - text.matches("\x1b[0m").count();
        assert_eq!(opens, text.matches("\x1b[0m").count(), "{text:?}");
    }

    #[test]
    fn output_wraps_at_the_requested_width() {
        let text = printed(
            "one two three four five six seven eight nine ten eleven twelve\n",
            false,
        );
        for line in text.lines() {
            assert!(line.chars().count() <= 40, "{line:?}");
        }
    }

    #[test]
    fn code_blocks_are_highlighted() {
        let text = printed("```rust\nfn main() {}\n```\n", true);
        assert!(text.contains('\x1b'), "{text:?}");

        // The tokens are in separate styled spans, which is the point, so the
        // text has to be reassembled before it can be looked for.
        let plain = strip_escapes(&text);
        assert!(plain.contains("fn main() {}"), "{plain:?}");
    }

    /// Drop every SGR sequence, leaving the text that was printed.
    fn strip_escapes(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars();

        while let Some(c) = chars.next() {
            if c != '\x1b' {
                out.push(c);
                continue;
            }
            for c in chars.by_ref() {
                if c == 'm' {
                    break;
                }
            }
        }

        out
    }

    #[test]
    fn an_empty_document_prints_nothing() {
        assert_eq!(printed("", false), "");
    }
}
