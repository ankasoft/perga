//! The built-in themes, embedded at compile time from `themes/`.

/// The default theme, and the base every other theme inherits missing keys from.
pub const DARK: &str = include_str!("../../themes/dark.toml");

/// A light theme for a pale terminal background.
pub const LIGHT: &str = include_str!("../../themes/light.toml");

/// An ANSI-16 theme for terminals without truecolour, and for readers who need
/// the contrast.
pub const HIGH_CONTRAST: &str = include_str!("../../themes/high-contrast.toml");

/// Every theme embedded in the binary, by the name it is selected with.
pub const ALL: &[(&str, &str)] = &[
    ("dark", DARK),
    ("light", LIGHT),
    ("high-contrast", HIGH_CONTRAST),
];

/// The source of a built-in theme, by name.
pub fn by_name(name: &str) -> Option<&'static str> {
    ALL.iter()
        .find(|(known, _)| known.eq_ignore_ascii_case(name))
        .map(|(_, source)| *source)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeFile;

    #[test]
    fn every_builtin_theme_parses() {
        for (name, source) in ALL {
            toml::from_str::<ThemeFile>(source)
                .unwrap_or_else(|e| panic!("built-in theme `{name}` must parse: {e}"));
        }
    }

    #[test]
    fn every_builtin_theme_names_itself() {
        for (name, source) in ALL {
            let file: ThemeFile = toml::from_str(source).unwrap();
            assert_eq!(
                file.name.as_deref(),
                Some(*name),
                "a theme's `name` must match the name it is selected by"
            );
        }
    }

    #[test]
    fn themes_are_found_by_name_case_insensitively() {
        assert!(by_name("light").is_some());
        assert!(by_name("HIGH-CONTRAST").is_some());
        assert!(by_name("chartreuse").is_none());
    }

    /// The high-contrast theme exists for terminals that have sixteen colours
    /// and no more, so a hex value or a 256-colour index in it defeats its
    /// whole purpose.
    #[test]
    fn the_high_contrast_theme_uses_only_ansi_16_colours() {
        use ratatui::style::Color;

        let mut theme = crate::theme::Theme::dark();
        theme.apply(&toml::from_str(HIGH_CONTRAST).unwrap());

        theme.for_each_style(&mut |style| {
            for colour in [style.fg, style.bg] {
                match colour {
                    None
                    | Some(Color::Black | Color::Red | Color::Green | Color::Yellow)
                    | Some(Color::Blue | Color::Magenta | Color::Cyan | Color::Gray)
                    | Some(Color::DarkGray | Color::LightRed | Color::LightGreen)
                    | Some(Color::LightYellow | Color::LightBlue | Color::LightMagenta)
                    | Some(Color::LightCyan | Color::White | Color::Reset) => {}
                    other => panic!("high-contrast uses a non-ANSI-16 colour: {other:?}"),
                }
            }
        });
    }
}
