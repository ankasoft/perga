//! The `App` struct, the blocking event loop, and top-level state transitions.
//!
//! Everything here is deliberately terminal-free: `App` is constructed, fed
//! [`Action`]s, and asserted on without a backend. The loop that reads from the
//! terminal lives in [`run`], and it does nothing but translate messages into
//! actions and hand them to [`App::update`].

use crossbeam_channel::Receiver;
use ratatui::layout::Rect;

use crate::action::Action;
use crate::config::keymap::Keymap;
use crate::config::schema::UiConfig;
use crate::terminal::{self, Tui};
use crate::theme::Theme;
use crate::ui;
use crate::ui::layout::{self, SidebarPlacement};
use crate::ui::sidebar::SidebarMode;

/// Which pane has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Focus {
    /// The sidebar.
    Sidebar,
    /// The document viewport.
    #[default]
    Viewport,
    /// An overlay, which swallows all input except `Esc` and `Ctrl+C`.
    Overlay,
}

/// Whether a tab is being read or edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TabMode {
    /// Reading. The mode perga opens in, always.
    #[default]
    Read,
    /// Editing.
    Edit,
}

impl TabMode {
    /// The label shown at the left of the status bar.
    pub fn label(self) -> &'static str {
        match self {
            TabMode::Read => "READ",
            TabMode::Edit => "EDIT",
        }
    }
}

/// One tab: a document, its history, and its own scroll and find state.
#[derive(Debug, Clone, Default)]
pub struct Tab {
    /// Read or edit.
    pub mode: TabMode,
}

impl Tab {
    /// The label shown in the tab bar.
    ///
    /// A tab with no document open shows the welcome screen.
    pub fn label(&self) -> &str {
        "welcome"
    }
}

/// The sidebar's own state.
#[derive(Debug, Clone)]
pub struct Sidebar {
    /// Whether the user has it switched on.
    pub visible: bool,
    /// The width the user has chosen, in columns.
    pub width: u16,
    /// Which of the four modes is showing.
    pub mode: SidebarMode,
}

/// How prominent a status message is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    /// Ordinary feedback.
    Info,
    /// Something did not work but the application carries on.
    Warning,
    /// Something failed.
    Error,
}

/// The status bar's transient content.
#[derive(Debug, Clone, Default)]
pub struct StatusLine {
    /// The current message, if any.
    pub message: Option<(String, Severity)>,
    /// A partially typed key sequence, shown as `g…`.
    pub pending: Option<String>,
}

impl StatusLine {
    /// Show a message.
    pub fn set(&mut self, text: impl Into<String>, severity: Severity) {
        self.message = Some((text.into(), severity));
    }

    /// Clear any message.
    pub fn clear(&mut self) {
        self.message = None;
    }
}

/// Which overlay is open, if any.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Overlay {
    /// The keybinding reference, generated from the resolved keymap.
    Help {
        /// How far the reference is scrolled.
        scroll: u16,
    },
}

/// The whole application state.
pub struct App {
    /// The resolved theme. Nothing in the UI hardcodes a colour.
    pub theme: Theme,
    /// The resolved keymap, and the source of the help overlay.
    pub keymap: Keymap,
    /// Presentation settings.
    pub ui: UiConfig,
    /// Open tabs. Never empty.
    pub tabs: Vec<Tab>,
    /// Index into [`App::tabs`].
    pub active_tab: usize,
    /// The sidebar.
    pub sidebar: Sidebar,
    /// Which pane has focus.
    pub focus: Focus,
    /// The open overlay, if any.
    pub overlay: Option<Overlay>,
    /// The status bar.
    pub status: StatusLine,
    /// Set when the application should exit after the current update.
    pub should_quit: bool,
    /// Set when the application should suspend itself after the current update.
    pub should_suspend: bool,
    /// The process exit code.
    pub exit_code: u8,
    /// The last known terminal size, so layout decisions can be made outside a
    /// draw call.
    pub area: Rect,
    /// Whether mouse capture is on.
    pub mouse_capture: bool,
}

