//! Theme resolution: built-in themes, user theme files, and colour degradation.
//!
//! A theme file is a tree of optional style tables. Loading one produces a
//! [`ThemeFile`], which is merged onto the built-in `dark` theme to produce a
//! [`Theme`] in which every style is concrete. Widgets only ever see [`Theme`];
//! nothing in the UI hardcodes a colour.

pub mod builtin;
pub mod schema;

use ratatui::style::{Color, Style};
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

            /// Every style in the group, with the name it is written under in
            /// a theme file. For the tests that have to say *which* key is
            /// wrong.
            pub fn named(&self) -> Vec<(&'static str, Style)> {
                vec![$((stringify!($key), self.$key),)*]
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

    /// Every style in the theme, with the name it is written under.
    pub fn named_styles(&self) -> Vec<(&'static str, Style)> {
        let mut out = self.ui.named();
        out.extend(self.tabs.named());
        out.extend(self.sidebar.named());
        out.extend(self.markdown.named());
        out.extend(self.hints.named());
        out
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

/// The RGB an ANSI-256 index stands for, in the standard xterm palette.
///
/// Indices 0-15 are whatever the reader configured in their terminal and have
/// no fixed value; the 6x6x6 cube and the grey ramp above them do. This is what
/// lets the contrast test measure a degraded theme — the palette a terminal
/// without truecolour actually receives.
pub fn ansi256_rgb(index: u8) -> Option<Color> {
    /// The six levels each channel of the colour cube takes.
    const LEVELS: [u8; 6] = [0, 95, 135, 175, 215, 255];

    if index < 16 {
        return None;
    }

    if index >= 232 {
        let grey = 8 + 10 * (index - 232);
        return Some(Color::Rgb(grey, grey, grey));
    }

    let cube = index - 16;
    Some(Color::Rgb(
        LEVELS[usize::from(cube / 36)],
        LEVELS[usize::from((cube / 6) % 6)],
        LEVELS[usize::from(cube % 6)],
    ))
}

/// The style keys that draw a decoration rather than text.
///
/// WCAG asks 4.5:1 of text and 3:1 of a user-interface component. A table's
/// border and a horizontal rule are components; everything else here carries
/// something a reader has to read.
pub const DECORATION_KEYS: &[&str] = &[
    "border",
    "scrollbar",
    "table_border",
    "rule",
    "blockquote_bar",
];

/// The contrast ratio between two colours, per WCAG 2.
///
/// Only meaningful for `Color::Rgb`; a named or indexed colour is whatever the
/// terminal's palette says it is, which perga cannot know.
pub fn contrast_ratio(a: Color, b: Color) -> Option<f64> {
    let luminance = |color: Color| -> Option<f64> {
        // An indexed colour above 15 has a fixed value, so it can be measured
        // too; that is what covers a theme after `degrade_to_256`.
        let color = match color {
            Color::Indexed(i) => ansi256_rgb(i)?,
            other => other,
        };

        let Color::Rgb(r, g, b) = color else {
            return None;
        };

        let channel = |v: u8| {
            let v = f64::from(v) / 255.0;
            if v <= 0.03928 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };

        Some(0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b))
    };

    let (a, b) = (luminance(a)?, luminance(b)?);
    let (hi, lo) = if a > b { (a, b) } else { (b, a) };

    Some((hi + 0.05) / (lo + 0.05))
}

/// The nearest ANSI-256 colour to a truecolour value.
///
/// Searched over the whole palette — the 6x6x6 cube *and* the 24-step grey
/// ramp — rather than snapping each channel to the cube. Snapping is what a
/// first version did, and it is wrong in a way that shows: `#313244`, the dark
/// theme's selection background, is 19 apart across its channels, fails a
/// "is this grey?" threshold, and lands on `(95, 95, 95)` — twice as light as
/// it should be, which then eats the contrast of every foreground drawn on it.
/// The grey ramp had an entry 1 away.
///
/// 240 candidates, compared once per colour when a theme loads. Only
/// `Color::Rgb` is touched; a named or indexed colour already means something
/// on a 256-colour terminal.
fn nearest_256(color: Color) -> Color {
    let Color::Rgb(r, g, b) = color else {
        return color;
    };

    let distance = |c: Color| match c {
        // Squared euclidean distance in RGB. Not perceptually uniform, but
        // the palette is coarse enough that a better metric changes nothing
        // a reader would notice.
        Color::Rgb(cr, cg, cb) => {
            let d = |a: u8, b: u8| i32::from(a).abs_diff(i32::from(b)) as i32;
            d(r, cr).pow(2) + d(g, cg).pow(2) + d(b, cb).pow(2)
        }
        _ => i32::MAX,
    };

    (16u8..=255)
        .filter_map(|index| ansi256_rgb(index).map(|c| (index, distance(c))))
        .min_by_key(|(_, d)| *d)
        .map_or(color, |(index, _)| Color::Indexed(index))
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

    /// Section 11.2 asks the `light` theme for 4.5:1 on body text. The same
    /// standard belongs on every theme and on every key: the first person to
    /// run the released binary could read the documents and not the interface
    /// around them, because thirteen keys of the default theme sat at 3.36:1.
    ///
    /// 4.5:1 for anything that carries text, 3:1 for a border or a rule, which
    /// is what WCAG asks of a user-interface component.
    ///
    /// `high-contrast` is not checked: its colours are the sixteen ANSI names,
    /// whose actual values are whatever the reader configured in their
    /// terminal, and perga cannot know them.
    #[test]
    fn the_truecolour_themes_are_readable() {
        for name in ["dark", "light"] {
            for (how, theme) in variants(name) {
                let theme = &theme;
                let page = theme.ui.background.bg.expect("a theme paints its ground");

                for (key, style) in theme.named_styles() {
                    let Some(fg) = style.fg else { continue };
                    // A key with its own background is read against that.
                    let surface = style.bg.unwrap_or(page);

                    let Some(ratio) = contrast_ratio(fg, surface) else {
                        continue;
                    };

                    let wanted = if DECORATION_KEYS.contains(&key) {
                        3.0
                    } else {
                        4.5
                    };

                    assert!(
                        ratio >= wanted,
                        "`{name}` theme{how}: `{key}` is {ratio:.2}:1 against its \
                     background, needs {wanted}:1"
                    );
                }
            }
        }
    }

    /// A theme as it is, and as a terminal without truecolour receives it.
    ///
    /// The second matters as much as the first: `COLORTERM` is unset on plenty
    /// of terminals, and the degraded palette is what those readers see. It is
    /// also where `code_inline` was still failing after the first pass — 8.23:1
    /// as written, 3.76:1 once snapped to the colour cube.
    fn variants(name: &str) -> Vec<(&'static str, Theme)> {
        let theme = Theme::builtin(name).expect("a built-in theme");
        let mut degraded = theme.clone();
        degraded.degrade_to_256();

        vec![("", theme), (" degraded to ANSI-256", degraded)]
    }

    /// Text drawn on a selected row keeps its own foreground, so the selection
    /// background is a surface too.
    #[test]
    fn text_stays_readable_on_a_selected_row() {
        for name in ["dark", "light"] {
            for (how, theme) in variants(name) {
                let Some(selection) = theme.ui.selection.bg else {
                    continue;
                };

                for (key, style) in theme.sidebar.named() {
                    let Some(fg) = style.fg else { continue };
                    // These two paint their own background over the selection.
                    if key == "mode_active" || style.bg.is_some() {
                        continue;
                    }

                    let Some(ratio) = contrast_ratio(fg, selection) else {
                        continue;
                    };

                    assert!(
                        ratio >= 4.5,
                        "`{name}` theme{how}: `{key}` is {ratio:.2}:1 on a \
                     selected row"
                    );
                }
            }
        }
    }

    #[test]
    fn the_contrast_ratio_matches_the_wcag_definition() {
        let white = Color::Rgb(0xff, 0xff, 0xff);
        let black = Color::Rgb(0, 0, 0);

        // The two extremes of the scale, exactly.
        assert!((contrast_ratio(white, black).unwrap() - 21.0).abs() < 0.001);
        assert!((contrast_ratio(white, white).unwrap() - 1.0).abs() < 0.001);
        // Order does not matter.
        assert_eq!(contrast_ratio(white, black), contrast_ratio(black, white));
        // A colour the terminal owns cannot be measured; an indexed one above
        // 15 has a fixed value and can be.
        assert_eq!(contrast_ratio(Color::Red, black), None);
        assert_eq!(contrast_ratio(Color::Indexed(7), black), None);
        assert!(contrast_ratio(Color::Indexed(231), black).unwrap() > 20.0);
    }

    #[test]
    fn the_ansi_256_palette_is_the_standard_one() {
        assert_eq!(ansi256_rgb(15), None, "0-15 belong to the terminal");
        assert_eq!(ansi256_rgb(16), Some(Color::Rgb(0, 0, 0)));
        assert_eq!(ansi256_rgb(231), Some(Color::Rgb(255, 255, 255)));
        assert_eq!(ansi256_rgb(196), Some(Color::Rgb(255, 0, 0)));
        assert_eq!(ansi256_rgb(232), Some(Color::Rgb(8, 8, 8)));
        assert_eq!(ansi256_rgb(255), Some(Color::Rgb(238, 238, 238)));
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
