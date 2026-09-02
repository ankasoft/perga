//! Key-string parsing, the default binding table, and user remapping.
//!
//! There is exactly one source of truth for keys: [`DEFAULT_BINDINGS`]. The
//! resolved [`Keymap`] drives input dispatch, and the help overlay is generated
//! from the same structure, so a remap can never make the help wrong.
//!
//! # Contexts
//!
//! The same key means different things depending on what has focus: `j` scrolls
//! the viewport but moves the selection in the sidebar. Each binding therefore
//! names a [`KeyContext`], and lookup tries the active context before falling
//! back to [`KeyContext::Global`].
//!
//! # Sequences
//!
//! A binding may be a sequence of chords written as space-separated tokens
//! (`"g g"`, `"m 1"`, `"t enter"`). Typing the first chord of a sequence puts
//! the keymap in a pending state; see [`Keymap::resolve`].

use std::collections::HashMap;
use std::fmt;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::Action;
use crate::ui::sidebar::SidebarMode;

/// Where a binding applies.
///
/// Lookup order is the active context first, then [`KeyContext::Global`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyContext {
    /// Applies wherever it is not shadowed by a more specific context.
    Global,
    /// Applies when the document viewport has focus in read mode.
    Viewport,
    /// Applies when the sidebar has focus.
    Sidebar,
    /// Applies in edit mode.
    ///
    /// Edit mode is deliberately sparse: the text area owns every key that is
    /// not listed here. See `docs/keybindings.md`.
    Edit,
}

impl KeyContext {
    /// The heading this context appears under in the help overlay.
    pub fn heading(self) -> &'static str {
        match self {
            KeyContext::Global => "Global",
            KeyContext::Viewport => "Reading",
            KeyContext::Sidebar => "Sidebar (focused)",
            KeyContext::Edit => "Editing",
        }
    }
}

/// A single key press: a code plus the modifiers that are significant for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct KeyChord {
    pub code: KeyCode,
    pub modifiers: KeyModifiers,
}

impl KeyChord {
    /// Build a chord from a terminal key event, normalising the modifiers.
    ///
    /// Two normalisations matter, and both exist because terminals disagree:
    ///
    /// * For a plain character, case already carries `Shift` — `G` arrives as
    ///   `Char('G')`, sometimes with `SHIFT` set and sometimes without. The
    ///   `SHIFT` bit is dropped so both spellings match the same binding.
    /// * For a character with `Ctrl` held, the case is not meaningful but
    ///   `Shift` is: `Ctrl+Shift+F` must be distinguishable from `Ctrl+F` on
    ///   terminals that can report the difference. The character is lowercased
    ///   and `SHIFT` is kept.
    ///
    /// Everything outside `SHIFT`, `CONTROL`, and `ALT` is discarded, so a
    /// terminal that reports `KEYPAD` or a `Super` modifier does not silently
    /// stop matching bindings.
    pub fn from_event(event: KeyEvent) -> Self {
        let mut modifiers =
            event.modifiers & (KeyModifiers::SHIFT | KeyModifiers::CONTROL | KeyModifiers::ALT);
        let mut code = event.code;

        if let KeyCode::Char(c) = code {
            if modifiers.contains(KeyModifiers::CONTROL) {
                code = KeyCode::Char(c.to_ascii_lowercase());
            } else if c.is_alphabetic() {
                modifiers.remove(KeyModifiers::SHIFT);
            }
        }

        KeyChord { code, modifiers }
    }

