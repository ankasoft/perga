//! Terminal setup, teardown, and the guarantees that the terminal is restored
//! on every exit path.
//!
//! Leaving a user's shell in raw mode with the alternate screen still active is
//! the worst thing a TUI can do, so restoration is driven by a process-global
//! record of what was actually enabled rather than by a value someone has to
//! remember to drop. [`restore`] is idempotent and safe to call from a panic
//! hook or a signal handler thread.
//!
//! `panic = "abort"` in the release profile does not defeat this: the panic
//! hook runs before the abort.

use std::io::{self, Stdout, Write};
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use crossterm::{cursor, execute};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

/// The terminal type the whole application renders into.
pub type Tui = Terminal<CrosstermBackend<Stdout>>;

/// What is currently enabled, so [`restore`] can undo exactly that much.
struct TerminalState {
    raw_mode: AtomicBool,
    alternate_screen: AtomicBool,
    mouse_capture: AtomicBool,
    bracketed_paste: AtomicBool,
    keyboard_enhancement: AtomicBool,
}

static STATE: TerminalState = TerminalState {
    raw_mode: AtomicBool::new(false),
    alternate_screen: AtomicBool::new(false),
    mouse_capture: AtomicBool::new(false),
    bracketed_paste: AtomicBool::new(false),
    keyboard_enhancement: AtomicBool::new(false),
};

/// Whether the terminal reported support for the kitty keyboard protocol.
///
/// Bindings that depend on it (`Ctrl+Enter`, `Shift+Enter`, `Ctrl+Shift+F`)
/// all have plain fallbacks that are always bound, so this only ever affects
/// which spelling of a binding a given terminal can deliver.
pub fn keyboard_enhancement_active() -> bool {
    STATE.keyboard_enhancement.load(Ordering::SeqCst)
}

/// Whether mouse capture is currently on.
pub fn mouse_capture_active() -> bool {
    STATE.mouse_capture.load(Ordering::SeqCst)
}

/// Enter raw mode and the alternate screen and return a terminal to draw into.
///
/// If any step fails, everything already enabled is undone before the error is
/// returned: a half-set-up terminal is exactly as broken as a half-torn-down
/// one, and the caller has no way to clean up what it cannot see.
pub fn setup(mouse: bool) -> io::Result<Tui> {
    match try_setup(mouse) {
        Ok(terminal) => Ok(terminal),
        Err(e) => {
            let _ = restore();
            Err(e)
        }
    }
}

fn try_setup(mouse: bool) -> io::Result<Tui> {
    let mut stdout = io::stdout();

    enable_raw_mode()?;
    STATE.raw_mode.store(true, Ordering::SeqCst);

    execute!(stdout, EnterAlternateScreen)?;
    STATE.alternate_screen.store(true, Ordering::SeqCst);

    // Bracketed paste turns a pasted block into one `Event::Paste` rather than
    // hundreds of key events. Without it a paste into the editor is slow and
    // lands as hundreds of undo steps.
    execute!(stdout, EnableBracketedPaste)?;
    STATE.bracketed_paste.store(true, Ordering::SeqCst);

    if mouse {
        set_mouse_capture(true)?;
    }

    // Guarded: on a terminal without the kitty keyboard protocol, pushing the
    // flags leaves an unanswered escape sequence in the input stream.
    if crossterm::terminal::supports_keyboard_enhancement().unwrap_or(false) {
        execute!(
            stdout,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_ALTERNATE_KEYS
            )
        )?;
        STATE.keyboard_enhancement.store(true, Ordering::SeqCst);
    }

    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
    terminal.hide_cursor()?;

    // Deliberately no `Terminal::clear()` here. It reads the cursor position
    // back from the terminal, which is a round trip that costs a chunk of the
    // 50 ms first-frame budget and fails outright on a terminal that does not
    // answer. Entering the alternate screen already gives a blank screen, and
    // the first draw paints all of it.

    Ok(terminal)
}

