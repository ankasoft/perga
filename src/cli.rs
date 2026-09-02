//! Command-line argument definitions.

use std::path::PathBuf;

use clap::{Parser, ValueEnum};

/// A terminal Markdown browser.
#[derive(Debug, Parser)]
#[command(
    name = "perga",
    version,
    about = "A terminal Markdown browser",
    long_about = None,
    disable_version_flag = false
)]
pub struct Cli {
    /// File or directory to open. A file opens that file with its parent
    /// directory as the vault root. A directory opens it as the vault root.
    pub path: Option<PathBuf>,

    /// Use this config file instead of the default location.
    #[arg(short = 'c', long, value_name = "FILE")]
    pub config: Option<PathBuf>,

    /// Ignore all config files and use built-in defaults.
    #[arg(long)]
    pub no_config: bool,

    /// Override the configured theme.
    #[arg(short = 't', long, value_name = "NAME")]
    pub theme: Option<String>,

    /// Initial sidebar mode.
    #[arg(long, value_name = "MODE")]
    pub sidebar: Option<SidebarModeArg>,

    /// Start with the sidebar hidden.
    #[arg(long)]
    pub no_sidebar: bool,

    /// Show non-Markdown files in the tree.
    #[arg(short = 'a', long)]
    pub all: bool,

    /// Do not respect .gitignore.
    #[arg(long)]
    pub no_gitignore: bool,

    /// Hard-wrap the document at this width (0 fits the viewport).
    #[arg(short = 'w', long, value_name = "COLUMNS")]
    pub wrap: Option<u16>,

    /// Render the file to stdout with ANSI styling and exit.
    #[arg(short = 'p', long)]
    pub print: bool,

    /// Validate config and theme files, print warnings, and exit.
    #[arg(long)]
    pub check_config: bool,

    /// Print the default configuration to stdout.
    #[arg(long)]
    pub generate_config: bool,

    /// Print shell completions to stdout.
    #[arg(long, value_name = "SHELL")]
    pub generate_completions: Option<Shell>,

    /// Print the man page to stdout.
    #[arg(long)]
    pub generate_man: bool,

    /// Do not restore or save the session for this run.
    #[arg(long)]
    pub no_session: bool,

    /// Start with mouse capture disabled.
    #[arg(long)]
    pub no_mouse: bool,

    /// Write debug logs to this file.
    #[arg(long, value_name = "FILE")]
    pub log: Option<PathBuf>,
}

/// The sidebar mode selected by `--sidebar`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum SidebarModeArg {
    Files,
    Search,
    Outline,
    Links,
}

/// Shells for which completions can be generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}
