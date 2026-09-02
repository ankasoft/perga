//! The `perga` binary.
//!
//! This owns process startup: argument parsing, the non-TUI outputs
//! (`--generate-man`, `--generate-completions`), terminal setup and teardown,
//! and the panic hook that guarantees the terminal is restored on every exit
//! path.

use std::io::{self, Write};
use std::process::ExitCode;
use std::thread;

use anyhow::Context;
use clap::{CommandFactory, Parser};
use clap_complete::Shell as CompleteShell;
use crossbeam_channel::{unbounded, Sender};

use perga::app::{self, App, Message};
use perga::cli::{Cli, Shell};
use perga::config::keymap::Keymap;
use perga::config::schema::UiConfig;
use perga::terminal;
use perga::theme::Theme;

/// Exit code for a runtime error.
const EXIT_RUNTIME: u8 = 1;
/// Exit code for a usage or argument error.
const EXIT_USAGE: u8 = 2;

fn main() -> ExitCode {
    let cli = Cli::parse();

    // Anything that writes to stdout and exits happens before the terminal is
    // touched, so `perga --generate-man > perga.1` never emits an escape code.
    match generate(&cli) {
        Ok(true) => return ExitCode::SUCCESS,
        Ok(false) => {}
        Err(err) => {
            eprintln!("perga: {err:#}");
            return ExitCode::from(EXIT_USAGE);
        }
    }

    terminal::install_panic_hook();

    match run(&cli) {
        Ok(code) => ExitCode::from(code),
        Err(err) => {
            // Belt and braces: `run` restores on the paths it knows about, and
            // this catches the ones it does not. `restore` is idempotent.
            let _ = terminal::restore();
            eprintln!("perga: {err:#}");
            ExitCode::from(EXIT_RUNTIME)
        }
    }
}

/// Handle the flags that print something and exit.
///
/// Returns whether one of them fired.
fn generate(cli: &Cli) -> anyhow::Result<bool> {
    if cli.generate_man {
        let man = clap_mangen::Man::new(Cli::command());
        let mut out = io::stdout().lock();
        man.render(&mut out).context("rendering the man page")?;
        out.flush()?;
        return Ok(true);
    }

    if let Some(shell) = cli.generate_completions {
        let shell = match shell {
            Shell::Bash => CompleteShell::Bash,
            Shell::Zsh => CompleteShell::Zsh,
            Shell::Fish => CompleteShell::Fish,
        };
        let mut cmd = Cli::command();
        let name = cmd.get_name().to_string();
        clap_complete::generate(shell, &mut cmd, name, &mut io::stdout().lock());
        return Ok(true);
    }

    Ok(false)
}

/// Set up the terminal, run the event loop, and tear the terminal down again.
fn run(cli: &Cli) -> anyhow::Result<u8> {
    let mut ui_config = UiConfig::default();
    if cli.no_sidebar {
        ui_config.sidebar_visible = false;
    }
    if cli.no_mouse {
        ui_config.mouse = false;
    }

    let mut theme = Theme::dark();
    if no_color() {
        theme.strip_colors();
    }

    let mut app = App::new(theme, Keymap::defaults(), ui_config);

    let (tx, rx) = unbounded();
    spawn_input_thread(tx.clone());
    #[cfg(unix)]
    spawn_signal_thread(tx.clone());
    drop(tx);

    let mut term = terminal::setup(app.mouse_capture).context("setting up the terminal")?;

    if cli.debug_panic {
        panic!("--debug-panic: the terminal must be readable below this line");
    }

    let result = app::run(&mut term, &mut app, &rx);

    // Restore before propagating, so an error message is not swallowed by the
    // alternate screen.
    let restored = terminal::restore();
    result?;
    restored.context("restoring the terminal")?;

    Ok(app.exit_code)
}

/// Whether `NO_COLOR` is set and non-empty, per <https://no-color.org>.
fn no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())
}

/// Read terminal input on its own thread.
///
/// The main loop then does one blocking `recv` and nothing else: no
/// `poll(timeout)`, no `try_recv` spin. That is what makes the 0% idle CPU
/// target hold by construction.
fn spawn_input_thread(tx: Sender<Message>) {
    thread::Builder::new()
        .name("perga-input".to_string())
        .spawn(move || {
            // A read error means the terminal is gone, and a send error means
            // the main loop has exited; either way there is nothing to do.
            while let Ok(event) = crossterm::event::read() {
                if tx.send(Message::Input(event)).is_err() {
                    break;
                }
            }
        })
        .expect("spawning the input thread");
}

/// Watch for the signals that must not leave the terminal broken.
#[cfg(unix)]
fn spawn_signal_thread(tx: Sender<Message>) {
    use signal_hook::consts::{SIGCONT, SIGHUP, SIGINT, SIGTERM, SIGTSTP};
    use signal_hook::iterator::Signals;

    let signals = Signals::new([SIGINT, SIGTERM, SIGHUP, SIGTSTP, SIGCONT]);

    let mut signals = match signals {
        Ok(signals) => signals,
        Err(e) => {
            // Not fatal: the panic hook and the normal exit path still restore
            // the terminal. Only `kill` handling is lost.
            tracing::warn!("cannot install signal handlers: {e}");
            return;
        }
    };

    thread::Builder::new()
        .name("perga-signals".to_string())
        .spawn(move || {
            for signal in &mut signals {
                if tx.send(Message::Signal(signal)).is_err() {
                    break;
                }
            }
        })
        .expect("spawning the signal thread");
}
