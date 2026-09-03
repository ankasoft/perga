//! The welcome screen and its three ASCII logo size tiers.
//!
//! A tab with no document open shows this: on launch in an empty directory, and
//! on every `Ctrl+T`. It is the only place in perga where branding appears —
//! there is no startup splash, no persistent banner, nothing in the help
//! overlay, and nothing in `--version`.
//!
//! The art uses only the Unicode block elements `█`, `▀`, `▄`, and space, which
//! render in effectively every monospace font.

use ratatui::layout::{Alignment, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Paragraph, Widget};

use unicode_width::UnicodeWidthStr;

use crate::action::Action;
use crate::config::keymap::Keymap;
use crate::theme::Theme;

/// Widest tier, for a viewport of at least [`LARGE_MIN_WIDTH`] columns.
const LOGO_LARGE: &[&str] = &[
    "██████  ███████ ██████   ██████   █████",
    "██   ██ ██      ██   ██ ██       ██   ██",
    "██████  █████   ██████  ██   ███ ███████",
    "██      ██      ██   ██ ██    ██ ██   ██",
    "██      ███████ ██   ██  ██████  ██   ██",
];

/// Middle tier, for a viewport between [`MEDIUM_MIN_WIDTH`] and
/// [`LARGE_MIN_WIDTH`] columns.
const LOGO_MEDIUM: &[&str] = &[
    "█▀▀█ █▀▀ █▀▀█ █▀▀▀ █▀▀█",
    "█▄▄█ █▀▀ █▄▄▀ █ ▀█ █▄▄█",
    "█    ▀▀▀ ▀  ▀ ▀▀▀▀ ▀  ▀",
];

/// The large tier needs this many columns.
pub const LARGE_MIN_WIDTH: u16 = 56;
/// The medium tier needs this many columns.
pub const MEDIUM_MIN_WIDTH: u16 = 36;
/// Below this many rows the logo is dropped entirely.
pub const MIN_LOGO_HEIGHT: u16 = 12;
/// The onboarding hints need this many columns; below it they reduce to a
/// single `? for help`.
const HINTS_MIN_WIDTH: u16 = 26;

/// Which size of logo the viewport can hold.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogoTier {
    /// Five rows of block art.
    Large,
    /// Three rows of half-block art.
    Medium,
    /// The word `perga` and a rule. No block art.
    Minimal,
    /// No logo at all: the viewport is too short.
    None,
}

impl LogoTier {
    /// Pick the tier that fits a viewport of this size.
    ///
    /// This is not cosmetic. A 40-column banner drawn into a 40-column terminal
    /// overflows and corrupts the frame, so the tier is chosen from the
    /// available width, never assumed.
    pub fn for_size(width: u16, height: u16) -> Self {
        if height < MIN_LOGO_HEIGHT {
            LogoTier::None
        } else if width >= LARGE_MIN_WIDTH {
            LogoTier::Large
        } else if width >= MEDIUM_MIN_WIDTH {
            LogoTier::Medium
        } else {
            LogoTier::Minimal
        }
    }

    /// The block art for this tier, if it has any.
    fn art(self) -> Option<&'static [&'static str]> {
        match self {
            LogoTier::Large => Some(LOGO_LARGE),
            LogoTier::Medium => Some(LOGO_MEDIUM),
            LogoTier::Minimal | LogoTier::None => None,
        }
    }
}

/// The onboarding hints shown under the logo, so an empty screen doubles as
/// onboarding. The keys come from the keymap, so a remap moves them too.
const HINTS: &[(Action, &str)] = &[
    (Action::FocusNext, "focus the file tree"),
    (Action::OpenQuickSwitcher, "find a document"),
    (Action::ToggleSidebar, "hide the sidebar"),
    (Action::ToggleHelp, "all keybindings"),
];

/// The welcome screen.
pub struct Welcome<'a> {
    theme: &'a Theme,
    keymap: &'a Keymap,
    version: &'a str,
}

impl<'a> Welcome<'a> {
    /// Build the welcome screen for a theme and the resolved keymap.
    pub fn new(theme: &'a Theme, keymap: &'a Keymap) -> Self {
        Welcome {
            theme,
            keymap,
            version: env!("CARGO_PKG_VERSION"),
        }
    }

    /// The onboarding block, reduced to a single line when the viewport is too
    /// narrow to hold the full hints without wrapping.
    fn onboarding(&self, width: u16, key_style: Style, text_style: Style) -> Vec<Line<'static>> {
        let help_key = self
            .keymap
            .binding_for(&Action::ToggleHelp)
            .unwrap_or_else(|| "?".to_string());

        if width < HINTS_MIN_WIDTH {
            return vec![Line::styled(format!("{help_key} for help"), text_style)];
        }

