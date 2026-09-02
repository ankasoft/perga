//! Layout constraint computation and responsive breakpoints.
//!
//! All geometry decisions live here so that the widgets never have to reason
//! about terminal size, and so the breakpoints are testable without rendering.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// The narrowest terminal perga will draw a normal frame in.
pub const MIN_WIDTH: u16 = 40;
/// The shortest terminal perga will draw a normal frame in.
pub const MIN_HEIGHT: u16 = 10;
/// At or below this height the tab bar is hidden and the status bar collapses.
pub const SHORT_HEIGHT: u16 = 20;

/// The narrowest a sidebar may be resized to.
pub const MIN_SIDEBAR_WIDTH: u16 = 12;
/// The widest a sidebar may be resized to, as a fraction of the terminal.
pub const MAX_SIDEBAR_FRACTION: u16 = 2;

/// How the sidebar is placed for the current terminal width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SidebarPlacement {
    /// Not drawn.
    Hidden,
    /// Splitting the main area with the viewport.
    Split,
    /// Drawn over the viewport, because the terminal is too narrow to split.
    Overlaid,
}

/// The computed geometry of one frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frames {
    /// The terminal is below the minimum supported size; draw only a message.
    pub too_small: bool,
    /// The title bar.
    pub title: Rect,
    /// The tab bar, when it is shown.
    pub tabs: Option<Rect>,
    /// The sidebar, when it is shown.
    pub sidebar: Option<Rect>,
    /// How the sidebar is placed.
    pub sidebar_placement: SidebarPlacement,
    /// The document viewport. Always present.
    pub viewport: Rect,
    /// The status bar, when it is shown.
    pub status: Option<Rect>,
    /// The status bar is reduced to its shortest form.
    pub status_collapsed: bool,
}

/// The inputs layout needs from application state.
#[derive(Debug, Clone, Copy)]
pub struct LayoutInput {
    /// The full terminal area.
    pub area: Rect,
    /// Whether the user has the sidebar switched on.
    pub sidebar_visible: bool,
    /// The sidebar width the user has chosen.
    pub sidebar_width: u16,
    /// Below this terminal width the sidebar stops splitting the main area.
    pub narrow_threshold: u16,
    /// How many tabs are open.
    pub tab_count: usize,
    /// Show the tab bar even with one tab open.
    pub always_show_tabs: bool,
    /// Whether the status bar is switched on.
    pub show_status_bar: bool,
}

/// Compute the geometry for one frame.
pub fn compute(input: LayoutInput) -> Frames {
    let area = input.area;

    if area.width < MIN_WIDTH || area.height < MIN_HEIGHT {
        return Frames {
            too_small: true,
            title: area,
            tabs: None,
            sidebar: None,
            sidebar_placement: SidebarPlacement::Hidden,
            viewport: area,
            status: None,
            status_collapsed: true,
        };
    }

    let short = area.height < SHORT_HEIGHT;
    let show_tabs = !short && (input.tab_count > 1 || input.always_show_tabs);
    let show_status = input.show_status_bar;

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(u16::from(show_tabs)),
            Constraint::Min(3),
            Constraint::Length(u16::from(show_status)),
        ])
        .split(area);

    let title = rows[0];
    let tabs = show_tabs.then_some(rows[1]);
    let main = rows[2];
    let status = show_status.then_some(rows[3]);

    let (sidebar, viewport, placement) = place_sidebar(main, &input);

    Frames {
        too_small: false,
        title,
        tabs,
        sidebar,
        sidebar_placement: placement,
        viewport,
        status,
        status_collapsed: short,
    }
}

/// Split the main area between the sidebar and the viewport.
///
/// Below `narrow_threshold` columns the sidebar stops splitting: there is not
/// enough room for two panes, so it is drawn over the viewport instead. It is
/// still recoverable with `Ctrl+B`, and it dismisses itself once it opens a
/// document.
fn place_sidebar(main: Rect, input: &LayoutInput) -> (Option<Rect>, Rect, SidebarPlacement) {
    if !input.sidebar_visible {
        return (None, main, SidebarPlacement::Hidden);
    }

    let width = clamp_sidebar_width(input.sidebar_width, main.width);

    if main.width < input.narrow_threshold {
        let overlay = Rect {
            width: width.min(main.width),
            ..main
        };
        return (Some(overlay), main, SidebarPlacement::Overlaid);
    }

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(width), Constraint::Min(MIN_WIDTH / 2)])
        .split(main);

    (Some(columns[0]), columns[1], SidebarPlacement::Split)
}

