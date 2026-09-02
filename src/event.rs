//! Translation of terminal input events into [`Action`] values.
//!
//! This module decides *which context* a key press is resolved in and hands the
//! press to the keymap; the keymap decides what the key means. Nothing here
//! matches on a `KeyCode` for an application binding — the moment two places do
//! that, the conflicts described in Section 12 of the build specification start
//! appearing as unreproducible bugs.

use crossterm::event::{
    Event as CtEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind,
};

use crate::action::Action;
use crate::app::{App, Focus, Message, Overlay, TabMode};
use crate::config::keymap::{KeyChord, KeyContext, Resolution};

/// Translate one message into the actions it produces.
///
/// Returns a list because a single message can be more than one action, and
/// because most messages are none at all.
pub fn translate(app: &mut App, message: Message) -> Vec<Action> {
    match message {
        Message::Input(event) => translate_input(app, event),
        Message::Signal(signal) => translate_signal(signal),
    }
}

fn translate_input(app: &mut App, event: CtEvent) -> Vec<Action> {
    match event {
        CtEvent::Key(key) => translate_key(app, key),
        CtEvent::Resize(width, height) => vec![Action::Resize(width, height)],
        CtEvent::Mouse(mouse) => match mouse.kind {
            MouseEventKind::ScrollDown => vec![Action::ScrollWheelDown],
            MouseEventKind::ScrollUp => vec![Action::ScrollWheelUp],
            _ => Vec::new(),
        },
        // Focus changes and pastes outside edit mode are not interesting.
        _ => Vec::new(),
    }
}

fn translate_key(app: &mut App, key: KeyEvent) -> Vec<Action> {
    // With the kitty keyboard protocol a key produces press, repeat, and
    // release events. Acting on all three would triple every keystroke.
    if key.kind == KeyEventKind::Release {
        return Vec::new();
    }

    // `Ctrl+C` is the one binding that is not remappable and not context
    // sensitive: it must always be able to end the program.
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return vec![Action::Quit];
    }

    let chord = KeyChord::from_event(key);

    if app.overlay.is_some() {
        return translate_overlay_key(app, chord);
    }

    let context = context_for(app);
    match app.keymap.resolve(context, chord) {
        Resolution::Action(action) => {
            app.status.pending = None;
            vec![action]
        }
        Resolution::Pending(hint) => {
            app.status.pending = Some(hint);
            Vec::new()
        }
        Resolution::Unbound => {
            app.status.pending = None;
            Vec::new()
        }
    }
}

/// Which keymap context the next press is resolved in.
fn context_for(app: &App) -> KeyContext {
    if app.tab().mode == TabMode::Edit {
        // In edit mode the text area owns every key the Edit context does not
        // claim. See `docs/keybindings.md`.
        KeyContext::Edit
    } else if app.focus == Focus::Sidebar {
        KeyContext::Sidebar
    } else {
        KeyContext::Viewport
    }
}

/// An open overlay swallows all input except the keys that close it and the
/// ones that scroll it.
fn translate_overlay_key(app: &mut App, chord: KeyChord) -> Vec<Action> {
    let Some(overlay) = &mut app.overlay else {
        return Vec::new();
    };

    match overlay {
        Overlay::Help { scroll } => match chord.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => vec![Action::Escape],
            KeyCode::Char('j') | KeyCode::Down => {
                *scroll = scroll.saturating_add(1);
                Vec::new()
            }
            KeyCode::Char('k') | KeyCode::Up => {
                *scroll = scroll.saturating_sub(1);
                Vec::new()
            }
            KeyCode::Char('g') | KeyCode::Home => {
                *scroll = 0;
                Vec::new()
            }
            KeyCode::PageDown => {
                *scroll = scroll.saturating_add(10);
                Vec::new()
            }
            KeyCode::PageUp => {
                *scroll = scroll.saturating_sub(10);
                Vec::new()
            }
            _ => Vec::new(),
        },
    }
}

/// Translate a delivered signal.
///
/// The exit codes follow the shell convention of 128 plus the signal number, so
/// a supervisor sees the usual value.
fn translate_signal(signal: i32) -> Vec<Action> {
    #[cfg(unix)]
    {
        use signal_hook::consts::{SIGCONT, SIGHUP, SIGINT, SIGTERM, SIGTSTP};

        match signal {
            SIGINT | SIGTERM | SIGHUP => vec![Action::ForceQuit(exit_code_for_signal(signal))],
            SIGTSTP => vec![Action::Suspend],
            // Resuming needs nothing but a redraw, which the loop does anyway.
            SIGCONT => Vec::new(),
            _ => Vec::new(),
        }
    }

    #[cfg(not(unix))]
    {
        let _ = signal;
        Vec::new()
    }
}

