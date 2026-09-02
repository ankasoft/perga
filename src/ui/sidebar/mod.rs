//! The sidebar and its four modes.

pub mod backlinks;
pub mod files;
pub mod outline;
pub mod search;

use std::fmt;

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, Paragraph, Widget};

use crate::app::{App, Focus};

/// Which of the four sidebar modes is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SidebarMode {
    /// The hierarchical vault tree.
    #[default]
    Files,
    /// Results of the last project-wide search.
    Search,
    /// Headings of the active document.
    Outline,
    /// Outgoing links and backlinks for the active document.
    Links,
}

impl SidebarMode {
    /// The four modes in the order they appear in the mode row.
    pub const ALL: [SidebarMode; 4] = [
        SidebarMode::Files,
        SidebarMode::Search,
        SidebarMode::Outline,
        SidebarMode::Links,
    ];

    /// The label shown in the sidebar mode row.
    pub fn label(self) -> &'static str {
        match self {
            SidebarMode::Files => "files",
            SidebarMode::Search => "search",
            SidebarMode::Outline => "outline",
            SidebarMode::Links => "links",
        }
    }
}

impl fmt::Display for SidebarMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

/// The sidebar pane.
pub struct SidebarPane<'a> {
    app: &'a App,
    /// Drawn over the viewport rather than beside it.
    overlaid: bool,
}

impl<'a> SidebarPane<'a> {
    /// Build the sidebar for the current state.
    pub fn new(app: &'a App, overlaid: bool) -> Self {
        SidebarPane { app, overlaid }
    }

    /// The mode row, in which the active mode is uppercase.
    ///
    /// This is the only cue the user has for which mode they are in, so it is
    /// styled as well as cased.
    pub fn mode_row(&self) -> Line<'static> {
        let theme = &self.app.theme;
        let mut spans = Vec::new();

        for mode in SidebarMode::ALL {
            let active = mode == self.app.sidebar.mode;
            let label = if active {
                mode.label().to_uppercase()
            } else {
                mode.label().to_string()
            };
            let style = if active {
                theme.sidebar.mode_active
            } else {
                theme.sidebar.mode_inactive
            };
            spans.push(Span::styled(label, style));
            spans.push(Span::styled(" ", theme.ui.status_bar));
        }

        Line::from(spans)
    }
}

impl Widget for SidebarPane<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let theme = &self.app.theme;
        let focused = self.app.focus == Focus::Sidebar;

        let block = Block::bordered()
            .border_style(if focused {
                theme.ui.border_focused
            } else {
                theme.ui.border
            })
            .style(theme.ui.background);
        let inner = block.inner(area);

        if self.overlaid {
            // Drawn on top of the viewport, so whatever is underneath has to go
            // first or it shows through the gaps.
            Clear.render(area, buf);
        }
        block.render(area, buf);

        // The mode row wraps to a second line when the sidebar is narrow.
        Paragraph::new(self.mode_row())
            .wrap(ratatui::widgets::Wrap { trim: true })
            .render(inner, buf);
    }
}
