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
use perga::config::Config;
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

    if cli.generate_config {
        print!("{}", perga::config::DEFAULT_CONFIG);
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

    if cli.check_config {
        return check_config(cli).map(|()| true);
    }

    Ok(false)
}

/// Validate the configuration and the theme, print what is wrong, and exit.
///
/// Exits `0` whatever it finds: the configuration always loads, and the
/// warnings are the output rather than a failure. A caller that wants a
/// non-zero exit on a warning can count the lines.
fn check_config(cli: &Cli) -> anyhow::Result<()> {
    let root = cli
        .path
        .clone()
        .filter(|path| path.is_dir())
        .unwrap_or_else(|| PathBuf::from("."));

    let (_, warnings) = configure(cli, &root);

    match perga::config::user_config_path() {
        Some(path) if path.exists() => println!("config: {}", path.display()),
        Some(path) => println!("config: {} (not present; using defaults)", path.display()),
        None => println!("config: no configuration directory on this platform"),
    }

    let local = root.join(".perga.toml");
    if local.exists() {
        println!("vault config: {}", local.display());
    }

    if warnings.is_empty() {
        println!("no problems found");
        return Ok(());
    }

    println!("\n{} problems:", warnings.len());
    for warning in &warnings {
        println!("  {warning}");
    }

    Ok(())
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

    // Section 9.12: the theme applies in print mode. It is resolved from the
    // same five layers the TUI uses, with the document's own directory as the
    // vault root: that is where a `.perga.toml` beside it would be.
    let root = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));

    let mut config = Config::load(root, cli.config.as_deref(), cli.no_config);
    if let Some(name) = &cli.theme {
        config.theme.name.clone_from(name);
    }
    if let Some(wrap) = cli.wrap {
        config.general.wrap = wrap;
    }

    let mut warnings = std::mem::take(&mut config.warnings);
    let theme = resolve_theme(&config, &mut warnings);

    // Warnings go to stderr: stdout is the document, and a reader piping it
    // into a file does not want a configuration warning in the middle of it.
    for warning in &warnings {
        eprintln!("perga: {warning}");
    }

    let options = print::PrintOptions {
        width: print_width(cli, config.general.wrap),
        colour: !no_color(),
        heading_markers: config.ui.show_heading_markers,
    };

    let mut out = io::stdout().lock();

    match print::print(&document, &theme, options, &mut out) {
        Ok(()) => Ok(true),
        // `perga note.md | head` and a reader quitting `less` early both close
        // the pipe mid-write. Every well-behaved Unix tool treats that as the
        // end of the job, not as a failure: no message, exit zero.
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => Ok(true),
        Err(e) => Err((
            anyhow::Error::new(e).context("writing to stdout"),
            EXIT_RUNTIME,
        )),
    }
}

/// The width print mode renders at.
///
/// A hard wrap wins, whether it came from `--wrap` or from `general.wrap`.
/// Otherwise the terminal's width when stdout is a TTY, otherwise 80 columns.
fn print_width(cli: &Cli, configured: u16) -> u16 {
    if let Some(wrap) = cli.wrap.or(Some(configured)).filter(|w| *w > 0) {
        return wrap;
    }

    if std::io::stdout().is_terminal() {
        if let Ok((width, _)) = crossterm::terminal::size() {
            return width.max(1);
        }
    }

    print::DEFAULT_WIDTH
}

/// Assemble the configuration and everything derived from it.
///
/// Returns the application and every warning raised on the way, which the
/// status bar shows and `--check-config` prints.
fn configure(cli: &Cli, root: &Path) -> (App, Vec<String>) {
    let mut config = Config::load(root, cli.config.as_deref(), cli.no_config);
    let mut warnings = std::mem::take(&mut config.warnings);

    // Layer five: individual flags, which override every file.
    if cli.no_sidebar {
        config.ui.sidebar_visible = false;
    }
    if cli.no_mouse {
        config.ui.mouse = false;
    }
    if let Some(mode) = cli.sidebar {
        config.ui.sidebar_default_mode = match mode {
            SidebarModeArg::Files => SidebarMode::Files,
            SidebarModeArg::Search => SidebarMode::Search,
            SidebarModeArg::Outline => SidebarMode::Outline,
            SidebarModeArg::Links => SidebarMode::Links,
        };
    }
    if cli.all {
        config.files.show_all = true;
    }
    if cli.no_gitignore {
        config.files.respect_gitignore = false;
    }
    let theme_pinned = cli.theme.is_some();
    if let Some(name) = &cli.theme {
        config.theme.name.clone_from(name);
    }
    if let Some(wrap) = cli.wrap {
        config.general.wrap = wrap;
    }

    let theme = resolve_theme(&config, &mut warnings);
    let keymap = Keymap::with_overrides(&config.keys);
    warnings.extend(keymap.warnings().iter().cloned());

    let mut app = App::new(theme, keymap, config.ui.clone(), config.files.clone());
    app.general = config.general;
    app.wikilinks = config.wikilinks;
    app.search_config = config.search;
    app.editor_config = config.editor;
    app.watch_config = config.watch;
    app.session_config = config.session;
    app.theme_config = config.theme;
    app.theme_pinned = theme_pinned;
    app.set_vault_root(root);

    (app, warnings)
}

/// Resolve the configured theme, degrading it to what the terminal can show.
fn resolve_theme(config: &Config, warnings: &mut Vec<String>) -> Theme {
    let dir = if config.theme.dir.as_os_str().is_empty() {
        perga::config::user_theme_dir()
    } else {
        Some(config.theme.dir.clone())
    };

    let mut theme = Theme::resolve(&config.theme.name, dir.as_deref(), warnings);

    if theme.code_theme.is_none() {
        theme.code_theme = Some(config.theme.code_theme.clone());
    }

    // Order matters: degrade first, then strip. `NO_COLOR` wins over
    // everything, and degrading a stripped theme would have nothing to do.
    if !perga::theme::truecolor() {
        theme.degrade_to_256();
    }
    if no_color() {
        theme.strip_colors();
    }

    theme
}

/// Set up the terminal, run the event loop, and tear the terminal down again.
fn run(cli: &Cli) -> anyhow::Result<u8> {
    let target = Target::resolve(cli.path.as_deref())?;

    let root = match &target {
        Target::Vault(root) => root.clone(),
        Target::File { root, .. } => root.clone(),
    };

    let (mut app, warnings) = configure(cli, &root);

    if let Some(first) = warnings.first() {
        app.status.set(
            if warnings.len() == 1 {
                first.clone()
            } else {
                format!("{first} (and {} more; --check-config)", warnings.len() - 1)
            },
            perga::app::Severity::Warning,
        );
    }

    match &target {
        Target::Vault(_) => {
            // Section 9.10: a session is restored only when perga was opened
            // with no path; a named file is what the reader asked for.
            if cli.path.is_none() && !cli.no_session {
                app.restore_session();
            }
        }
        Target::File { path, .. } => {
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

    let watch_tx = tx.clone();
    app.start_watch(move |event| {
        let _ = watch_tx.send(Message::Watch(event));
    });

    // The theme directory is watched too, so writing a theme shows its effect
    // without restarting. Any change there means the same thing: re-read.
    let theme_tx = tx.clone();
    app.start_theme_watch(move |event| {
        if matches!(event, perga::vault::watch::WatchEvent::Stopped(_)) {
            return;
        }
        let _ = theme_tx.send(Message::ThemeChanged);
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

    if !cli.no_session {
        app.save_session();
    }

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