    /// Parse one token of a binding string, such as `ctrl+shift+f` or `g`.
    pub fn parse(token: &str) -> Result<Self, KeyParseError> {
        if token.is_empty() {
            return Err(KeyParseError::Empty);
        }

        let mut modifiers = KeyModifiers::NONE;
        let mut rest = token;

        // `+` is also a bindable character, so only split on it when what
        // precedes it is a modifier name.
        while let Some(idx) = rest.find('+') {
            let (prefix, tail) = rest.split_at(idx);
            let modifier = match prefix {
                "ctrl" => KeyModifiers::CONTROL,
                "alt" => KeyModifiers::ALT,
                "shift" => KeyModifiers::SHIFT,
                _ => break,
            };
            if modifiers.contains(modifier) {
                return Err(KeyParseError::DuplicateModifier(token.to_string()));
            }
            modifiers |= modifier;
            rest = &tail[1..];
        }

        let mut code = match rest {
            "enter" => KeyCode::Enter,
            "esc" => KeyCode::Esc,
            "tab" => KeyCode::Tab,
            "backtab" => KeyCode::BackTab,
            "space" => KeyCode::Char(' '),
            "backspace" => KeyCode::Backspace,
            "delete" => KeyCode::Delete,
            "insert" => KeyCode::Insert,
            "home" => KeyCode::Home,
            "end" => KeyCode::End,
            "pageup" => KeyCode::PageUp,
            "pagedown" => KeyCode::PageDown,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            other => {
                if let Some(n) = other
                    .strip_prefix('f')
                    .and_then(|d| d.parse::<u8>().ok())
                    .filter(|n| (1..=12).contains(n))
                {
                    KeyCode::F(n)
                } else {
                    let mut chars = other.chars();
                    match (chars.next(), chars.next()) {
                        (Some(c), None) => KeyCode::Char(c),
                        _ => return Err(KeyParseError::UnknownKey(other.to_string())),
                    }
                }
            }
        };

        // `"shift+g"` and `"G"` must resolve to the same chord: for a plain
        // letter the case carries the shift, so fold the modifier into it
        // before normalising. With `Ctrl` held the case is not meaningful and
        // `Shift` is significant in its own right, so this does not apply.
        if let KeyCode::Char(c) = code {
            if modifiers.contains(KeyModifiers::SHIFT)
                && !modifiers.contains(KeyModifiers::CONTROL)
                && c.is_alphabetic()
            {
                code = KeyCode::Char(c.to_ascii_uppercase());
                modifiers.remove(KeyModifiers::SHIFT);
            }
        }

        Ok(KeyChord::from_event(KeyEvent::new(code, modifiers)))
    }
}

impl fmt::Display for KeyChord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.modifiers.contains(KeyModifiers::CONTROL) {
            f.write_str("Ctrl+")?;
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            f.write_str("Alt+")?;
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            f.write_str("Shift+")?;
        }

        match self.code {
            KeyCode::Char(' ') => f.write_str("Space"),
            // A modified letter is conventionally written in upper case —
            // `Ctrl+B`, not `Ctrl+b` — even though the chord itself stores the
            // lower-case form.
            KeyCode::Char(c)
                if self
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                write!(f, "{}", c.to_uppercase())
            }
            KeyCode::Char(c) => write!(f, "{c}"),
            KeyCode::Enter => f.write_str("Enter"),
            KeyCode::Esc => f.write_str("Esc"),
            KeyCode::Tab => f.write_str("Tab"),
            KeyCode::BackTab => f.write_str("Shift+Tab"),
            KeyCode::Backspace => f.write_str("Backspace"),
            KeyCode::Delete => f.write_str("Delete"),
            KeyCode::Insert => f.write_str("Insert"),
            KeyCode::Home => f.write_str("Home"),
            KeyCode::End => f.write_str("End"),
            KeyCode::PageUp => f.write_str("PageUp"),
            KeyCode::PageDown => f.write_str("PageDown"),
            KeyCode::Up => f.write_str("Up"),
            KeyCode::Down => f.write_str("Down"),
            KeyCode::Left => f.write_str("Left"),
            KeyCode::Right => f.write_str("Right"),
            KeyCode::F(n) => write!(f, "F{n}"),
            other => write!(f, "{other:?}"),
        }
    }
}

/// A parsed binding: one chord, or a sequence of them.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct KeySequence(pub Vec<KeyChord>);

