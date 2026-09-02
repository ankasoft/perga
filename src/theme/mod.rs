//! Theme resolution: built-in themes, user theme files, and colour degradation.
//!
//! A theme file is a tree of optional style tables. Loading one produces a
//! [`ThemeFile`], which is merged onto the built-in `dark` theme to produce a
//! [`Theme`] in which every style is concrete. Widgets only ever see [`Theme`];
//! nothing in the UI hardcodes a colour.

pub mod builtin;
pub mod schema;

use ratatui::style::Style;
use serde::Deserialize;

use crate::theme::schema::StyleDef;

/// Declares a group of theme keys as a pair of structs: an all-optional `*File`
/// form for deserialization and a fully resolved form of `ratatui` styles.
macro_rules! theme_group {
    ($(#[$meta:meta])* $file:ident => $resolved:ident { $($key:ident),* $(,)? }) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
        #[serde(default, deny_unknown_fields)]
        pub struct $file {
            $(pub $key: Option<StyleDef>,)*
        }

        $(#[$meta])*
        #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
        pub struct $resolved {
            $(pub $key: Style,)*
        }

        impl $resolved {
            /// Layer `file` on top of this group, key by key.
            fn merge(&mut self, file: &$file) {
                $(
                    if let Some(def) = file.$key {
                        self.$key = def.merge_onto(self.$key);
                    }
                )*
            }

            /// Apply `f` to every style in the group.
            fn map_styles(&mut self, f: &mut impl FnMut(Style) -> Style) {
                $(self.$key = f(self.$key);)*
            }
        }
    };
}

theme_group! {
    /// Chrome: borders, the title bar, the status bar, and the welcome logo.
    UiStylesFile => UiStyles {
        background,
        border,
        border_focused,
        title,
        status_bar,
        status_mode,
        status_warning,
        status_error,
        selection,
        scrollbar,
        logo,
        logo_subtitle,
    }
}

theme_group! {
    /// The tab bar.
    TabStylesFile => TabStyles {
        active,
        inactive,
        dirty,
    }
}

theme_group! {
    /// The sidebar in all four modes.
    SidebarStylesFile => SidebarStyles {
        directory,
        file,
        file_active,
        file_other,
        mode_active,
        mode_inactive,
        r#match,
        line_number,
    }
}

theme_group! {
    /// Rendered Markdown.
    MarkdownStylesFile => MarkdownStyles {
        h1,
        h2,
        h3,
        h4,
        h5,
        h6,
        text,
        emphasis,
        strong,
        strikethrough,
        blockquote,
        blockquote_bar,
        code_inline,
        code_block_bg,
        link,
        link_focused,
        link_broken,
        link_external,
        wikilink,
        list_marker,
        task_done,
        task_todo,
        table_border,
        table_header,
        rule,
        footnote,
        image_placeholder,
        html,
        frontmatter,
    }
}

theme_group! {
    /// Link hint labels.
    HintStylesFile => HintStyles {
        label,
    }
}

/// A theme file as written on disk. Every key is optional.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub struct ThemeFile {
    /// The theme's own name. Informational; the name a theme is selected by is
    /// its filename.
    pub name: Option<String>,
    /// The `syntect` theme for fenced code blocks. Overrides `theme.code_theme`.
    pub code_theme: Option<String>,
    pub ui: UiStylesFile,
    pub tabs: TabStylesFile,
    pub sidebar: SidebarStylesFile,
    pub markdown: MarkdownStylesFile,
    pub hints: HintStylesFile,
}

/// What the terminal's background looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Background {
    /// Light text on a dark ground.
    Dark,
    /// Dark text on a light ground.
    Light,
}

/// Detect the terminal's background from `COLORFGBG`.
///
/// The variable is `fg;bg` in ANSI colour numbers, set by rxvt, Konsole, and
/// several others. `None` when it is absent or unreadable, in which case the
/// caller uses `dark` — see `docs/decisions.md` for why the OSC 11 query the
/// specification also names is not attempted.
pub fn detect_background() -> Option<Background> {
    let raw = std::env::var("COLORFGBG").ok()?;
    background_from_colorfgbg(&raw)
}

/// Read a `COLORFGBG` value.
fn background_from_colorfgbg(raw: &str) -> Option<Background> {
    // The last field is the background; some terminals put a third field
    // between them for the cursor.
    let background: u8 = raw.rsplit(';').next()?.trim().parse().ok()?;

    // 0-6 and 8 are the dark half of the ANSI palette; 7 and 9-15 are the
    // light half. This is the same rule vim uses.
    Some(match background {
        0..=6 | 8 => Background::Dark,
        _ => Background::Light,
    })
}