/// Turn mouse capture on or off at runtime.
///
/// With capture on, the terminal's own text selection is unavailable; most
/// terminals let the user bypass capture with `Shift`+drag.
pub fn set_mouse_capture(enabled: bool) -> io::Result<()> {
    let mut stdout = io::stdout();
    if enabled {
        execute!(stdout, EnableMouseCapture)?;
    } else {
        execute!(stdout, DisableMouseCapture)?;
    }
    STATE.mouse_capture.store(enabled, Ordering::SeqCst);
    Ok(())
}

/// Undo everything [`setup`] did, in reverse order.
///
/// Idempotent, and every step is attempted even if an earlier one fails: a
/// half-restored terminal is worse than a slightly noisy one.
pub fn restore() -> io::Result<()> {
    let mut stdout = io::stdout();
    let mut first_error = None;

    let mut attempt = |result: io::Result<()>| {
        if let Err(e) = result {
            first_error.get_or_insert(e);
        }
    };

    if STATE.keyboard_enhancement.swap(false, Ordering::SeqCst) {
        attempt(execute!(stdout, PopKeyboardEnhancementFlags));
    }
    if STATE.mouse_capture.swap(false, Ordering::SeqCst) {
        attempt(execute!(stdout, DisableMouseCapture));
    }
    if STATE.bracketed_paste.swap(false, Ordering::SeqCst) {
        attempt(execute!(stdout, DisableBracketedPaste));
    }
    if STATE.alternate_screen.swap(false, Ordering::SeqCst) {
        attempt(execute!(stdout, LeaveAlternateScreen));
    }
    if STATE.raw_mode.swap(false, Ordering::SeqCst) {
        attempt(disable_raw_mode());
    }

    attempt(execute!(stdout, cursor::Show));
    attempt(stdout.flush());

    match first_error {
        Some(e) => Err(e),
        None => Ok(()),
    }
}

/// Install a panic hook that restores the terminal before the panic is printed.
///
/// Without this the panic message scrolls past inside the alternate screen and
/// vanishes when the screen is torn down, leaving a broken terminal and no
/// explanation.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = restore();
        previous(info);
    }));
}

/// Put text on the system clipboard with OSC 52.
///
/// The one way a terminal application can reach the clipboard of the machine
/// the *terminal* is running on, which over SSH is not the machine perga is.
/// Not every emulator honours it (several disable it by default as a security
/// measure) and there is no reply to wait for, so the caller says what it
/// tried rather than what happened.
pub fn copy_to_clipboard(text: &str) -> std::io::Result<()> {
    use base64_encode as encode;
    use std::io::Write as _;

    let mut out = std::io::stdout().lock();
    write!(out, "\x1b]52;c;{}\x07", encode(text.as_bytes()))?;
    out.flush()
}

/// Base64, as OSC 52 requires.
///
/// Twenty lines rather than a dependency: this is the only place in perga that
/// needs it, and the alphabet has not changed since 1987.
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);

    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let triple = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);

        for i in 0..4 {
            if i <= chunk.len() {
                let index = (triple >> (18 - 6 * i)) & 0x3f;
                out.push(ALPHABET[index as usize] as char);
            } else {
                out.push('=');
            }
        }
    }

    out
}

#[cfg(test)]
mod clipboard_tests {
    use super::base64_encode;

    #[test]
    fn base64_matches_the_standard_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");
    }

    #[test]
    fn base64_handles_non_ascii() {
        assert_eq!(base64_encode("ışık".as_bytes()), "xLHFn8Sxaw==");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_is_idempotent_when_nothing_was_set_up() {
        // Nothing was enabled, so nothing is undone and no terminal is needed.
        assert!(!STATE.raw_mode.load(Ordering::SeqCst));
        assert!(restore().is_ok());
        assert!(restore().is_ok());
    }
}