impl App {
    /// A new application with built-in defaults.
    pub fn new(theme: Theme, keymap: Keymap, ui: UiConfig) -> Self {
        let sidebar = Sidebar {
            visible: ui.sidebar_visible,
            width: ui.sidebar_width,
            mode: ui.sidebar_default_mode,
        };
        let mouse_capture = ui.mouse;

        // Anything the keymap could not make sense of is the user's to know
        // about; it is never a hard error.
        let mut status = StatusLine::default();
        if let Some(warning) = keymap.warnings().first() {
            status.set(warning.clone(), Severity::Warning);
        }

        App {
            theme,
            keymap,
            ui,
            tabs: vec![Tab::default()],
            active_tab: 0,
            sidebar,
            focus: Focus::Viewport,
            overlay: None,
            status,
            should_quit: false,
            should_suspend: false,
            exit_code: 0,
            area: Rect::default(),
            mouse_capture,
        }
    }

    /// The active tab.
    pub fn tab(&self) -> &Tab {
        &self.tabs[self.active_tab]
    }

    /// The layout for the current state and terminal size.
    pub fn frames(&self) -> layout::Frames {
        layout::compute(layout::LayoutInput {
            area: self.area,
            sidebar_visible: self.sidebar.visible,
            sidebar_width: self.sidebar.width,
            narrow_threshold: self.ui.narrow_threshold,
            tab_count: self.tabs.len(),
            always_show_tabs: self.ui.always_show_tabs,
            show_status_bar: self.ui.show_status_bar,
        })
    }

    /// Apply one action. The only place application state changes.
    pub fn update(&mut self, action: Action) {
        // Any deliberate action supersedes whatever the status bar was saying.
        if !matches!(action, Action::Resize(..)) {
            self.status.clear();
        }

        match action {
            // -- Lifecycle -----------------------------------------------
            Action::Quit => self.should_quit = true,
            Action::ForceQuit(code) => {
                self.should_quit = true;
                self.exit_code = code;
            }
            Action::Suspend => self.should_suspend = true,
            Action::Resize(width, height) => {
                // The chosen width is deliberately not clamped here. Layout
                // clamps what it draws, so a temporary shrink of the terminal
                // narrows the sidebar on screen without destroying the width
                // the user picked; widening the terminal restores it.
                self.area = Rect::new(0, 0, width, height);
            }

            // -- Focus and chrome ----------------------------------------
            Action::FocusNext => self.cycle_focus(true),
            Action::FocusPrev => self.cycle_focus(false),
            Action::ToggleSidebar => self.toggle_sidebar(),
            Action::SidebarWiden => self.resize_sidebar(1),
            Action::SidebarNarrow => self.resize_sidebar(-1),
            Action::SetSidebarMode(mode) => {
                self.sidebar.mode = mode;
                if !self.sidebar.visible {
                    self.sidebar.visible = true;
                }
            }
            Action::ToggleMouse => self.toggle_mouse(),
            Action::ToggleHelp => self.toggle_help(),
            Action::Escape => self.escape(),

            // Actions whose handlers arrive with the features that need them.
            _ => {}
        }
    }

    /// Move focus between the sidebar and the viewport.
    ///
    /// Edit mode locks focus to the viewport, and an open overlay owns focus
    /// until it closes.
    fn cycle_focus(&mut self, _forward: bool) {
        if self.overlay.is_some() || self.tab().mode == TabMode::Edit {
            return;
        }

        // With only two focusable panes, forwards and backwards agree.
        self.focus = match self.focus {
            Focus::Viewport if self.sidebar.visible => Focus::Sidebar,
            Focus::Sidebar => Focus::Viewport,
            other => other,
        };
    }

    fn toggle_sidebar(&mut self) {
        self.sidebar.visible = !self.sidebar.visible;

        if self.sidebar.visible {
            // An overlaid sidebar is useless without focus: it covers the
            // viewport, so the keys the user is about to press should reach it.
            if self.frames().sidebar_placement == SidebarPlacement::Overlaid {
                self.focus = Focus::Sidebar;
            }
        } else if self.focus == Focus::Sidebar {
            self.focus = Focus::Viewport;
        }
    }

    /// Widen or narrow the sidebar, within what the terminal can hold.
    fn resize_sidebar(&mut self, delta: i32) {
        let requested =
            (i32::from(self.sidebar.width) + delta).clamp(0, i32::from(u16::MAX)) as u16;

        self.sidebar.width = if self.area.width > 0 {
            layout::clamp_sidebar_width(requested, self.area.width)
        } else {
            requested
        };
    }