impl KeySequence {
    /// Parse a binding string. Tokens are separated by spaces; a single token
    /// is one chord, two or more are a sequence.
    pub fn parse(s: &str) -> Result<Self, KeyParseError> {
        let chords = s
            .split_whitespace()
            .map(KeyChord::parse)
            .collect::<Result<Vec<_>, _>>()?;

        if chords.is_empty() {
            return Err(KeyParseError::Empty);
        }

        Ok(KeySequence(chords))
    }
}

impl fmt::Display for KeySequence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, chord) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(" ")?;
            }
            write!(f, "{chord}")?;
        }
        Ok(())
    }
}

/// Why a binding string could not be parsed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum KeyParseError {
    /// The binding string was empty or contained only whitespace.
    #[error("a key binding cannot be empty")]
    Empty,
    /// A key name matched no named key and was longer than one character.
    ///
    /// This is what catches `"gt"`, which a user means as a sequence and must
    /// write as `"g t"`.
    #[error("`{0}` is not a key; write a sequence as space-separated tokens, e.g. \"g t\"")]
    UnknownKey(String),
    /// The same modifier appeared twice in one token.
    #[error("`{0}` repeats a modifier")]
    DuplicateModifier(String),
}

/// One row of the default binding table.
pub struct BindingSpec {
    /// The action the binding performs.
    pub action: Action,
    /// Where the binding applies.
    pub context: KeyContext,
    /// The default binding strings, in the order they are shown in help.
    pub keys: &'static [&'static str],
    /// One-line description, shown in the help overlay and in the docs.
    pub description: &'static str,
}

/// A convenience constructor keeping the table below readable.
const fn b(
    action: Action,
    context: KeyContext,
    keys: &'static [&'static str],
    description: &'static str,
) -> BindingSpec {
    BindingSpec {
        action,
        context,
        keys,
        description,
    }
}

