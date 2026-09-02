//! Serde structures for theme files and their resolution to `ratatui` styles.
//!
//! Every key in a theme file is optional. A partially specified theme inherits
//! the missing keys from the built-in `dark` theme, which is the reason this
//! module models a theme as a tree of `Option`s (`ThemeFile`) that is merged
//! down onto a fully populated tree of styles ([`Theme`]).

use std::str::FromStr;

use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;

/// A colour as written in a theme file.
///
/// Accepts `#rrggbb`, an ANSI index `0`-`255`, or a named ANSI colour with an
/// optional `bright_` prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThemeColor(pub Color);

/// The reason a theme value could not be understood.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ThemeError {
    /// A colour string matched none of the accepted forms.
    #[error("`{0}` is not a colour: expected #rrggbb, 0-255, or an ANSI colour name")]
    BadColor(String),
}

impl FromStr for ThemeColor {
    type Err = ThemeError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let raw = s.trim();

        if let Some(hex) = raw.strip_prefix('#') {
            if hex.len() == 6 {
                if let Ok(v) = u32::from_str_radix(hex, 16) {
                    let [_, r, g, b] = v.to_be_bytes();
                    return Ok(ThemeColor(Color::Rgb(r, g, b)));
                }
            }
            return Err(ThemeError::BadColor(raw.to_string()));
        }

        if let Ok(index) = raw.parse::<u8>() {
            return Ok(ThemeColor(Color::Indexed(index)));
        }

        let named = match raw.to_ascii_lowercase().as_str() {
            "black" => Color::Black,
            "red" => Color::Red,
            "green" => Color::Green,
            "yellow" => Color::Yellow,
            "blue" => Color::Blue,
            "magenta" => Color::Magenta,
            "cyan" => Color::Cyan,
            "white" => Color::Gray,
            "bright_black" => Color::DarkGray,
            "bright_red" => Color::LightRed,
            "bright_green" => Color::LightGreen,
            "bright_yellow" => Color::LightYellow,
            "bright_blue" => Color::LightBlue,
            "bright_magenta" => Color::LightMagenta,
            "bright_cyan" => Color::LightCyan,
            "bright_white" => Color::White,
            _ => return Err(ThemeError::BadColor(raw.to_string())),
        };

        Ok(ThemeColor(named))
    }
}

impl<'de> Deserialize<'de> for ThemeColor {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Accept both `"#89b4fa"` and a bare integer such as `12`, since TOML
        // distinguishes the two and a theme author reasonably writes either.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Text(String),
            Index(u8),
        }

        match Raw::deserialize(deserializer)? {
            Raw::Index(i) => Ok(ThemeColor(Color::Indexed(i))),
            Raw::Text(s) => s.parse().map_err(serde::de::Error::custom),
        }
    }
}

/// One style table from a theme file, with every field optional.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StyleDef {
    pub fg: Option<ThemeColor>,
    pub bg: Option<ThemeColor>,
    pub bold: Option<bool>,
    pub italic: Option<bool>,
    pub underline: Option<bool>,
    pub dim: Option<bool>,
    pub reversed: Option<bool>,
    pub crossed_out: Option<bool>,
}

impl StyleDef {
    /// Layer this definition on top of `base`, keeping `base` wherever this
    /// definition is silent.
    pub fn merge_onto(self, base: Style) -> Style {
        let mut style = base;

        if let Some(ThemeColor(c)) = self.fg {
            style = style.fg(c);
        }
        if let Some(ThemeColor(c)) = self.bg {
            style = style.bg(c);
        }

        for (flag, modifier) in [
            (self.bold, Modifier::BOLD),
            (self.italic, Modifier::ITALIC),
            (self.underline, Modifier::UNDERLINED),
            (self.dim, Modifier::DIM),
            (self.reversed, Modifier::REVERSED),
            (self.crossed_out, Modifier::CROSSED_OUT),
        ] {
            match flag {
                Some(true) => style = style.add_modifier(modifier),
                Some(false) => style = style.remove_modifier(modifier),
                None => {}
            }
        }

        style
    }
}
