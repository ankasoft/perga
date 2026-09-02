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
