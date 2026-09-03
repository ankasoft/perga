//! Syntax highlighting for fenced code blocks, loaded lazily and off-thread.
//!
//! `SyntaxSet::load_defaults_newlines` decompresses an embedded dump and costs
//! on the order of 50 to 100 ms, and on its own that is the entire first-frame
//! budget from Section 14. Loading therefore happens on a background thread:
//! until it finishes, code blocks render unhighlighted against the theme's
//! `code_block_bg`, and they are re-rendered once the set arrives. A single
//! frame of plain code is acceptable; a blank screen is not.

use std::sync::{Arc, OnceLock};
use std::thread;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, ThemeSet};
use syntect::parsing::SyntaxSet;
use syntect::util::LinesWithEndings;

/// The syntect theme used when the configured one is not available.
pub const FALLBACK_CODE_THEME: &str = "base16-ocean.dark";

/// The loaded syntect assets.
struct Assets {
    syntaxes: SyntaxSet,
    themes: ThemeSet,
}

/// A handle to the syntax highlighter.
///
/// Cheap to clone and safe to consult before loading has finished, which is the
/// whole point: the UI never waits for it.
#[derive(Clone, Default)]
pub struct Highlighter {
    assets: Arc<OnceLock<Assets>>,
}

impl std::fmt::Debug for Highlighter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Highlighter")
            .field("ready", &self.is_ready())
            .finish()
    }
}

impl Highlighter {
    /// A highlighter with nothing loaded yet.
    pub fn new() -> Self {
        Highlighter::default()
    }

    /// Start loading the syntax and theme sets on a background thread.
    ///
    /// `on_ready` is called once loading finishes, so the application can turn
    /// it into an action and re-render the code blocks it drew plain.
    pub fn spawn_load(&self, on_ready: impl FnOnce() + Send + 'static) {
        let assets = Arc::clone(&self.assets);

        let spawned = thread::Builder::new()
            .name("perga-syntax".to_string())
            .spawn(move || {
                let loaded = Assets {
                    syntaxes: SyntaxSet::load_defaults_newlines(),
                    themes: ThemeSet::load_defaults(),
                };
                // A second loader would be a bug, but losing the race is
                // harmless: the winner's assets are equivalent.
                let _ = assets.set(loaded);
                on_ready();
            });

        if let Err(e) = spawned {
            tracing::warn!("cannot load syntax highlighting: {e}");
        }
    }

    /// Load synchronously. Only for tests and for `--print`, which has no frame
    /// budget to protect.
    pub fn load_blocking(&self) {
        let _ = self.assets.set(Assets {
            syntaxes: SyntaxSet::load_defaults_newlines(),
            themes: ThemeSet::load_defaults(),
        });
    }

    /// Whether the assets have finished loading.
    pub fn is_ready(&self) -> bool {
        self.assets.get().is_some()
    }

    /// Highlight a block of code.
    ///
    /// Returns `None` when the assets are still loading or the language is not
    /// one syntect knows, in which case the caller renders the code plain.
    pub fn highlight(
        &self,
        code: &str,
        language: Option<&str>,
        theme_name: &str,
    ) -> Option<Vec<Line<'static>>> {
        let assets = self.assets.get()?;
        let language = language?;

        let syntax = assets
            .syntaxes
            .find_syntax_by_token(language)
            .or_else(|| assets.syntaxes.find_syntax_by_extension(language))?;

        let theme = assets
            .themes
            .themes
            .get(theme_name)
            .or_else(|| assets.themes.themes.get(FALLBACK_CODE_THEME))?;

        let mut highlighter = HighlightLines::new(syntax, theme);
        let mut lines = Vec::new();

        for source_line in LinesWithEndings::from(code) {
            // A highlighting failure on one line must not lose the document;
            // fall back to plain text for the rest of the block.
            let Ok(ranges) = highlighter.highlight_line(source_line, &assets.syntaxes) else {
                return None;
            };

            let spans = ranges
                .into_iter()
                .map(|(style, text)| {
                    Span::styled(
                        text.trim_end_matches(['\n', '\r']).to_string(),
                        convert_style(style),
                    )
                })
                .collect::<Vec<_>>();

            lines.push(Line::from(spans));
        }