/// The single source of truth for keys: every action, its default bindings, and
/// its description.
///
/// Every binding that depends on the kitty keyboard protocol is listed with its
/// always-available fallback alongside it, so the help overlay shows both. See
/// Section 8.6 of the build specification.
pub fn default_bindings() -> Vec<BindingSpec> {
    use Action as A;
    use KeyContext::{Edit, Global, Sidebar, Viewport};

    vec![
        // -- Global --------------------------------------------------------
        b(A::Quit, Global, &["q", "ctrl+q"], "Quit"),
        b(A::ToggleHelp, Global, &["?"], "Help overlay"),
        b(
            A::ToggleSidebar,
            Global,
            &["ctrl+b", "ctrl+e"],
            "Toggle the sidebar (Ctrl+E for tmux users)",
        ),
        b(A::FocusNext, Global, &["tab"], "Focus the next pane"),
        b(
            A::FocusPrev,
            Global,
            &["backtab"],
            "Focus the previous pane",
        ),
        b(
            A::SetSidebarMode(SidebarMode::Files),
            Global,
            &["m 1", "alt+1"],
            "Sidebar mode: files",
        ),
        b(
            A::SetSidebarMode(SidebarMode::Search),
            Global,
            &["m 2", "alt+2"],
            "Sidebar mode: search",
        ),
        b(
            A::SetSidebarMode(SidebarMode::Outline),
            Global,
            &["m 3", "alt+3"],
            "Sidebar mode: outline",
        ),
        b(
            A::SetSidebarMode(SidebarMode::Links),
            Global,
            &["m 4", "alt+4"],
            "Sidebar mode: links",
        ),
        b(A::ToggleMouse, Global, &["m m"], "Toggle mouse capture"),
        b(A::NewFile, Global, &["ctrl+n"], "New file"),
        b(A::OpenQuickSwitcher, Global, &["ctrl+o"], "Quick switcher"),
        b(
            A::OpenFindInDocument,
            Global,
            &["ctrl+f"],
            "Find in document",
        ),
        b(
            A::OpenProjectSearch,
            Global,
            &["ctrl+shift+f", "ctrl+g"],
            "Search the project",
        ),
        b(A::NewTab, Global, &["ctrl+t"], "New tab"),
        b(A::CloseTab, Global, &["ctrl+w"], "Close tab"),
        b(A::NextTab, Global, &["g t", "ctrl+pagedown"], "Next tab"),
        b(A::PrevTab, Global, &["g T", "ctrl+pageup"], "Previous tab"),
        b(
            A::Escape,
            Global,
            &["esc"],
            "Close the overlay, or leave edit mode",
        ),
        b(
            A::Suspend,
            Global,
            &["ctrl+z"],
            "Suspend perga (resume with `fg`)",
        ),
        // -- Reading -------------------------------------------------------
        b(
            A::ScrollLineDown,
            Viewport,
            &["j", "down"],
            "Scroll down one line",
        ),
        b(
            A::ScrollLineUp,
            Viewport,
            &["k", "up"],
            "Scroll up one line",
        ),
        b(
            A::ScrollHalfPageDown,
            Viewport,
            &["ctrl+d"],
            "Scroll down half a page",
        ),
        b(
            A::ScrollHalfPageUp,
            Viewport,
            &["ctrl+u"],
            "Scroll up half a page",
        ),
        b(
            A::ScrollPageDown,
            Viewport,
            &["space", "pagedown"],
            "Scroll down a page",
        ),
        b(
            A::ScrollPageUp,
            Viewport,
            &["b", "pageup"],
            "Scroll up a page",
        ),
        b(A::ScrollTop, Viewport, &["g g"], "Top of the document"),
        b(A::ScrollBottom, Viewport, &["G"], "Bottom of the document"),
        b(A::ScrollLeft, Viewport, &["h"], "Scroll left"),
        b(A::ScrollRight, Viewport, &["l"], "Scroll right"),
        b(A::PrevHeading, Viewport, &["{"], "Previous heading"),
        b(A::NextHeading, Viewport, &["}"], "Next heading"),
        b(A::NextLink, Viewport, &["n"], "Focus the next link"),
        b(A::PrevLink, Viewport, &["N"], "Focus the previous link"),
        b(
            A::FollowLink,
            Viewport,
            &["enter"],
            "Follow the focused link",
        ),
        b(
            A::FollowLinkInNewTab,
            Viewport,
            &["ctrl+enter", "t enter"],
            "Follow the focused link in a new tab",
        ),
        b(A::HintMode, Viewport, &["f"], "Label and jump to a link"),
        b(A::HistoryBack, Viewport, &["H", "alt+left"], "Back"),
        b(A::HistoryForward, Viewport, &["L", "alt+right"], "Forward"),
        b(
            A::RenameDocument,
            Viewport,
            &["R"],
            "Rename the active document",
        ),
        b(A::EnterEditMode, Viewport, &["e", "i"], "Enter edit mode"),
        b(A::OpenInExternalEditor, Viewport, &["o"], "Open in $EDITOR"),
        b(
            A::CopyDocumentPath,
            Viewport,
            &["y"],
            "Copy the document path",
        ),
        b(
            A::ReloadDocument,
            Viewport,
            &["r"],
            "Reload the active document",
        ),
        // -- Sidebar -------------------------------------------------------
        b(
            A::SidebarDown,
            Sidebar,
            &["j", "down"],
            "Move the selection down",
        ),
        b(A::SidebarUp, Sidebar, &["k", "up"], "Move the selection up"),
        b(
            A::SidebarActivate,
            Sidebar,
            &["l", "right", "enter"],
            "Open the selection",
        ),
        b(
            A::SidebarBack,
            Sidebar,
            &["h", "left"],
            "Collapse a directory, or go to its parent",
        ),
        b(
            A::SetSidebarMode(SidebarMode::Files),
            Sidebar,
            &["1"],
            "Sidebar mode: files",
        ),
        b(
            A::SetSidebarMode(SidebarMode::Search),
            Sidebar,
            &["2"],
            "Sidebar mode: search",
        ),
        b(
            A::SetSidebarMode(SidebarMode::Outline),
            Sidebar,
            &["3"],
            "Sidebar mode: outline",
        ),
        b(
            A::SetSidebarMode(SidebarMode::Links),
            Sidebar,
            &["4"],
            "Sidebar mode: links",
        ),
        b(A::TreeToggleHidden, Sidebar, &["."], "Toggle hidden files"),
        b(
            A::TreeToggleAllFiles,
            Sidebar,
            &["a"],
            "Toggle non-Markdown files",
        ),
        b(
            A::TreeFilter,
            Sidebar,
            &["/"],
            "Filter the tree, or edit the last search query",
        ),
        b(A::TreeRename, Sidebar, &["r"], "Rename the selected entry"),
        b(A::SidebarWiden, Sidebar, &[">"], "Widen the sidebar"),
        b(A::SidebarNarrow, Sidebar, &["<"], "Narrow the sidebar"),
        // -- Editing -------------------------------------------------------
        //
        // Deliberately sparse: in edit mode the text area owns every key that
        // is not listed here. See Section 12 of the build specification.
        b(A::Save, Edit, &["ctrl+s"], "Save"),
        b(A::Escape, Edit, &["esc"], "Leave edit mode"),
        b(A::Undo, Edit, &["ctrl+z"], "Undo"),
        b(A::Redo, Edit, &["ctrl+y"], "Redo"),
    ]
}