    fn toggle_mouse(&mut self) {
        self.mouse_capture = !self.mouse_capture;

        match terminal::set_mouse_capture(self.mouse_capture) {
            Ok(()) => {
                let state = if self.mouse_capture { "on" } else { "off" };
                self.status
                    .set(format!("Mouse capture {state}"), Severity::Info);
            }
            Err(e) => {
                self.mouse_capture = !self.mouse_capture;
                self.status
                    .set(format!("Cannot change mouse capture: {e}"), Severity::Error);
            }
        }
    }

    fn toggle_help(&mut self) {
        match self.overlay {
            Some(Overlay::Help { .. }) => self.close_overlay(),
            None => {
                self.overlay = Some(Overlay::Help { scroll: 0 });
                self.focus = Focus::Overlay;
            }
        }
    }

    /// `Esc`: close the overlay, abandon a pending key sequence, or leave edit
    /// mode, in that order.
    fn escape(&mut self) {
        if self.overlay.is_some() {
            self.close_overlay();
            return;
        }

        if !self.keymap.pending().is_empty() {
            self.keymap.clear_pending();
            self.status.pending = None;
        }
    }

    fn close_overlay(&mut self) {
        self.overlay = None;
        self.focus = if self.sidebar.visible
            && self.frames().sidebar_placement == SidebarPlacement::Overlaid
        {
            Focus::Sidebar
        } else {
            Focus::Viewport
        };
    }
}

/// A message from any source, waiting to become an [`Action`].
///
/// Terminal input, signals, and every background worker all send these down one
/// channel, so the main loop is a single blocking `recv` and nothing else.
#[derive(Debug)]
pub enum Message {
    /// A terminal input event.
    Input(crossterm::event::Event),
    /// A signal was delivered. The payload is the `libc` signal number.
    Signal(i32),
}