/// Whether the terminal claims truecolour support.
pub fn truecolor() -> bool {
    std::env::var("COLORTERM")
        .map(|v| v.contains("truecolor") || v.contains("24bit"))
        .unwrap_or(false)
}

/// A theme in which every style is concrete.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Theme {
    pub name: String,
    pub code_theme: Option<String>,
    pub ui: UiStyles,
    pub tabs: TabStyles,
    pub sidebar: SidebarStyles,
    pub markdown: MarkdownStyles,
    pub hints: HintStyles,
}

impl Theme {
    /// The built-in `dark` theme, the base every other theme inherits from.
    ///
    /// # Panics
    ///
    /// Panics if the embedded theme does not parse. That is a build-time
    /// mistake in this repository, caught by [`builtin`]'s tests, never
    /// something a user can trigger.
    pub fn dark() -> Self {
        let file: ThemeFile =
            toml::from_str(builtin::DARK).expect("the embedded dark theme must parse");

        let mut theme = Theme {
            name: "dark".to_string(),
            ..Theme::default()
        };
        theme.apply(&file);
        theme
    }

    /// Layer a theme file on top of this theme.
    pub fn apply(&mut self, file: &ThemeFile) {
        if let Some(name) = &file.name {
            self.name.clone_from(name);
        }
        if let Some(code_theme) = &file.code_theme {
            self.code_theme = Some(code_theme.clone());
        }
        self.ui.merge(&file.ui);
        self.tabs.merge(&file.tabs);
        self.sidebar.merge(&file.sidebar);
        self.markdown.merge(&file.markdown);
        self.hints.merge(&file.hints);
    }

    /// Visit every style in the theme, for the tests that assert across all
    /// of them at once.
    pub fn for_each_style(&self, f: &mut impl FnMut(Style)) {
        let mut copy = self.clone();
        copy.map_styles(|style| {
            f(style);
            style
        });
    }

    /// Apply `f` to every style in the theme.
    fn map_styles(&mut self, mut f: impl FnMut(Style) -> Style) {
        self.ui.map_styles(&mut f);
        self.tabs.map_styles(&mut f);
        self.sidebar.map_styles(&mut f);
        self.markdown.map_styles(&mut f);
        self.hints.map_styles(&mut f);
    }

    /// A built-in theme by name, or `None` when there is no such theme.
    pub fn builtin(name: &str) -> Option<Self> {
        let source = builtin::by_name(name)?;
        let file: ThemeFile = toml::from_str(source).ok()?;

        let mut theme = Theme::dark();
        theme.apply(&file);
        Some(theme)
    }

    /// Resolve `theme.name` to a theme, warning about whatever went wrong.
    ///
    /// `auto` picks `dark` or `light` from the terminal background. A name that
    /// is not built in is looked for as `<name>.toml` in the theme directory,
    /// and a name that matches nothing at all falls back to `dark` — perga
    /// opens the vault either way.
    pub fn resolve(name: &str, dir: Option<&std::path::Path>, warnings: &mut Vec<String>) -> Self {
        if name.eq_ignore_ascii_case("auto") {
            return match detect_background() {
                Some(Background::Light) => Theme::builtin("light").unwrap_or_else(Theme::dark),
                // Dark is the answer both when the terminal says so and when
                // it says nothing, which is most terminals.
                _ => Theme::dark(),
            };
        }

        if let Some(theme) = Theme::builtin(name) {
            return theme;
        }

        let Some(dir) = dir else {
            warnings.push(format!("no theme named `{name}`; using `dark`"));
            return Theme::dark();
        };

        let path = dir.join(format!("{name}.toml"));
        match Theme::from_file(&path) {
            Ok(theme) => theme,
            Err(e) => {
                warnings.push(format!("cannot use theme `{name}`: {e}"));
                Theme::dark()
            }
        }
    }

    /// Load a theme file, inheriting every key it does not set from `dark`.
    pub fn from_file(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read `{}`: {e}", path.display()))?;

        let file: ThemeFile = toml::from_str(&text)
            .map_err(|e| format!("`{}` is not a valid theme: {e}", path.display()))?;

        let mut theme = Theme::dark();
        theme.apply(&file);

        // A theme file that does not name itself is known by its filename,
        // which is what it is selected by anyway.
        if file.name.is_none() {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                theme.name = stem.to_string();
            }
        }

