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
use crate::search::content::SearchEvent;
use crate::ui::hints;
use crate::ui::overlay::prompt::TextEdit;
use crate::vault::index::IndexEvent;
use crate::vault::walker::WalkEvent;
use crate::vault::watch::WatchEvent;

/// Translate one message into the actions it produces.
///
/// Returns a list because a single message can be more than one action, and
/// because most messages are none at all.
pub fn translate(app: &mut App, message: Message) -> Vec<Action> {
    match message {
        Message::Input(event) => translate_input(app, event),
        Message::Signal(signal) => translate_signal(signal),
        Message::SyntaxReady => vec![Action::SyntaxReady],
        Message::ThemeChanged => vec![Action::ReloadTheme],
        Message::Watch(event) => vec![match event {
            WatchEvent::Changed(paths) => Action::FilesChanged(paths),
            WatchEvent::Removed(paths) => Action::FilesRemoved(paths),
            WatchEvent::Stopped(reason) => Action::WatchStopped(reason),
        }],
        Message::Search(event) => vec![match event {
            SearchEvent::Hits(hits) => Action::SearchHits(hits),
            SearchEvent::Finished { total, truncated } => {
                Action::SearchFinished { total, truncated }
            }
            SearchEvent::BadPattern(e) => Action::SearchFailed(e),
        }],
        Message::Index(event) => vec![match event {
            IndexEvent::Indexed(entries) => Action::IndexBatch(entries),
            IndexEvent::Finished => Action::IndexFinished,
        }],
        Message::Walk(event) => vec![match event {
            WalkEvent::Entries(entries) => Action::VaultEntries(entries),
            WalkEvent::Finished(total) => Action::VaultWalkFinished(total),
            WalkEvent::Failed(reason) => Action::VaultWalkFailed(reason),
        }],
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
        // A bracketed paste is one edit, not one per character: 5,000 lines
        // pasted must be a single undo step.
        CtEvent::Paste(text) if app.tab().mode == TabMode::Edit => {
            vec![Action::EditPaste(text)]
        }
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

    // An open text line owns every key it can use, the same way edit mode owns
    // the text area. Without that, typing `a` into a filter would toggle
    // non-Markdown files instead.
    if app.sidebar.filter.is_some() {
        return translate_text_line(chord);
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

            // Section 12: in edit mode the text area owns every key the Edit
            // context did not claim. Outside it, an unbound key does nothing.
            if context == KeyContext::Edit {
                return vec![Action::EditInput(key)];
            }

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

/// Keys typed into the tree filter line.
///
/// Everything the line cannot use is swallowed rather than falling through to
/// the keymap: a stray `q` while typing a filter must not quit perga.
fn translate_text_line(chord: KeyChord) -> Vec<Action> {
    let edit = |edit| vec![Action::TreeFilterEdit(edit)];
    let ctrl = chord.modifiers.contains(KeyModifiers::CONTROL);

    match chord.code {
        KeyCode::Esc => vec![Action::TreeFilterCancel],
        KeyCode::Enter => vec![Action::TreeFilterAccept],
        KeyCode::Backspace => edit(TextEdit::Backspace),
        KeyCode::Delete => edit(TextEdit::Delete),
        KeyCode::Left => edit(TextEdit::Left),
        KeyCode::Right => edit(TextEdit::Right),
        KeyCode::Home => edit(TextEdit::Home),
        KeyCode::End => edit(TextEdit::End),
        // The selection still moves while the filter is being typed, so the
        // user can type and choose without leaving the line.
        KeyCode::Down => vec![Action::SidebarDown],
        KeyCode::Up => vec![Action::SidebarUp],
        KeyCode::Char('u') if ctrl => edit(TextEdit::Clear),
        KeyCode::Char('w') if ctrl => edit(TextEdit::DeleteWordBack),
        KeyCode::Char(c) if !ctrl && !chord.modifiers.contains(KeyModifiers::ALT) => {
            edit(TextEdit::Insert(c))
        }
        _ => Vec::new(),
    }
}

/// Keys typed into the find bar.
///
/// Like the tree filter, the bar owns every key it can use; scrolling the
/// document while it is open would take the reader away from the match they
/// are looking at.
fn translate_find_key(chord: KeyChord) -> Vec<Action> {
    let edit = |edit| vec![Action::FindEdit(edit)];
    let ctrl = chord.modifiers.contains(KeyModifiers::CONTROL);

    match chord.code {
        KeyCode::Esc => vec![Action::CloseFind],
        KeyCode::Enter if chord.modifiers.contains(KeyModifiers::SHIFT) => vec![Action::FindPrev],
        KeyCode::Enter => vec![Action::FindNext],
        // `Shift+Enter` is not reportable on every terminal, so the arrows and
        // `Ctrl+N`/`Ctrl+P` cycle too.
        KeyCode::Down => vec![Action::FindNext],
        KeyCode::Up => vec![Action::FindPrev],
        KeyCode::Char('n') if ctrl => vec![Action::FindNext],
        KeyCode::Char('p') if ctrl => vec![Action::FindPrev],
        KeyCode::Backspace => edit(TextEdit::Backspace),
        KeyCode::Delete => edit(TextEdit::Delete),
        KeyCode::Left => edit(TextEdit::Left),
        KeyCode::Right => edit(TextEdit::Right),
        KeyCode::Home => edit(TextEdit::Home),
        KeyCode::End => edit(TextEdit::End),
        KeyCode::Char('u') if ctrl => edit(TextEdit::Clear),
        KeyCode::Char('w') if ctrl => edit(TextEdit::DeleteWordBack),
        KeyCode::Char(c) if !chord.modifiers.intersects(CTRL_OR_ALT) => edit(TextEdit::Insert(c)),
        _ => Vec::new(),
    }
}

/// Keys typed into a single-line input.
///
/// Shared by the prompt, the quick switcher, and the tree filter: one set of
/// editing keys, so `Ctrl+W` means the same thing wherever text is typed.
fn translate_line_key(
    chord: KeyChord,
    edit: impl Fn(TextEdit) -> Action,
    accept: Action,
    cancel: Action,
) -> Vec<Action> {
    let ctrl = chord.modifiers.contains(KeyModifiers::CONTROL);

    match chord.code {
        KeyCode::Esc => vec![cancel],
        KeyCode::Enter => vec![accept],
        KeyCode::Backspace => vec![edit(TextEdit::Backspace)],
        KeyCode::Delete => vec![edit(TextEdit::Delete)],
        KeyCode::Left => vec![edit(TextEdit::Left)],
        KeyCode::Right => vec![edit(TextEdit::Right)],
        KeyCode::Home => vec![edit(TextEdit::Home)],
        KeyCode::End => vec![edit(TextEdit::End)],
        KeyCode::Char('u') if ctrl => vec![edit(TextEdit::Clear)],
        KeyCode::Char('w') if ctrl => vec![edit(TextEdit::DeleteWordBack)],
        KeyCode::Char(c) if !chord.modifiers.intersects(CTRL_OR_ALT) => {
            vec![edit(TextEdit::Insert(c))]
        }
        _ => Vec::new(),
    }
}

/// The modifiers that mean a character key is a command rather than text.
const CTRL_OR_ALT: KeyModifiers = KeyModifiers::CONTROL.union(KeyModifiers::ALT);

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
        Overlay::Find => translate_find_key(chord),
        Overlay::Confirm { choices, .. } => match chord.code {
            KeyCode::Esc => vec![Action::Escape],
            // Only the keys the dialog offered do anything; a stray press is
            // swallowed rather than being taken as an answer.
            KeyCode::Char(c)
                if choices
                    .iter()
                    .any(|(key, _)| *key == c.to_ascii_lowercase()) =>
            {
                vec![Action::Confirm(c.to_ascii_lowercase())]
            }
            _ => Vec::new(),
        },
        Overlay::Prompt { .. } => translate_line_key(
            chord,
            Action::PromptEdit,
            Action::PromptAccept,
            Action::Escape,
        ),
        Overlay::Switcher { .. } => match chord.code {
            KeyCode::Down | KeyCode::Tab => vec![Action::SwitcherMove(1)],
            KeyCode::Up | KeyCode::BackTab => vec![Action::SwitcherMove(-1)],
            KeyCode::Char('n') if chord.modifiers.contains(KeyModifiers::CONTROL) => {
                vec![Action::SwitcherMove(1)]
            }
            KeyCode::Char('p') if chord.modifiers.contains(KeyModifiers::CONTROL) => {
                vec![Action::SwitcherMove(-1)]
            }
            // `Ctrl+Enter` is not reportable on every terminal; Section 8.3
            // gives `Tab` then `Enter` as the way through on those, which the
            // `Tab` binding above already provides for moving.
            KeyCode::Enter if chord.modifiers.contains(KeyModifiers::CONTROL) => {
                vec![Action::SwitcherAccept { new_tab: true }]
            }
            _ => translate_line_key(
                chord,
                Action::SwitcherEdit,
                Action::SwitcherAccept { new_tab: false },
                Action::Escape,
            ),
        },
        Overlay::Disambiguate {
            candidates,
            selected,
            ..
        } => {
            let last = candidates.len().saturating_sub(1);
            match chord.code {
                KeyCode::Esc | KeyCode::Char('q') => vec![Action::Escape],
                KeyCode::Enter | KeyCode::Char('l') => vec![Action::ChooseCandidate(*selected)],
                KeyCode::Char('j') | KeyCode::Down => {
                    *selected = (*selected + 1).min(last);
                    Vec::new()
                }
                KeyCode::Char('k') | KeyCode::Up => {
                    *selected = selected.saturating_sub(1);
                    Vec::new()
                }
                // A one-key pick for the first nine candidates, which is more
                // than any realistic collision produces.
                KeyCode::Char(c) if c.is_ascii_digit() && c != '0' => {
                    let at = c as usize - '1' as usize;
                    if at <= last {
                        vec![Action::ChooseCandidate(at)]
                    } else {
                        Vec::new()
                    }
                }
                _ => Vec::new(),
            }
        }
        Overlay::Hints { links, typed } => {
            let count = links.len();

            match chord.code {
                KeyCode::Esc => vec![Action::Escape],
                KeyCode::Backspace => {
                    typed.pop();
                    Vec::new()
                }
                KeyCode::Char(c) if !chord.modifiers.intersects(CTRL_OR_ALT) => {
                    typed.push(c.to_ascii_lowercase());

                    match hints::match_typed(typed, count) {
                        hints::HintMatch::Complete(at) => {
                            let link = links[at];
                            vec![Action::Escape, Action::FollowHintedLink(link)]
                        }
                        hints::HintMatch::Partial => Vec::new(),
                        // A key that matches nothing is a typo, not a
                        // command: drop it rather than cancelling the mode.
                        hints::HintMatch::None => {
                            typed.pop();
                            Vec::new()
                        }
                    }
                }
                _ => Vec::new(),
            }
        }
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
    use crate::config::schema::{FilesConfig, UiConfig};
    use crate::theme::Theme;

    fn app() -> App {
        let mut app = App::new(
            Theme::dark(),
            Keymap::defaults(),
            UiConfig::default(),
            FilesConfig::default(),
        );
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
            vec![Action::SidebarDown]
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
