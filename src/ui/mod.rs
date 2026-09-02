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
use ratatui::widgets::{Block, Clear, Paragraph, Widget};
use ratatui::Frame;

use crate::app::{App, Overlay};
use crate::ui::hints::Hints;
use crate::ui::layout::{SidebarPlacement, MIN_HEIGHT, MIN_WIDTH};
use crate::ui::overlay::find::FindBar;
use crate::ui::overlay::help::Help;
use crate::ui::overlay::prompt::PromptLine;
use crate::ui::overlay::switcher::Picker;
use crate::ui::sidebar::SidebarPane;
use crate::ui::statusbar::StatusBar;
use crate::ui::tabs::TabBar;
use crate::ui::viewport::Viewport;

/// How much of the frame an overlay takes, as a percentage of each dimension.
const OVERLAY_WIDTH_PERCENT: u16 = 80;
const OVERLAY_HEIGHT_PERCENT: u16 = 80;

/// Measure whatever the next frame needs, then draw it.
///
/// Measuring mutates the layout cache, so it happens here rather than inside
/// `render`, which stays a pure function of state.
pub fn draw(app: &mut App, frame: &mut Frame) {
    let (lines, total) = viewport::measure(app);
    render_with(app, frame, &lines, total);
}

/// Draw one frame from state alone.
pub fn render(app: &App, frame: &mut Frame) {
    render_with(app, frame, &[], None);
}

fn render_with(app: &App, frame: &mut Frame, lines: &[Line<'static>], total: Option<usize>) {
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

    Viewport::new(app, lines, total).render(frames.viewport, buf);

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
        // The find bar sits along the bottom of the viewport: it is the
        // matches behind it the reader is looking at.
        Some(Overlay::Find) => {
            if let Some(find) = &app.tab().find {
                FindBar::new(find, &app.theme).render(inner(frames.viewport), buf);
            }
        }
        // A prompt is one line, centred, rather than a panel: it holds one
        // line of text and a box around it would be mostly empty.
        Some(Overlay::Prompt { kind, input }) => {
            let area = prompt_area(area);
            Clear.render(area, buf);

            let block = Block::bordered()
                .border_style(app.theme.ui.border_focused)
                .style(app.theme.ui.background);
            let inner = block.inner(area);
            block.render(area, buf);

            PromptLine::new(
                input,
                kind.prefix(),
                app.theme.ui.status_bar,
                app.theme.ui.selection,
            )
            .render(inner, buf);
        }
        Some(Overlay::Switcher {
            input,
            rows,
            selected,
        }) => {
            let listed = rows
                .iter()
                .map(|row| switcher_row(app, row))
                .collect::<Vec<_>>();

            let title = if input.is_empty() {
                "Recent".to_string()
            } else {
                format!("Open: {}", input.value())
            };

            Picker::new(
                &app.theme,
                title,
                " Enter open   Ctrl+Enter new tab   Esc cancel ",
                listed,
                *selected,
            )
            .render(centred(area), buf);
        }
        Some(Overlay::Disambiguate {
            page,
            candidates,
            selected,
            ..
        }) => {
            let rows = candidates
                .iter()
                .map(|path| {
                    Line::from(Span::styled(
                        format!(" {}", path.display()),
                        app.theme.sidebar.file,
                    ))
                })
                .collect();

            Picker::new(
                &app.theme,
                format!("[[{page}]] matches {} pages", candidates.len()),
                " Enter open   Esc cancel ",
                rows,
                *selected,
            )
            .render(centred(area), buf);
        }
        // Hints are drawn over the document rather than in a panel: a label
        // only means anything on top of the link it belongs to.
        Some(Overlay::Hints { links, typed }) => {
            Hints::new(app, lines, links, typed).render(inner(frames.viewport), buf);
        }
        None => {}
    }
}

/// The title bar: the application name, the document path relative to the vault
/// root, and the scroll position.
fn render_title(app: &App, area: Rect, buf: &mut ratatui::buffer::Buffer) {
    let theme = &app.theme;

    let left = Line::from(vec![
        Span::styled(" perga ", theme.ui.title),
        Span::styled(app.title_path().unwrap_or_default(), theme.ui.logo_subtitle),
    ]);

    Paragraph::new(left)
        .style(theme.ui.status_bar)
        .render(area, buf);

    // The scroll position is only shown once the document has been measured to
    // the end; a total that jumps as it is discovered is worse than no total.
    if let Some((current, total)) = app.scroll_position() {
        Paragraph::new(Line::from(Span::styled(
            format!("{current}/{total} "),
            theme.ui.logo_subtitle,
        )))
        .alignment(Alignment::Right)
        .style(theme.ui.status_bar)
        .render(area, buf);
    }
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

/// One quick-switcher row, with the matched characters picked out.
fn switcher_row(app: &App, row: &crate::app::SwitcherRow) -> Line<'static> {
    let theme = &app.theme;
    let display = row.path.display().to_string();

    if row.create {
        return Line::from(vec![
            Span::styled(" + ", theme.ui.status_warning),
            Span::styled(format!("create \"{display}\""), theme.sidebar.file_other),
        ]);
    }

    let mut spans = vec![Span::styled(" ", theme.sidebar.file)];

    for (at, c) in display.chars().enumerate() {
        let matched = row.indices.contains(&(at as u32));
        spans.push(Span::styled(
            c.to_string(),
            if matched {
                theme.sidebar.r#match
            } else {
                theme.sidebar.file
            },
        ));
    }

    Line::from(spans)
}

/// Where a one-line prompt is drawn: centred, and no taller than it needs.
fn prompt_area(area: Rect) -> Rect {
    let width = (area.width * OVERLAY_WIDTH_PERCENT / 100).max(1);
    Rect {
        x: area.x + (area.width - width) / 2,
        y: area.y + area.height / 3,
        width,
        height: 3.min(area.height),
    }
}

/// The content area inside a bordered pane.
fn inner(area: Rect) -> Rect {
    Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(2),
    }
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