        Ok(theme)
    }

    /// Map every truecolour value to its nearest ANSI-256 index.
    ///
    /// Applied when `COLORTERM` does not claim truecolour. Degrading is better
    /// than sending 24-bit escapes to a terminal that will print them as text
    /// or ignore them.
    pub fn degrade_to_256(&mut self) {
        self.map_styles(|style| Style {
            fg: style.fg.map(nearest_256),
            bg: style.bg.map(nearest_256),
            underline_color: style.underline_color.map(nearest_256),
            ..style
        });
    }

    /// Drop every colour, keeping bold, dim, italic, underline, reverse, and
    /// strikethrough. Applied when `NO_COLOR` is set and non-empty, per
    /// <https://no-color.org>.
    pub fn strip_colors(&mut self) {
        self.map_styles(|style| Style {
            fg: None,
            bg: None,
            underline_color: None,
            ..style
        });
    }
}

/// The nearest ANSI-256 index to a truecolour value.
///
/// The 6×6×6 colour cube and the 24-step grey ramp, whichever is closer. Only
/// `Color::Rgb` is touched; a named or indexed colour already means something
/// on a 256-colour terminal.
fn nearest_256(color: ratatui::style::Color) -> ratatui::style::Color {
    use ratatui::style::Color;

    let Color::Rgb(r, g, b) = color else {
        return color;
    };

    // A grey is better served by the ramp than by the cube, which only has six
    // steps per channel and would visibly tint it.
    let grey_level = (u16::from(r) + u16::from(g) + u16::from(b)) / 3;
    let is_grey = r.abs_diff(g) < 10 && g.abs_diff(b) < 10 && r.abs_diff(b) < 10;

    if is_grey {
        if grey_level < 4 {
            return Color::Indexed(16);
        }
        if grey_level > 246 {
            return Color::Indexed(231);
        }
        let step = ((grey_level - 8) / 10).min(23) as u8;
        return Color::Indexed(232 + step);
    }

    let index = |v: u8| -> u16 {
        // The cube's levels are 0, 95, 135, 175, 215, 255.
        const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];
        LEVELS
            .iter()
            .enumerate()
            .min_by_key(|(_, level)| level.abs_diff(v))
            .map_or(0, |(at, _)| at as u16)
    };

    Color::Indexed((16 + 36 * index(r) + 6 * index(g) + index(b)) as u8)
}

#[cfg(test)]
mod tests {
    use ratatui::style::{Color, Modifier};

    use super::*;
    use crate::theme::schema::{ThemeColor, ThemeError};

    #[test]
    fn parses_hex_colors() {
        assert_eq!(
            "#89b4fa".parse::<ThemeColor>().unwrap(),
            ThemeColor(Color::Rgb(0x89, 0xb4, 0xfa))
        );
    }

    #[test]
    fn parses_ansi_indices_and_names() {
        assert_eq!(
            "12".parse::<ThemeColor>().unwrap(),
            ThemeColor(Color::Indexed(12))
        );
        assert_eq!(
            "bright_cyan".parse::<ThemeColor>().unwrap(),
            ThemeColor(Color::LightCyan)
        );
        assert_eq!("RED".parse::<ThemeColor>().unwrap(), ThemeColor(Color::Red));
    }

    #[test]
    fn rejects_malformed_colors() {
        for bad in ["#89b4f", "#gggggg", "chartreuse", "256", ""] {
            assert!(
                matches!(bad.parse::<ThemeColor>(), Err(ThemeError::BadColor(_))),
                "{bad} should not parse"
            );
        }
    }

    #[test]
    fn dark_theme_resolves_every_key() {
        let dark = Theme::dark();
        assert_eq!(dark.name, "dark");
        assert_eq!(dark.code_theme.as_deref(), Some("base16-ocean.dark"));
        assert_eq!(dark.markdown.h1.fg, Some(Color::Rgb(0xf3, 0x8b, 0xa8)));
        assert!(dark.markdown.h1.add_modifier.contains(Modifier::BOLD));
        assert_eq!(dark.ui.background.bg, Some(Color::Rgb(0x1e, 0x1e, 0x2e)));
        assert!(dark
            .markdown
            .strikethrough
            .add_modifier
            .contains(Modifier::CROSSED_OUT));
    }

    #[test]
    fn partial_theme_inherits_from_dark() {
        let file: ThemeFile = toml::from_str(
            r#"
            name = "mine"
            [markdown]
            h1 = { fg = "red" }
            "#,
        )
        .unwrap();

        let mut theme = Theme::dark();
        theme.apply(&file);

        assert_eq!(theme.name, "mine");
        assert_eq!(theme.markdown.h1.fg, Some(Color::Red));
        // Inherited from dark, both the untouched modifier on h1 and all of h2.
        assert!(theme.markdown.h1.add_modifier.contains(Modifier::BOLD));
        assert_eq!(theme.markdown.h2.fg, Some(Color::Rgb(0xfa, 0xb3, 0x87)));
        // Inherited: code_theme was not overridden.
        assert_eq!(theme.code_theme.as_deref(), Some("base16-ocean.dark"));
    }

