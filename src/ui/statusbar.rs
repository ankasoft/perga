//! The status bar: mode indicator, context hints, and transient messages.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use unicode_width::UnicodeWidthStr;

use crate::action::Action;
use crate::app::{App, Severity, DIRTY_MARKER};

/// The actions the standing hint row advertises, in the order they are dropped
/// from the right as the terminal narrows.
///
/// The keys themselves are looked up in the keymap rather than written here, so
/// a remap moves the hint with it. The first entry of each pair is the action
/// whose key is shown; the second, when present, is shown after a slash.
const HINTS: &[(Action, Option<Action>, &str)] = &[
    (Action::HistoryBack, Some(Action::HistoryForward), "history"),
    (Action::HintMode, None, "links"),
    (Action::ToggleSidebar, None, "sidebar"),
    (Action::OpenQuickSwitcher, None, "switch"),
    (Action::OpenFindInDocument, None, "find"),
    (Action::ToggleHelp, None, "help"),
];

/// The status bar.
pub struct StatusBar<'a> {
    app: &'a App,
    /// Reduced to its shortest form, on a short terminal.
    collapsed: bool,
}

impl<'a> StatusBar<'a> {
    /// Build the status bar for the current state.
    pub fn new(app: &'a App, collapsed: bool) -> Self {
        StatusBar { app, collapsed }
    }

    /// The line the status bar draws, exposed so it can be asserted on without
    /// a backend.
    ///
    /// `width` is the room available. Hints are dropped from the right rather
    /// than clipped, so the bar never ends mid-key.
    pub fn line(&self, width: u16) -> Line<'static> {
        let theme = &self.app.theme;
        let mut spans = vec![
            Span::styled(
                format!(" {} ", self.app.tab().mode.label()),
                theme.ui.status_mode,
            ),
            Span::styled(" ", theme.ui.status_bar),
        ];

        // In edit mode the dirty marker and the cursor position replace the
        // hints: they are what the writer needs to see, and they change with
        // every keystroke.
        if let Some(editor) = &self.app.tab().editor {
            let (line, column) = editor.cursor();

            if self.app.tab().dirty {
                spans.push(Span::styled(format!("{DIRTY_MARKER} "), theme.tabs.dirty));
            }
            spans.push(Span::styled(
                format!("{}:{}  ", line + 1, column + 1),
                theme.ui.status_bar,
            ));
        }

        // Priority, highest first: a message the user needs to see, then the
        // sequence they are halfway through typing, then the standing hints.
        if let Some((text, severity)) = &self.app.status.message {
            let style = match severity {
                Severity::Info => theme.ui.status_bar,
                Severity::Warning => theme.ui.status_warning,
                Severity::Error => theme.ui.status_error,
            };
            spans.push(Span::styled(text.clone(), style));
        } else if let Some(pending) = &self.app.status.pending {
            spans.push(Span::styled(pending.clone(), theme.ui.status_warning));
        } else if !self.collapsed {
            let mut used: usize = spans.iter().map(|s| s.content.width()).sum();

            for (action, paired, what) in HINTS {
                let Some(mut key) = self.app.keymap.binding_for(action) else {
                    continue;
                };
                if let Some(second) = paired.as_ref().and_then(|a| self.app.keymap.binding_for(a)) {
                    key = format!("{key}/{second}");
                }

                let label = format!(" {what}  ");
                let cost = key.width() + label.width();
                if used + cost > width as usize {
                    break;
                }
                used += cost;

                spans.push(Span::styled(key, theme.ui.title));
                spans.push(Span::styled(label, theme.ui.status_bar));
            }
        }

        Line::from(spans)
    }
}

impl Widget for StatusBar<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        let style = self.app.theme.ui.status_bar;
        Paragraph::new(self.line(area.width))
            .style(style)
            .render(area, buf);
    }
}
