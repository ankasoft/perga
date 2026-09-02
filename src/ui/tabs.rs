//! The tab bar.
//!
//! Hidden entirely when only one tab is open and `ui.always_show_tabs` is off,
//! which is the default: a row of vertical space is the scarcest resource in a
//! terminal.

use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use unicode_width::UnicodeWidthStr;

use crate::app::App;

/// Shown at either end when there are tabs the bar could not fit.
const MORE: &str = "…";

/// The tab bar.
pub struct TabBar<'a> {
    app: &'a App,
}

impl<'a> TabBar<'a> {
    /// Build the tab bar for the current state.
    pub fn new(app: &'a App) -> Self {
        TabBar { app }
    }

    /// The line the tab bar draws, in `width` columns.
    pub fn line(&self, width: u16) -> Line<'static> {
        let theme = &self.app.theme;
        let labels: Vec<String> = self
            .app
            .tabs
            .iter()
            .map(|tab| format!(" {} ", tab.display_label()))
            .collect();

        let shown = visible_range(&labels, self.app.active_tab, usize::from(width));
        let mut spans = Vec::new();

        if shown.start > 0 {
            spans.push(Span::styled(MORE.to_string(), theme.tabs.inactive));
        }

        for index in shown.clone() {
            let tab = &self.app.tabs[index];
            let style = if index == self.app.active_tab {
                theme.tabs.active
            } else if tab.dirty {
                theme.tabs.dirty
            } else {
                theme.tabs.inactive
            };

            spans.push(Span::styled(labels[index].clone(), style));
            spans.push(Span::styled("│", theme.ui.border));
        }

        if shown.end < labels.len() {
            spans.push(Span::styled(MORE.to_string(), theme.tabs.inactive));
        } else {
            // The `+` only means anything when it is at the end of the list.
            spans.push(Span::styled(" + ", theme.tabs.inactive));
        }

        Line::from(spans)
    }
}

impl Widget for TabBar<'_> {
    fn render(self, area: Rect, buf: &mut ratatui::buffer::Buffer) {
        Paragraph::new(self.line(area.width)).render(area, buf);
    }
}

/// Which tabs fit, keeping the active one on screen.
///
/// The window grows outwards from the active tab rather than starting at the
/// first: with twenty tabs open, the one being read must always be visible, and
/// scrolling the bar back to find it is not something a reader should have to
/// do.
fn visible_range(labels: &[String], active: usize, width: usize) -> std::ops::Range<usize> {
    if labels.is_empty() {
        return 0..0;
    }

    // Every label is followed by a separator, and either end may need an
    // ellipsis; reserving for both is a column too cautious and never a column
    // short.
    let budget = width.saturating_sub(MORE.width() * 2);
    let cost = |index: usize| labels[index].width() + 1;

    let active = active.min(labels.len() - 1);
    let mut used = cost(active);
    let (mut start, mut end) = (active, active + 1);

    // Alternate outwards so the active tab stays roughly centred rather than
    // jumping to an edge.
    loop {
        let can_grow_right = end < labels.len() && used + cost(end) <= budget;
        let can_grow_left = start > 0 && used + cost(start - 1) <= budget;

        if can_grow_right {
            used += cost(end);
            end += 1;
        } else if can_grow_left {
            used += cost(start - 1);
            start -= 1;
        } else {
            break;
        }
    }

    start..end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(count: usize) -> Vec<String> {
        (0..count).map(|i| format!(" tab-{i:02} ")).collect()
    }

    #[test]
    fn everything_fits_on_a_wide_bar() {
        assert_eq!(visible_range(&labels(3), 0, 120), 0..3);
    }

    #[test]
    fn a_narrow_bar_keeps_the_active_tab_in_view() {
        let labels = labels(20);

        let far_right = visible_range(&labels, 19, 40);
        assert!(far_right.contains(&19), "{far_right:?}");
        assert!(far_right.start > 0, "the left-hand tabs were dropped");

        let far_left = visible_range(&labels, 0, 40);
        assert!(far_left.contains(&0), "{far_left:?}");
        assert_eq!(far_left.start, 0);
    }

    #[test]
    fn the_active_tab_survives_a_bar_with_room_for_one() {
        let range = visible_range(&labels(20), 7, 12);
        assert_eq!(range, 7..8);
    }

    #[test]
    fn no_tabs_is_an_empty_range() {
        assert_eq!(visible_range(&[], 0, 80), 0..0);
    }
}
