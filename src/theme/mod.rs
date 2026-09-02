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

    /// Apply `f` to every style in the theme.
    fn map_styles(&mut self, mut f: impl FnMut(Style) -> Style) {
        self.ui.map_styles(&mut f);
        self.tabs.map_styles(&mut f);
        self.sidebar.map_styles(&mut f);
        self.markdown.map_styles(&mut f);
        self.hints.map_styles(&mut f);
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
