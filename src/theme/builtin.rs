//! The built-in themes, embedded at compile time from `themes/`.

/// The default theme, and the base every other theme inherits missing keys from.
pub const DARK: &str = include_str!("../../themes/dark.toml");

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::ThemeFile;

    /// Every theme embedded in the binary, by the name it is selected with.
    ///
    /// The other two built-ins join this list in M10.
    const ALL: &[(&str, &str)] = &[("dark", DARK)];

    #[test]
    fn every_builtin_theme_parses() {
        for (name, source) in ALL {
            toml::from_str::<ThemeFile>(source)
                .unwrap_or_else(|e| panic!("built-in theme `{name}` must parse: {e}"));
        }
    }
}