        HINTS
            .iter()
            .filter_map(|(action, what)| {
                let key = self.keymap.binding_for(action)?;
                Some(hint_line(&key, what, key_style, text_style))
            })
            .collect()
    }

    /// Build the lines for a viewport of this size.
    ///
    /// Separated from rendering so the tier logic can be tested without a
    /// backend.
    pub fn lines(&self, area: Rect) -> Text<'static> {
        let tier = LogoTier::for_size(area.width, area.height);
        let logo = self.theme.ui.logo;
        let subtitle = self.theme.ui.logo_subtitle;

        let mut lines: Vec<Line<'static>> = Vec::new();

        match tier {
            LogoTier::Large | LogoTier::Medium => {
                let art = tier.art().expect("these tiers have art");
                // The art is padded to a common width so that centring places
                // every row on the same column.
                let art_width = art.iter().map(|r| r.width()).max().unwrap_or(0);
                for row in art {
                    lines.push(Line::styled(format!("{row:<art_width$}"), logo));
                }
                lines.push(Line::default());
                lines.push(Line::styled(format!("perga {}", self.version), subtitle));
                lines.push(Line::default());
                lines.extend(self.onboarding(area.width, logo, subtitle));
            }
            LogoTier::Minimal => {
                lines.push(Line::styled("perga".to_string(), self.theme.ui.title));
                lines.push(Line::styled("─────".to_string(), subtitle));
                lines.push(Line::default());
                lines.push(Line::styled(self.version.to_string(), subtitle));
                let help_key = self
                    .keymap
                    .binding_for(&Action::ToggleHelp)
                    .unwrap_or_else(|| "?".to_string());
                lines.push(Line::styled(format!("{help_key} for help"), subtitle));
            }
            LogoTier::None => {
                lines.push(Line::styled(format!("perga {}", self.version), subtitle));
                lines.extend(self.onboarding(area.width, logo, subtitle));
            }
        }

        Text::from(lines)
    }
}

/// One `Ctrl+O   find a document` line, with the key and the description
/// styled separately.
fn hint_line(key: &str, what: &str, key_style: Style, text_style: Style) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:>8}"), key_style),
        Span::styled(format!("   {what}"), text_style),
    ])
}

impl Widget for Welcome<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let text = self.lines(area);

        // Placed at roughly a third of the height rather than centred: text
        // centred slightly high reads better.
        let content_height = text.lines.len() as u16;
        let free = area.height.saturating_sub(content_height);
        let top = free / 3;

        let block = Rect {
            y: area.y + top,
            height: content_height.min(area.height),
            ..area
        };

        Paragraph::new(text)
            .alignment(Alignment::Center)
            .render(block, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiers_are_selected_by_width() {
        assert_eq!(LogoTier::for_size(100, 40), LogoTier::Large);
        assert_eq!(LogoTier::for_size(56, 40), LogoTier::Large);
        assert_eq!(LogoTier::for_size(55, 40), LogoTier::Medium);
        assert_eq!(LogoTier::for_size(36, 40), LogoTier::Medium);
        assert_eq!(LogoTier::for_size(35, 40), LogoTier::Minimal);
    }

    #[test]
    fn a_short_viewport_drops_the_logo() {
        assert_eq!(LogoTier::for_size(100, 11), LogoTier::None);
        assert_eq!(LogoTier::for_size(100, 12), LogoTier::Large);
    }

    #[test]
    fn art_fits_inside_the_narrowest_viewport_of_its_tier() {
        // The whole point of the tiers: a banner wider than the viewport
        // overflows and corrupts the frame.
        for row in LOGO_LARGE {
            assert!(
                row.width() as u16 <= LARGE_MIN_WIDTH,
                "large tier row is {} columns, tier starts at {LARGE_MIN_WIDTH}",
                row.width()
            );
        }
        for row in LOGO_MEDIUM {
            assert!(
                row.width() as u16 <= MEDIUM_MIN_WIDTH,
                "medium tier row is {} columns, tier starts at {MEDIUM_MIN_WIDTH}",
                row.width()
            );
        }
    }

    #[test]
    fn art_uses_only_portable_glyphs() {
        // Braille and uncommon box-drawing produce tofu on terminals without
        // the glyphs; these four render everywhere.
        for row in LOGO_LARGE.iter().chain(LOGO_MEDIUM) {
            for c in row.chars() {
                assert!(
                    matches!(c, '█' | '▀' | '▄' | ' '),
                    "unexpected glyph {c:?} in the logo"
                );
            }
        }
    }

    #[test]
    fn rendered_art_rows_are_all_the_same_width() {
        // The source art is ragged by a column; padding at render time is what
        // keeps centring from shifting a row sideways.
        let theme = Theme::dark();
        let keymap = Keymap::defaults();
        let welcome = Welcome::new(&theme, &keymap);

        for (width, rows) in [(100u16, LOGO_LARGE.len()), (48, LOGO_MEDIUM.len())] {
            let text = welcome.lines(Rect::new(0, 0, width, 40));
            let widths: Vec<_> = text
                .lines
                .iter()
                .take(rows)
                .map(|l| l.spans.iter().map(|s| s.content.width()).sum::<usize>())
                .collect();
            assert!(
                widths.windows(2).all(|w| w[0] == w[1]),
                "ragged logo rows at width {width}: {widths:?}"
            );
        }
    }

    #[test]
    fn no_line_overflows_the_viewport_at_any_width() {
        let theme = Theme::dark();
        let keymap = Keymap::defaults();
        let welcome = Welcome::new(&theme, &keymap);

        for width in [20u16, 30, 35, 36, 48, 55, 56, 80, 100, 200] {
            for height in [8u16, 10, 11, 12, 24, 40] {
                let area = Rect::new(0, 0, width, height);
                for line in welcome.lines(area).lines {
                    let rendered: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
                    assert!(
                        rendered.width() as u16 <= width,
                        "{width}x{height}: {rendered:?} is {} columns",
                        rendered.width()
                    );
                }
            }
        }
    }
}