/// Keep a sidebar width usable: wide enough to read a filename, and never more
/// than half the terminal.
pub fn clamp_sidebar_width(requested: u16, available: u16) -> u16 {
    let max = (available / MAX_SIDEBAR_FRACTION).max(MIN_SIDEBAR_WIDTH);
    requested.clamp(MIN_SIDEBAR_WIDTH, max)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(width: u16, height: u16) -> LayoutInput {
        LayoutInput {
            area: Rect::new(0, 0, width, height),
            sidebar_visible: true,
            sidebar_width: 32,
            narrow_threshold: 80,
            tab_count: 1,
            always_show_tabs: false,
            show_status_bar: true,
        }
    }

    #[test]
    fn a_normal_terminal_gets_every_pane() {
        let frames = compute(input(120, 40));
        assert!(!frames.too_small);
        assert_eq!(frames.sidebar_placement, SidebarPlacement::Split);
        assert_eq!(frames.sidebar.unwrap().width, 32);
        assert!(frames.status.is_some());
        assert!(!frames.status_collapsed);
        // One tab and `always_show_tabs = false`: no tab bar.
        assert!(frames.tabs.is_none());
    }

    #[test]
    fn a_second_tab_brings_the_tab_bar() {
        let mut input = input(120, 40);
        input.tab_count = 2;
        assert!(compute(input).tabs.is_some());
    }

    #[test]
    fn always_show_tabs_brings_it_with_one_tab() {
        let mut input = input(120, 40);
        input.always_show_tabs = true;
        assert!(compute(input).tabs.is_some());
    }

    #[test]
    fn a_narrow_terminal_overlays_the_sidebar() {
        let frames = compute(input(70, 40));
        assert_eq!(frames.sidebar_placement, SidebarPlacement::Overlaid);
        // The viewport keeps the whole main area; the sidebar is drawn on top.
        assert_eq!(frames.viewport.width, 70);
    }

    #[test]
    fn a_short_terminal_hides_the_tab_bar_and_collapses_the_status_bar() {
        let mut input = input(120, 18);
        input.tab_count = 3;
        let frames = compute(input);
        assert!(frames.tabs.is_none());
        assert!(frames.status_collapsed);
        assert!(frames.status.is_some());
    }

    #[test]
    fn a_tiny_terminal_is_reported_as_too_small() {
        assert!(compute(input(39, 30)).too_small);
        assert!(compute(input(80, 9)).too_small);
        assert!(!compute(input(40, 10)).too_small);
    }

    #[test]
    fn no_pane_ever_escapes_the_terminal() {
        for width in [40u16, 41, 60, 79, 80, 81, 120, 200] {
            for height in [10u16, 11, 19, 20, 40, 60] {
                let frames = compute(input(width, height));
                let area = Rect::new(0, 0, width, height);
                for rect in [
                    Some(frames.title),
                    frames.tabs,
                    frames.sidebar,
                    frames.status,
                ]
                .into_iter()
                .flatten()
                .chain([frames.viewport])
                {
                    assert!(
                        rect.right() <= area.right() && rect.bottom() <= area.bottom(),
                        "{rect:?} escapes {width}x{height}"
                    );
                }
            }
        }
    }

    #[test]
    fn sidebar_width_is_clamped_to_something_usable() {
        assert_eq!(clamp_sidebar_width(32, 120), 32);
        assert_eq!(clamp_sidebar_width(2, 120), MIN_SIDEBAR_WIDTH);
        // Never more than half the terminal.
        assert_eq!(clamp_sidebar_width(100, 60), 30);
        // ...unless half the terminal is narrower than the minimum, in which
        // case the minimum wins and the layout clips it.
        assert_eq!(clamp_sidebar_width(100, 20), MIN_SIDEBAR_WIDTH);
    }
}