/// The exit code for a signal, following the 128 plus signal number convention.
pub fn exit_code_for_signal(signal: i32) -> u8 {
    (128 + signal).clamp(0, 255) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::keymap::Keymap;
    use crate::config::schema::UiConfig;
    use crate::theme::Theme;

    fn app() -> App {
        let mut app = App::new(Theme::dark(), Keymap::defaults(), UiConfig::default());
        app.update(Action::Resize(120, 40));
        app
    }

    fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> Vec<Action> {
        translate_key(app, KeyEvent::new(code, modifiers))
    }

    #[test]
    fn a_bound_key_produces_its_action() {
        let mut app = app();
        assert_eq!(
            press(&mut app, KeyCode::Char('q'), KeyModifiers::NONE),
            vec![Action::Quit]
        );
    }

    #[test]
    fn context_follows_focus() {
        let mut app = app();
        assert_eq!(
            press(&mut app, KeyCode::Char('j'), KeyModifiers::NONE),
            vec![Action::ScrollLineDown]
        );

        app.update(Action::FocusNext);
        assert_eq!(app.focus, Focus::Sidebar);
        assert_eq!(
            press(&mut app, KeyCode::Char('j'), KeyModifiers::NONE),
            vec![Action::TreeDown]
        );
    }

    #[test]
    fn a_pending_sequence_is_shown_in_the_status_bar() {
        let mut app = app();
        assert!(press(&mut app, KeyCode::Char('g'), KeyModifiers::NONE).is_empty());
        assert_eq!(app.status.pending.as_deref(), Some("g…"));

        assert_eq!(
            press(&mut app, KeyCode::Char('t'), KeyModifiers::NONE),
            vec![Action::NextTab]
        );
        assert!(app.status.pending.is_none());
    }

    #[test]
    fn key_release_events_are_ignored() {
        let mut app = app();
        let mut key = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE);
        key.kind = KeyEventKind::Release;
        assert!(translate_key(&mut app, key).is_empty());
    }

    #[test]
    fn ctrl_c_always_quits_even_behind_an_overlay() {
        let mut app = app();
        app.update(Action::ToggleHelp);
        assert_eq!(
            press(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL),
            vec![Action::Quit]
        );
    }

    #[test]
    fn an_overlay_swallows_unrelated_keys() {
        let mut app = app();
        app.update(Action::ToggleHelp);
        // `q` closes the help overlay rather than quitting.
        assert_eq!(
            press(&mut app, KeyCode::Char('q'), KeyModifiers::NONE),
            vec![Action::Escape]
        );

        // The overlay is still open: `q` produced an action but nothing
        // applied it. `Ctrl+T` would open a tab, but the overlay owns input.
        assert!(app.overlay.is_some());
        assert!(press(&mut app, KeyCode::Char('t'), KeyModifiers::CONTROL).is_empty());
    }

    #[test]
    fn the_help_overlay_scrolls() {
        let mut app = app();
        app.update(Action::ToggleHelp);
        press(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
        press(&mut app, KeyCode::Char('j'), KeyModifiers::NONE);
        assert_eq!(app.overlay, Some(Overlay::Help { scroll: 2 }));

        press(&mut app, KeyCode::Char('k'), KeyModifiers::NONE);
        assert_eq!(app.overlay, Some(Overlay::Help { scroll: 1 }));
    }

    #[test]
    fn a_resize_event_becomes_a_resize_action() {
        let mut app = app();
        assert_eq!(
            translate_input(&mut app, CtEvent::Resize(80, 24)),
            vec![Action::Resize(80, 24)]
        );
    }

    #[cfg(unix)]
    #[test]
    fn signals_map_to_the_right_actions_and_codes() {
        use signal_hook::consts::{SIGCONT, SIGINT, SIGTERM, SIGTSTP};

        assert_eq!(translate_signal(SIGINT), vec![Action::ForceQuit(130)]);
        assert_eq!(translate_signal(SIGTERM), vec![Action::ForceQuit(143)]);
        assert_eq!(translate_signal(SIGTSTP), vec![Action::Suspend]);
        assert!(translate_signal(SIGCONT).is_empty());

        assert_eq!(exit_code_for_signal(SIGINT), 130);
        assert_eq!(exit_code_for_signal(SIGTERM), 143);
    }
}
