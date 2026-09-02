//! The `perga` binary.
//!
//! This owns process startup: argument parsing, the non-TUI outputs
//! (`--generate-man`, `--generate-completions`), terminal setup and teardown,
//! and the panic hook that guarantees the terminal is restored on every exit
//! path.

use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;

use anyhow::Context;
use clap::{CommandFactory, Parser};
use clap_complete::Shell as CompleteShell;
use crossbeam_channel::{unbounded, Sender};

use perga::app::{self, App, Message};
use perga::cli::{Cli, Shell, SidebarModeArg};
use perga::config::keymap::Keymap;
use perga::config::schema::{FilesConfig, UiConfig};
use perga::doc::document::Document;
use perga::doc::print;
use perga::terminal;
use perga::theme::Theme;
use perga::ui::sidebar::SidebarMode;

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

    // Print mode never touches the terminal, so it is settled before the panic
    // hook that exists to put the terminal back.
    match print_mode(&cli) {
        Ok(true) => return ExitCode::SUCCESS,
        Ok(false) => {}
        Err((err, code)) => {
            eprintln!("perga: {err:#}");
            return ExitCode::from(code);
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

/// What perga was asked to open.
enum Target {
    /// A directory, opened as the vault root.
    Vault(PathBuf),
    /// A file, opened with its parent directory as the vault root.
    File { path: PathBuf, root: PathBuf },
}

impl Target {
    /// Resolve the `PATH` argument.
    fn resolve(path: Option<&Path>) -> anyhow::Result<Self> {
        let path = path.unwrap_or(Path::new("."));

        let metadata =
            std::fs::metadata(path).with_context(|| format!("cannot open `{}`", path.display()))?;

        if metadata.is_dir() {
            return Ok(Target::Vault(path.to_path_buf()));
        }

        let root = path
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(Path::new("."))
            .to_path_buf();

        Ok(Target::File {
            path: path.to_path_buf(),
            root,
        })
    }
}

/// Render to stdout and exit, when asked to or when stdout is not a terminal.
///
/// Returns whether print mode fired. The error carries its own exit code,
/// because a directory in print mode is a usage error rather than a runtime one.
fn print_mode(cli: &Cli) -> Result<bool, (anyhow::Error, u8)> {
    let implied = !std::io::stdout().is_terminal();
    if !cli.print && !implied {
        return Ok(false);
    }

    let target = Target::resolve(cli.path.as_deref()).map_err(|e| (e, EXIT_USAGE))?;

    let path = match target {
        Target::File { path, .. } => path,
        Target::Vault(path) => {
            // There is nothing to print for a directory, and silently picking a
            // file out of it would be a guess.
            let err = anyhow::anyhow!(
                "`{}` is a directory; print mode needs a file",
                path.display()
            );
            return Err((err, EXIT_USAGE));
        }
    };

    let document = Document::load(&path)
        .with_context(|| format!("cannot read `{}`", path.display()))
        .map_err(|e| (e, EXIT_USAGE))?;

    let mut theme = Theme::dark();
    let colour = !no_color();
    if !colour {
        theme.strip_colors();
    }

    let width = print_width(cli);
    let mut out = io::stdout().lock();

    print::print(&document, &theme, width, colour, &mut out)
        .context("writing to stdout")
        .map_err(|e| (e, EXIT_RUNTIME))?;

    Ok(true)
}

/// The width print mode renders at.
///
/// The terminal's when stdout is a TTY, otherwise `--wrap` if it was given,
/// otherwise 80 columns.
fn print_width(cli: &Cli) -> u16 {
    if let Some(wrap) = cli.wrap.filter(|w| *w > 0) {
        return wrap;
    }

    if std::io::stdout().is_terminal() {
        if let Ok((width, _)) = crossterm::terminal::size() {
            return width.max(1);
        }
    }

    print::DEFAULT_WIDTH
}

/// Set up the terminal, run the event loop, and tear the terminal down again.
fn run(cli: &Cli) -> anyhow::Result<u8> {
    let target = Target::resolve(cli.path.as_deref())?;

    let mut ui_config = UiConfig::default();
    if cli.no_sidebar {
        ui_config.sidebar_visible = false;
    }
    if cli.no_mouse {
        ui_config.mouse = false;
    }
    if let Some(mode) = cli.sidebar {
        ui_config.sidebar_default_mode = match mode {
            SidebarModeArg::Files => SidebarMode::Files,
            SidebarModeArg::Search => SidebarMode::Search,
            SidebarModeArg::Outline => SidebarMode::Outline,
            SidebarModeArg::Links => SidebarMode::Links,
        };
    }

    let mut files_config = FilesConfig::default();
    if cli.all {
        files_config.show_all = true;
    }
    if cli.no_gitignore {
        files_config.respect_gitignore = false;
    }

    let mut theme = Theme::dark();
    if no_color() {
        theme.strip_colors();
    }

    let mut app = App::new(theme, Keymap::defaults(), ui_config, files_config);

    match &target {
        Target::Vault(root) => app.set_vault_root(root),
        Target::File { path, root } => {
            app.set_vault_root(root);
            let document = Document::load(path)
                .with_context(|| format!("cannot read `{}`", path.display()))?;
            app.open(document);
        }
    }

    let (tx, rx) = unbounded();
    spawn_input_thread(tx.clone());

    // The walk streams into the same channel as everything else, so the first
    // frame is painted from an empty tree and the tree fills in behind it.
    let walk_tx = tx.clone();
    app.start_walk(move |event| {
        let _ = walk_tx.send(Message::Walk(event));
    });

    // The index build starts when the walk finishes and has said which files
    // the cache does not already cover.
    let index_tx = tx.clone();
    app.on_index(move |event| {
        let _ = index_tx.send(Message::Index(event));
    });

    let search_tx = tx.clone();
    app.on_search(move |event| {
        let _ = search_tx.send(Message::Search(event));
    });

    #[cfg(unix)]
    spawn_signal_thread(tx.clone());

    // Loading the syntax sets costs 50 to 100 ms, which is the whole
    // first-frame budget, so it happens on its own thread and the code blocks
    // drawn plain in the meantime are re-rendered when it lands.
    let ready = tx.clone();
    app.highlighter.spawn_load(move || {
        let _ = ready.send(Message::SyntaxReady);
    });

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
