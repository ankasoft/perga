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
use crate::ui::sidebar::backlinks::LinksMode;
use crate::ui::sidebar::files::FilesMode;
use crate::ui::sidebar::outline::OutlineMode;

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

/// How many lines a run of `width` columns takes when wrapped into `available`.
fn wrapped_height(width: usize, available: u16) -> u16 {
    if available == 0 {
        return 0;
    }
    let available = usize::from(available);
    width.div_ceil(available).max(1) as u16
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

    /// The mode header: what the mode is showing, and how much of it.
    ///
    /// The files mode reports progress here, so a vault still being walked
    /// says so rather than looking like a vault that is simply small.
    pub fn header(&self) -> Line<'static> {
        let theme = &self.app.theme;
        let tree = &self.app.vault.tree;

        let text = match self.app.sidebar.mode {
            SidebarMode::Files if tree.complete => format!("{} entries", tree.entries),
            SidebarMode::Files => format!("{} entries, scanning…", tree.entries),
            SidebarMode::Outline => {
                let count = self.app.tab().doc.as_ref().map_or(0, |d| d.outline.len());
                format!("{count} headings")
            }
            SidebarMode::Links => {
                let count = self.app.tab().doc.as_ref().map_or(0, |d| d.links.len());
                format!("{count} outgoing")
            }
            other => other.label().to_string(),
        };

        Line::styled(text, theme.sidebar.file_other)
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

        if inner.height == 0 {
            return;
        }

        // The mode row wraps to a second line when the sidebar is narrow, so
        // the rows below it start wherever it actually ended.
        let mode_row = self.mode_row();
        let mode_height = wrapped_height(mode_row.width(), inner.width).min(inner.height);
        Paragraph::new(mode_row)
            .wrap(ratatui::widgets::Wrap { trim: true })
            .render(
                Rect {
                    height: mode_height,
                    ..inner
                },
                buf,
            );

        let below = Rect {
            y: inner.y + mode_height,
            height: inner.height.saturating_sub(mode_height),
            ..inner
        };
        if below.height == 0 {
            return;
        }

        Paragraph::new(self.header()).render(Rect { height: 1, ..below }, buf);

        let content = Rect {
            y: below.y + 1,
            height: below.height.saturating_sub(1),
            ..below
        };

        match self.app.sidebar.mode {
            SidebarMode::Files => FilesMode::new(self.app).render(content, buf),
            SidebarMode::Outline => OutlineMode::new(self.app).render(content, buf),
            SidebarMode::Links => LinksMode::new(self.app).render(content, buf),
            // The search mode arrives with the feature behind it.
            SidebarMode::Search => {}
        }
    }
}
