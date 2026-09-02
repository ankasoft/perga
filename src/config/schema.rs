//! Serde structures and built-in defaults for every configuration key.
//!
//! The full five-layer precedence chain is assembled in [`super`]. This module
//! only describes the shape of the configuration and what each key defaults to.

use serde::Deserialize;

use crate::ui::sidebar::SidebarMode;

/// Presentation and chrome. The `[ui]` table.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct UiConfig {
    /// Whether the sidebar starts visible.
    pub sidebar_visible: bool,
    /// The sidebar's width in columns.
    pub sidebar_width: u16,
    /// Which sidebar mode is showing on startup.
    pub sidebar_default_mode: SidebarMode,
    /// Show the tab bar even with a single tab open.
    pub always_show_tabs: bool,
    /// Show line numbers in the viewport.
    pub show_line_numbers: bool,
    /// Show the status bar.
    pub show_status_bar: bool,
    /// Lines scrolled per mouse wheel notch.
    pub mouse_scroll_lines: u16,
    /// Capture the mouse. Off leaves terminal text selection available.
    pub mouse: bool,
    /// Auto-hide the sidebar below this terminal width.
    pub narrow_threshold: u16,
}

impl Default for UiConfig {
    fn default() -> Self {
        UiConfig {
            sidebar_visible: true,
            sidebar_width: 32,
            sidebar_default_mode: SidebarMode::Files,
            always_show_tabs: false,
            show_line_numbers: false,
            show_status_bar: true,
            mouse_scroll_lines: 3,
            mouse: true,
            narrow_threshold: 80,
        }
    }
}

/// How the files sidebar orders the entries in a directory.
///
/// Directories always come before files whatever this says; the key only
/// orders entries of the same kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortKey {
    /// Alphabetically, case-insensitively.
    #[default]
    Name,
    /// Most recently modified first.
    Mtime,
    /// Largest first.
    Size,
}

/// The vault tree. The `[files]` table.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FilesConfig {
    /// Include dotted directories such as `.github` and `.claude`.
    ///
    /// On by default, deliberately: a dotted directory in a notes vault holds
    /// notes, and hiding it by default with no visible override is the
    /// behaviour this project exists to avoid.
    pub include_hidden: bool,
    /// Honour `.gitignore`, `.ignore`, and the global gitignore.
    pub respect_gitignore: bool,
    /// Show files that are not Markdown.
    pub show_all: bool,
    /// How entries within a directory are ordered.
    pub sort: SortKey,
    /// Reverse whatever order `sort` produces.
    pub sort_reverse: bool,
    /// Extensions treated as Markdown, without the leading dot.
    pub extensions: Vec<String>,
}

impl Default for FilesConfig {
    fn default() -> Self {
        FilesConfig {
            include_hidden: true,
            respect_gitignore: true,
            show_all: false,
            sort: SortKey::Name,
            sort_reverse: false,
            extensions: ["md", "markdown", "mdx"]
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
        }
    }
}

impl FilesConfig {
    /// Whether a path's extension marks it as Markdown.
    pub fn is_markdown(&self, path: &std::path::Path) -> bool {
        let Some(extension) = path.extension().and_then(|e| e.to_str()) else {
            return false;
        };
        self.extensions
            .iter()
            .any(|known| known.eq_ignore_ascii_case(extension))
    }
}

/// The order wiki-link targets are looked up in. The `[wikilinks]` table's
/// `resolution` key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WikiResolutionOrder {
    /// An exact relative path first, then a filename search.
    #[default]
    PathFirst,
    /// A filename search first, then an exact relative path.
    FilenameFirst,
}

/// Wiki-links and the backlink index. The `[wikilinks]` table.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WikiLinkConfig {
    /// Parse `[[Page]]` links and build the backlink index at all.
    pub enabled: bool,
    /// Extensions a page name is searched with, without the leading dot.
    pub extensions: Vec<String>,
    /// Which of the two resolution orders to use.
    pub resolution: WikiResolutionOrder,
    /// Build the index on startup rather than on first use.
    pub index_on_start: bool,
    /// Cache the index between runs.
    pub cache: bool,
    /// Where a file created from a broken wiki-link goes. Empty means the
    /// active document's own directory.
    pub new_file_dir: std::path::PathBuf,
}

impl Default for WikiLinkConfig {
    fn default() -> Self {
        WikiLinkConfig {
            enabled: true,
            extensions: ["md", "markdown"]
                .iter()
                .map(|s| (*s).to_string())
                .collect(),
            resolution: WikiResolutionOrder::default(),
            index_on_start: true,
            cache: true,
            new_file_dir: std::path::PathBuf::new(),
        }
    }
}

/// Vault-wide behaviour. The `[general]` table.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GeneralConfig {
    /// Directory opened when no `PATH` argument is given.
    pub start_path: std::path::PathBuf,
    /// Read `.perga.toml` from the vault root.
    pub allow_local_config: bool,
    /// Follow symlinks while walking the vault.
    ///
    /// Off by default: a vault with a symlink loop in it is not exotic, and a
    /// walker that follows links has to carry a device/inode set to survive
    /// one.
    pub follow_symlinks: bool,
    /// Hard wrap width; 0 fits the viewport.
    pub wrap: u16,
    /// Expand tabs in source to this many spaces when rendering.
    pub tab_width: u8,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        GeneralConfig {
            start_path: std::path::PathBuf::from("."),
            allow_local_config: true,
            follow_symlinks: false,
            wrap: 0,
            tab_width: 4,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_documented_configuration() {
        let ui = UiConfig::default();
        assert!(ui.sidebar_visible);
        assert_eq!(ui.sidebar_width, 32);
        assert_eq!(ui.sidebar_default_mode, SidebarMode::Files);
        assert!(!ui.always_show_tabs);
        assert_eq!(ui.narrow_threshold, 80);
        assert_eq!(ui.mouse_scroll_lines, 3);
    }

    #[test]
    fn a_partial_table_keeps_the_other_defaults() {
        let ui: UiConfig = toml::from_str("sidebar_width = 40").unwrap();
        assert_eq!(ui.sidebar_width, 40);
        assert!(ui.sidebar_visible);
    }

    #[test]
    fn sidebar_mode_parses_from_its_lowercase_name() {
        let ui: UiConfig = toml::from_str(r#"sidebar_default_mode = "outline""#).unwrap();
        assert_eq!(ui.sidebar_default_mode, SidebarMode::Outline);
    }
}