/// Run the application until it quits.
///
/// The loop blocks on `recv` and does no polling, which is what makes the 0%
/// idle CPU target hold by construction rather than by tuning a timeout.
pub fn run(terminal: &mut Tui, app: &mut App, messages: &Receiver<Message>) -> anyhow::Result<()> {
    // Seed the size before the first frame so layout-dependent state is right
    // from the start rather than after the first resize.
    let size = terminal.size()?;
    app.update(Action::Resize(size.width, size.height));

    loop {
        terminal.draw(|frame| ui::render(app, frame))?;

        let Ok(message) = messages.recv() else {
            // Every sender is gone, which can only happen during teardown.
            break;
        };

        for action in crate::event::translate(app, message) {
            app.update(action);
        }

        if app.should_suspend {
            app.should_suspend = false;
            suspend(terminal)?;
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}

/// Suspend the process with `SIGTSTP` and pick the terminal back up on resume.
///
/// This is what a terminal user expects `Ctrl+Z` to do in a full-screen
/// program: `fg` must bring the screen back intact.
fn suspend(terminal: &mut Tui) -> anyhow::Result<()> {
    let mouse = terminal::mouse_capture_active();

    terminal::restore()?;

    #[cfg(unix)]
    signal_hook::low_level::raise(signal_hook::consts::SIGTSTP)?;

    *terminal = terminal::setup(mouse)?;
    // The screen was handed back to the shell while suspended, so the whole
    // frame has to be repainted rather than diffed against what was there.
    terminal.clear()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An app sized like a comfortable terminal.
    fn app() -> App {
        let mut app = App::new(Theme::dark(), Keymap::defaults(), UiConfig::default());
        app.update(Action::Resize(120, 40));
        app
    }

    #[test]
    fn opens_in_read_mode_with_the_viewport_focused() {
        let app = app();
        assert_eq!(app.tab().mode, TabMode::Read);
        assert_eq!(app.focus, Focus::Viewport);
        assert!(app.overlay.is_none());
    }

    #[test]
    fn focus_cycles_between_the_sidebar_and_the_viewport() {
        let mut app = app();
        app.update(Action::FocusNext);
        assert_eq!(app.focus, Focus::Sidebar);
        app.update(Action::FocusNext);
        assert_eq!(app.focus, Focus::Viewport);
        app.update(Action::FocusPrev);
        assert_eq!(app.focus, Focus::Sidebar);
    }

    #[test]
    fn hiding_the_sidebar_moves_focus_out_of_it() {
        let mut app = app();
        app.update(Action::FocusNext);
        assert_eq!(app.focus, Focus::Sidebar);

        app.update(Action::ToggleSidebar);
        assert!(!app.sidebar.visible);
        assert_eq!(app.focus, Focus::Viewport);
    }

    #[test]
    fn focus_does_not_move_into_a_hidden_sidebar() {
        let mut app = app();
        app.update(Action::ToggleSidebar);
        app.update(Action::FocusNext);
        assert_eq!(app.focus, Focus::Viewport);
    }

    #[test]
    fn an_overlaid_sidebar_takes_focus_when_it_opens() {
        let mut app = app();
        app.update(Action::Resize(70, 40));
        app.update(Action::ToggleSidebar); // hide
        app.update(Action::ToggleSidebar); // show, overlaid
        assert_eq!(app.frames().sidebar_placement, SidebarPlacement::Overlaid);
        assert_eq!(app.focus, Focus::Sidebar);
    }

    #[test]
    fn a_split_sidebar_does_not_steal_focus_when_it_opens() {
        let mut app = app();
        app.update(Action::ToggleSidebar);
        app.update(Action::ToggleSidebar);
        assert_eq!(app.frames().sidebar_placement, SidebarPlacement::Split);
        assert_eq!(app.focus, Focus::Viewport);
    }

    #[test]
    fn resizing_the_sidebar_stays_within_bounds() {
        let mut app = app();
        for _ in 0..200 {
            app.update(Action::SidebarWiden);
        }
        assert!(app.sidebar.width <= 60, "{}", app.sidebar.width);

        for _ in 0..200 {
            app.update(Action::SidebarNarrow);
        }
        assert_eq!(app.sidebar.width, layout::MIN_SIDEBAR_WIDTH);
    }

    #[test]
    fn a_shrunken_terminal_narrows_the_drawn_sidebar_but_keeps_the_choice() {
        let mut app = app();
        for _ in 0..40 {
            app.update(Action::SidebarWiden);
        }
        let chosen = app.sidebar.width;
        assert_eq!(chosen, 60);

        app.update(Action::Resize(60, 20));
        assert_eq!(app.frames().sidebar.unwrap().width, 30);
        assert_eq!(app.sidebar.width, chosen, "the chosen width was destroyed");

        // ...and it comes back when there is room for it again.
        app.update(Action::Resize(120, 40));
        assert_eq!(app.frames().sidebar.unwrap().width, chosen);
    }

    #[test]
    fn help_opens_takes_focus_and_closes_again() {
        let mut app = app();
        app.update(Action::ToggleHelp);
        assert_eq!(app.overlay, Some(Overlay::Help { scroll: 0 }));
        assert_eq!(app.focus, Focus::Overlay);

        app.update(Action::ToggleHelp);
        assert!(app.overlay.is_none());
        assert_eq!(app.focus, Focus::Viewport);
    }

    #[test]
    fn escape_closes_an_overlay() {
        let mut app = app();
        app.update(Action::ToggleHelp);
        app.update(Action::Escape);
        assert!(app.overlay.is_none());
        assert_eq!(app.focus, Focus::Viewport);
    }

    #[test]
    fn focus_does_not_move_while_an_overlay_is_open() {
        let mut app = app();
        app.update(Action::ToggleHelp);
        app.update(Action::FocusNext);
        assert_eq!(app.focus, Focus::Overlay);
    }

    #[test]
    fn switching_sidebar_mode_reveals_a_hidden_sidebar() {
        let mut app = app();
        app.update(Action::ToggleSidebar);
        assert!(!app.sidebar.visible);

        app.update(Action::SetSidebarMode(SidebarMode::Outline));
        assert!(app.sidebar.visible);
        assert_eq!(app.sidebar.mode, SidebarMode::Outline);
    }

    #[test]
    fn quit_sets_the_flag() {
        let mut app = app();
        app.update(Action::Quit);
        assert!(app.should_quit);
        assert_eq!(app.exit_code, 0);
    }
}