/// What a key press did to the keymap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Resolution {
    /// The press completed a binding.
    Action(Action),
    /// The press is a prefix of one or more bindings; the keymap is waiting for
    /// the next chord. The payload is what to show in the status bar.
    Pending(String),
    /// The press matched nothing.
    Unbound,
}

/// A binding as resolved for one context, ready to be shown in help.
#[derive(Debug, Clone)]
pub struct ResolvedBinding {
    pub action: Action,
    pub context: KeyContext,
    pub sequences: Vec<KeySequence>,
    pub description: &'static str,
}

/// The resolved keymap: what every key does, in every context.
#[derive(Debug, Clone, Default)]
pub struct Keymap {
    /// Complete bindings, keyed by context and sequence.
    bindings: HashMap<(KeyContext, KeySequence), Action>,
    /// Every proper prefix of a bound sequence, so a partial sequence can be
    /// distinguished from an unbound key without scanning every binding.
    prefixes: HashMap<(KeyContext, KeySequence), ()>,
    /// Bindings in table order, for the help overlay and the docs.
    entries: Vec<ResolvedBinding>,
    /// Chords typed so far that have not yet completed a binding.
    pending: Vec<KeyChord>,
    /// Warnings raised while building the keymap, surfaced in the status bar.
    warnings: Vec<String>,
}

impl Keymap {
    /// Build the keymap from the built-in defaults.
    ///
    /// # Panics
    ///
    /// Panics if a default binding string does not parse. That is a mistake in
    /// this repository, caught by the tests below, never something a user can
    /// trigger; user remaps are handled by [`Keymap::apply_remaps`], which
    /// warns instead.
    pub fn defaults() -> Self {
        let mut keymap = Keymap::default();

        for spec in default_bindings() {
            let sequences = spec
                .keys
                .iter()
                .map(|key| {
                    KeySequence::parse(key).unwrap_or_else(|e| {
                        panic!(
                            "default binding `{key}` for {:?} must parse: {e}",
                            spec.action
                        )
                    })
                })
                .collect::<Vec<_>>();

            keymap.entries.push(ResolvedBinding {
                action: spec.action.clone(),
                context: spec.context,
                sequences: sequences.clone(),
                description: spec.description,
            });

            for sequence in sequences {
                keymap.insert(spec.context, sequence, spec.action.clone());
            }
        }

        keymap
    }

    /// Record one binding and every proper prefix of it.
    fn insert(&mut self, context: KeyContext, sequence: KeySequence, action: Action) {
        for len in 1..sequence.0.len() {
            let prefix = KeySequence(sequence.0[..len].to_vec());
            self.prefixes.insert((context, prefix), ());
        }
        self.bindings.insert((context, sequence), action);
    }