        Some(lines)
    }
}

/// Convert a syntect style to a `ratatui` one.
///
/// The background is deliberately dropped: the theme's `code_block_bg` owns the
/// block's background, so that a syntect theme with a different background does
/// not cut a differently coloured rectangle out of the document.
fn convert_style(style: syntect::highlighting::Style) -> Style {
    let fg = style.foreground;
    let mut converted = Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b));

    if style.font_style.contains(FontStyle::BOLD) {
        converted = converted.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        converted = converted.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        converted = converted.add_modifier(Modifier::UNDERLINED);
    }

    converted
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    /// One loaded highlighter shared by the tests: loading the default sets is
    /// the expensive thing this module exists to keep off the UI thread, and
    /// paying for it once per test would be slow for no extra coverage.
    fn loaded() -> &'static Highlighter {
        static LOADED: OnceLock<Highlighter> = OnceLock::new();
        LOADED.get_or_init(|| {
            let highlighter = Highlighter::new();
            highlighter.load_blocking();
            highlighter
        })
    }

    #[test]
    fn an_unloaded_highlighter_declines_rather_than_blocking() {
        let highlighter = Highlighter::new();
        assert!(!highlighter.is_ready());
        assert!(highlighter
            .highlight("fn main() {}", Some("rust"), FALLBACK_CODE_THEME)
            .is_none());
    }

    #[test]
    fn loading_off_thread_reports_when_it_is_ready() {
        let highlighter = Highlighter::new();
        let (tx, rx) = mpsc::channel();
        highlighter.spawn_load(move || {
            let _ = tx.send(());
        });

        rx.recv_timeout(Duration::from_secs(30))
            .expect("the loader must report back");
        assert!(highlighter.is_ready());
    }

    #[test]
    fn highlights_a_known_language() {
        let lines = loaded()
            .highlight(
                "fn main() {\n    let x = 1;\n}\n",
                Some("rust"),
                FALLBACK_CODE_THEME,
            )
            .expect("rust is a known language");

        assert_eq!(lines.len(), 3);
        // More than one colour on the first line, or nothing was highlighted.
        let colours: Vec<_> = lines[0].spans.iter().map(|s| s.style.fg).collect();
        assert!(colours.windows(2).any(|w| w[0] != w[1]), "{colours:?}");
    }

    #[test]
    fn an_unknown_language_falls_back_to_plain_text() {
        assert!(loaded()
            .highlight("...", Some("not-a-language"), FALLBACK_CODE_THEME)
            .is_none());
        assert!(loaded()
            .highlight("...", None, FALLBACK_CODE_THEME)
            .is_none());
    }

    #[test]
    fn an_unknown_theme_falls_back_rather_than_failing() {
        let lines = loaded().highlight("fn main() {}\n", Some("rust"), "no-such-theme");
        assert!(lines.is_some());
    }

    #[test]
    fn highlighted_lines_carry_no_background() {
        // The theme's code_block_bg owns the background; a syntect theme's own
        // background would cut a differently coloured rectangle out of the page.
        let lines = loaded()
            .highlight("fn main() {}\n", Some("rust"), FALLBACK_CODE_THEME)
            .unwrap();

        for span in &lines[0].spans {
            assert_eq!(span.style.bg, None);
        }
    }

    #[test]
    fn line_endings_are_not_rendered() {
        let lines = loaded()
            .highlight(
                "let a = 1;\nlet b = 2;\n",
                Some("rust"),
                FALLBACK_CODE_THEME,
            )
            .unwrap();

        for line in &lines {
            for span in &line.spans {
                assert!(!span.content.contains('\n'), "{span:?}");
                assert!(!span.content.contains('\r'), "{span:?}");
            }
        }
    }
}
