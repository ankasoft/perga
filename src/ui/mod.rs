//! Frame composition. Rendering is a pure function of application state.
//!
//! Nothing in this module or below it mutates the application: every widget
//! takes `&App`, and every state change goes through `App::update`.

pub mod hints;
pub mod layout;
pub mod overlay;
pub mod sidebar;
pub mod statusbar;
pub mod tabs;
pub mod viewport;
pub mod welcome;

use ratatui::layout::{Alignment, Rect};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Widget};
use ratatui::Frame;

use crate::app::{App, Overlay};
use crate::ui::layout::{SidebarPlacement, MIN_HEIGHT, MIN_WIDTH};
use crate::ui::overlay::help::Help;
use crate::ui::sidebar::SidebarPane;
use crate::ui::statusbar::StatusBar;
use crate::ui::tabs::TabBar;
use crate::ui::viewport::Viewport;

/// How much of the frame an overlay takes, as a percentage of each dimension.
const OVERLAY_WIDTH_PERCENT: u16 = 80;
const OVERLAY_HEIGHT_PERCENT: u16 = 80;

/// Draw one frame.
pub fn render(app: &App, frame: &mut Frame) {
    let area = frame.area();
    let buf = frame.buffer_mut();
    let frames = app.frames();

    // Paint the theme's background first so a terminal with a different
    // background does not show through the gaps between panes.
    Paragraph::new("")
        .style(app.theme.ui.background)
        .render(area, buf);

    if frames.too_small {
        render_too_small(app, area, buf);
        return;
    }

    render_title(app, frames.title, buf);

    if let Some(tabs) = frames.tabs {
        TabBar::new(app).render(tabs, buf);
    }

    Viewport::new(app).render(frames.viewport, buf);

    if let Some(sidebar) = frames.sidebar {
        let overlaid = frames.sidebar_placement == SidebarPlacement::Overlaid;
        SidebarPane::new(app, overlaid).render(sidebar, buf);
    }

    if let Some(status) = frames.status {
        StatusBar::new(app, frames.status_collapsed).render(status, buf);
    }

    match &app.overlay {
        Some(Overlay::Help { scroll }) => {
            Help::new(&app.keymap, &app.theme, *scroll).render(centred(area), buf);
        }
        None => {}
    }
}

/// The title bar: the application name, the document path relative to the vault
/// root, and the scroll position.
fn render_title(app: &App, area: Rect, buf: &mut ratatui::buffer::Buffer) {
    let theme = &app.theme;

    Paragraph::new(Line::from(vec![
        Span::styled(" perga ", theme.ui.title),
        Span::styled("", theme.ui.status_bar),
    ]))
    .style(theme.ui.status_bar)
    .render(area, buf);
}

/// Below the minimum supported size there is no honest way to draw a frame, so
/// say so rather than clipping panes into nonsense.
fn render_too_small(app: &App, area: Rect, buf: &mut ratatui::buffer::Buffer) {
    let text = vec![
        Line::styled("terminal too small", app.theme.ui.status_error),
        Line::styled(
            format!("need {MIN_WIDTH}x{MIN_HEIGHT}"),
            app.theme.ui.logo_subtitle,
        ),
    ];

    let top = area.height.saturating_sub(text.len() as u16) / 2;
    let block = Rect {
        y: area.y + top,
        height: (text.len() as u16).min(area.height),
        ..area
    };

    Paragraph::new(text)
        .alignment(Alignment::Center)
        .render(block, buf);
}

/// Centre an overlay in the frame.
fn centred(area: Rect) -> Rect {
    let width = (area.width * OVERLAY_WIDTH_PERCENT / 100).max(1);
    let height = (area.height * OVERLAY_HEIGHT_PERCENT / 100).max(1);

    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + (area.height - height) / 2,
        width,
        height,
    }
}