    /// Warnings raised while building the keymap.
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }

    /// Every binding in table order, for the help overlay and the docs.
    pub fn entries(&self) -> &[ResolvedBinding] {
        &self.entries
    }

    /// The first binding for an action, formatted for display.
    ///
    /// Used wherever the UI shows a key to the user — the status bar hints and
    /// the welcome screen's onboarding block — so those reflect a remap for the
    /// same reason the help overlay does.
    pub fn binding_for(&self, action: &Action) -> Option<String> {
        self.entries
            .iter()
            .find(|e| &e.action == action)
            .and_then(|e| e.sequences.first())
            .map(|s| s.to_string())
    }

    /// The chords typed so far that have not completed a binding.
    pub fn pending(&self) -> &[KeyChord] {
        &self.pending
    }

    /// Abandon a partially typed sequence.
    pub fn clear_pending(&mut self) {
        self.pending.clear();
    }

    /// Look up one complete sequence, trying `context` before `Global`.
    fn lookup(&self, context: KeyContext, sequence: &KeySequence) -> Option<&Action> {
        self.bindings
            .get(&(context, sequence.clone()))
            .or_else(|| self.bindings.get(&(KeyContext::Global, sequence.clone())))
    }

    /// Whether `sequence` is a proper prefix of some binding in `context` or in
    /// `Global`.
    fn is_prefix(&self, context: KeyContext, sequence: &KeySequence) -> bool {
        self.prefixes.contains_key(&(context, sequence.clone()))
            || self
                .prefixes
                .contains_key(&(KeyContext::Global, sequence.clone()))
    }

    /// Feed one key press to the keymap.
    ///
    /// There is no sequence timeout: a pending sequence completes on the next
    /// chord, or is abandoned by a chord that does not continue it — and that
    /// chord is then reconsidered on its own, so nothing is swallowed. This is
    /// why prefix keys such as `g` and `m` have no standalone meaning.
    pub fn resolve(&mut self, context: KeyContext, chord: KeyChord) -> Resolution {
        self.pending.push(chord);
        let candidate = KeySequence(self.pending.clone());

        if let Some(action) = self.lookup(context, &candidate) {
            let action = action.clone();
            self.pending.clear();
            return Resolution::Action(action);
        }

        if self.is_prefix(context, &candidate) {
            return Resolution::Pending(format!("{candidate}…"));
        }

        // The sequence is dead. If more than one chord had accumulated, the
        // latest chord may still mean something on its own, so try it alone.
        let restart = self.pending.len() > 1;
        self.pending.clear();

        if restart {
            return self.resolve(context, chord);
        }

        Resolution::Unbound
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chord(s: &str) -> KeyChord {
        KeyChord::parse(s).unwrap()
    }

    #[test]
    fn parses_plain_characters() {
        assert_eq!(
            chord("q"),
            KeyChord {
                code: KeyCode::Char('q'),
                modifiers: KeyModifiers::NONE
            }
        );
        assert_eq!(
            chord("?"),
            KeyChord {
                code: KeyCode::Char('?'),
                modifiers: KeyModifiers::NONE
            }
        );
        assert_eq!(
            chord("/"),
            KeyChord {
                code: KeyCode::Char('/'),
                modifiers: KeyModifiers::NONE
            }
        );
    }

    #[test]
    fn parses_modifiers_in_any_supported_order() {
        assert_eq!(
            chord("ctrl+shift+f"),
            KeyChord {
                code: KeyCode::Char('f'),
                modifiers: KeyModifiers::CONTROL | KeyModifiers::SHIFT,
            }
        );
        assert_eq!(
            chord("alt+left"),
            KeyChord {
                code: KeyCode::Left,
                modifiers: KeyModifiers::ALT,
            }
        );
    }

    #[test]
    fn parses_named_and_function_keys() {
        assert_eq!(chord("enter").code, KeyCode::Enter);
        assert_eq!(chord("backtab").code, KeyCode::BackTab);
        assert_eq!(chord("space").code, KeyCode::Char(' '));
        assert_eq!(chord("pagedown").code, KeyCode::PageDown);
        assert_eq!(chord("f12").code, KeyCode::F(12));
    }

    #[test]
    fn plus_is_bindable_as_a_character() {
        assert_eq!(
            chord("+"),
            KeyChord {
                code: KeyCode::Char('+'),
                modifiers: KeyModifiers::NONE
            }
        );
        assert_eq!(
            chord("ctrl++"),
            KeyChord {
                code: KeyCode::Char('+'),
                modifiers: KeyModifiers::CONTROL
            }
        );
    }

    #[test]
    fn uppercase_and_shift_spellings_agree() {
        assert_eq!(chord("G"), chord("shift+g"));
        assert_eq!(chord("G").modifiers, KeyModifiers::NONE);
    }

    #[test]
    fn rejects_a_run_together_sequence() {
        // The mistake this error exists to catch.
        assert_eq!(
            KeySequence::parse("gt"),
            Err(KeyParseError::UnknownKey("gt".to_string()))
        );
    }

    #[test]
    fn rejects_empty_and_unknown_keys() {
        assert_eq!(KeySequence::parse("   "), Err(KeyParseError::Empty));
        assert_eq!(
            KeySequence::parse("hyper+q"),
            Err(KeyParseError::UnknownKey("hyper+q".to_string()))
        );
        assert_eq!(
            KeyChord::parse("ctrl+ctrl+a"),
            Err(KeyParseError::DuplicateModifier("ctrl+ctrl+a".to_string()))
        );
    }

    #[test]
    fn parses_sequences() {
        let seq = KeySequence::parse("g t").unwrap();
        assert_eq!(seq.0, vec![chord("g"), chord("t")]);
        assert_eq!(seq.to_string(), "g t");
    }

    #[test]
    fn normalises_shift_on_plain_characters() {
        let from_terminal =
            KeyChord::from_event(KeyEvent::new(KeyCode::Char('G'), KeyModifiers::SHIFT));
        assert_eq!(from_terminal, chord("G"));
    }

    #[test]
    fn keeps_shift_on_control_characters() {
        let from_terminal = KeyChord::from_event(KeyEvent::new(
            KeyCode::Char('F'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ));
        assert_eq!(from_terminal, chord("ctrl+shift+f"));
        assert_ne!(from_terminal, chord("ctrl+f"));
    }

    #[test]
    fn every_default_binding_parses() {
        // `Keymap::defaults` panics on a bad binding string; this is the test
        // that makes that panic a build-time guarantee.
        let keymap = Keymap::defaults();
        assert!(keymap.warnings().is_empty());
        assert!(!keymap.entries().is_empty());
    }

    #[test]
    fn binding_for_returns_the_primary_spelling() {
        let keymap = Keymap::defaults();
        assert_eq!(
            keymap.binding_for(&Action::ToggleSidebar).as_deref(),
            Some("Ctrl+B")
        );
        assert_eq!(
            keymap.binding_for(&Action::ScrollTop).as_deref(),
            Some("g g")
        );
    }

    #[test]
    fn resolves_a_single_chord() {
        let mut keymap = Keymap::defaults();
        assert_eq!(
            keymap.resolve(KeyContext::Viewport, chord("q")),
            Resolution::Action(Action::Quit)
        );
    }

    #[test]
    fn context_shadows_global() {
        let mut keymap = Keymap::defaults();
        assert_eq!(
            keymap.resolve(KeyContext::Viewport, chord("j")),
            Resolution::Action(Action::ScrollLineDown)
        );
        assert_eq!(
            keymap.resolve(KeyContext::Sidebar, chord("j")),
            Resolution::Action(Action::SidebarDown)
        );
    }

    #[test]
    fn global_applies_where_no_context_binding_exists() {
        let mut keymap = Keymap::defaults();
        assert_eq!(
            keymap.resolve(KeyContext::Sidebar, chord("ctrl+t")),
            Resolution::Action(Action::NewTab)
        );
    }

    #[test]
    fn resolves_a_sequence_through_a_pending_state() {
        let mut keymap = Keymap::defaults();

        assert!(matches!(
            keymap.resolve(KeyContext::Viewport, chord("g")),
            Resolution::Pending(_)
        ));
        assert_eq!(keymap.pending().len(), 1);

        assert_eq!(
            keymap.resolve(KeyContext::Viewport, chord("g")),
            Resolution::Action(Action::ScrollTop)
        );
        assert!(keymap.pending().is_empty());
    }

    #[test]
    fn a_prefix_key_has_no_standalone_meaning() {
        let mut keymap = Keymap::defaults();
        // `g` alone never acts; it only ever opens a sequence.
        assert!(matches!(
            keymap.resolve(KeyContext::Viewport, chord("g")),
            Resolution::Pending(_)
        ));
    }

    #[test]
    fn an_abandoning_key_is_processed_on_its_own() {
        let mut keymap = Keymap::defaults();

        assert!(matches!(
            keymap.resolve(KeyContext::Viewport, chord("g")),
            Resolution::Pending(_)
        ));
        // `g` `q` is not a binding, so `q` is reconsidered alone and quits.
        assert_eq!(
            keymap.resolve(KeyContext::Viewport, chord("q")),
            Resolution::Action(Action::Quit)
        );
        assert!(keymap.pending().is_empty());
    }

    #[test]
    fn an_abandoning_key_that_means_nothing_is_unbound() {
        let mut keymap = Keymap::defaults();
        keymap.resolve(KeyContext::Viewport, chord("g"));
        assert_eq!(
            keymap.resolve(KeyContext::Viewport, chord("Z")),
            Resolution::Unbound
        );
        assert!(keymap.pending().is_empty());
    }

    #[test]
    fn sequence_case_is_significant() {
        let mut keymap = Keymap::defaults();
        keymap.resolve(KeyContext::Viewport, chord("g"));
        assert_eq!(
            keymap.resolve(KeyContext::Viewport, chord("t")),
            Resolution::Action(Action::NextTab)
        );
        keymap.resolve(KeyContext::Viewport, chord("g"));
        assert_eq!(
            keymap.resolve(KeyContext::Viewport, chord("T")),
            Resolution::Action(Action::PrevTab)
        );
    }

    #[test]
    fn every_enhanced_binding_has_a_plain_fallback() {
        // Section 8.6: nothing may be reachable only through the kitty
        // keyboard protocol. A binding is "enhanced" when it needs Ctrl+Enter,
        // Shift+Enter, a Ctrl+Shift combination, Alt+arrow, Alt+digit, or
        // Ctrl+Page{Up,Down}; each such action must also carry a binding that
        // any terminal can deliver.
        fn needs_enhancement(seq: &KeySequence) -> bool {
            seq.0.iter().any(|c| {
                let ctrl = c.modifiers.contains(KeyModifiers::CONTROL);
                let alt = c.modifiers.contains(KeyModifiers::ALT);
                let shift = c.modifiers.contains(KeyModifiers::SHIFT);
                match c.code {
                    KeyCode::Enter => ctrl || shift,
                    KeyCode::PageUp | KeyCode::PageDown => ctrl,
                    KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => alt,
                    KeyCode::Char(ch) => (ctrl && shift) || (alt && ch.is_ascii_digit()),
                    _ => false,
                }
            })
        }

        for entry in Keymap::defaults().entries() {
            if entry.sequences.iter().any(needs_enhancement) {
                assert!(
                    entry.sequences.iter().any(|s| !needs_enhancement(s)),
                    "{:?} is only reachable through the kitty keyboard protocol",
                    entry.action
                );
            }
        }
    }
}