    #[test]
    fn colorfgbg_says_which_half_of_the_palette_the_background_is_in() {
        assert_eq!(background_from_colorfgbg("15;0"), Some(Background::Dark));
        assert_eq!(background_from_colorfgbg("0;15"), Some(Background::Light));
        assert_eq!(background_from_colorfgbg("7;0"), Some(Background::Dark));
        // Some terminals put the cursor colour between the two.
        assert_eq!(
            background_from_colorfgbg("15;default;0"),
            Some(Background::Dark)
        );
        assert_eq!(background_from_colorfgbg("nonsense"), None);
        assert_eq!(background_from_colorfgbg(""), None);
    }

    #[test]
    fn each_builtin_theme_loads_by_name() {
        for name in ["dark", "light", "high-contrast"] {
            let theme = Theme::builtin(name).unwrap_or_else(|| panic!("`{name}` must load"));
            assert_eq!(theme.name, name);
        }
        assert!(Theme::builtin("chartreuse").is_none());
    }

    #[test]
    fn an_unknown_theme_name_warns_and_falls_back_to_dark() {
        let mut warnings = Vec::new();
        let theme = Theme::resolve("chartreuse", None, &mut warnings);

        assert_eq!(theme.name, "dark");
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("chartreuse"));
    }

    #[test]
    fn a_theme_file_is_loaded_and_inherits_what_it_does_not_set() {
        let dir = std::env::temp_dir().join(format!("perga-theme-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("mine.toml"),
            "[markdown]\nh1 = { fg = \"green\" }\n",
        )
        .unwrap();

        let mut warnings = Vec::new();
        let theme = Theme::resolve("mine", Some(&dir), &mut warnings);

        assert!(warnings.is_empty(), "{warnings:?}");
        assert_eq!(theme.name, "mine", "a file names itself by its stem");
        assert_eq!(theme.markdown.h1.fg, Some(Color::Green));
        assert_eq!(theme.markdown.h2.fg, Some(Color::Rgb(0xfa, 0xb3, 0x87)));
    }

    #[test]
    fn a_corrupt_theme_file_warns_and_falls_back_to_dark() {
        let dir = std::env::temp_dir().join(format!("perga-theme-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("bad.toml"), "[markdown\nh1 = ").unwrap();

        let mut warnings = Vec::new();
        let theme = Theme::resolve("bad", Some(&dir), &mut warnings);

        assert_eq!(theme.name, "dark");
        assert!(warnings[0].contains("bad"), "{warnings:?}");
    }

    #[test]
    fn degrading_maps_truecolour_to_the_256_palette() {
        let mut theme = Theme::dark();
        theme.degrade_to_256();

        theme.for_each_style(&mut |style| {
            for colour in [style.fg, style.bg] {
                assert!(
                    !matches!(colour, Some(Color::Rgb(..))),
                    "a truecolour value survived degradation: {colour:?}"
                );
            }
        });
    }

    #[test]
    fn degrading_picks_a_recognisable_neighbour() {
        assert_eq!(nearest_256(Color::Rgb(255, 0, 0)), Color::Indexed(196));
        assert_eq!(nearest_256(Color::Rgb(0, 0, 0)), Color::Indexed(16));
        assert_eq!(nearest_256(Color::Rgb(255, 255, 255)), Color::Indexed(231));
        // A grey goes to the ramp rather than to the cube.
        assert!(
            matches!(nearest_256(Color::Rgb(128, 128, 128)), Color::Indexed(n) if (232..=255).contains(&n))
        );
        // A colour that already means something is left alone.
        assert_eq!(nearest_256(Color::Red), Color::Red);
        assert_eq!(nearest_256(Color::Indexed(42)), Color::Indexed(42));
    }

    #[test]
    fn strip_colors_keeps_modifiers() {
        let mut theme = Theme::dark();
        theme.strip_colors();

        assert_eq!(theme.markdown.h1.fg, None);
        assert_eq!(theme.markdown.h1.bg, None);
        assert!(theme.markdown.h1.add_modifier.contains(Modifier::BOLD));
        assert!(theme
            .markdown
            .link
            .add_modifier
            .contains(Modifier::UNDERLINED));
    }
}
