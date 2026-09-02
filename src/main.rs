//! `perga` — a terminal Markdown browser.
//!
//! This binary owns process startup: argument parsing, the non-TUI subcommands
//! (`--generate-man`, `--generate-completions`, `--generate-config`,
//! `--check-config`, `--print`), terminal setup and teardown, and the panic
//! hook that guarantees the terminal is restored on every exit path.

mod action;
mod app;
mod cli;
mod config;
mod doc;
mod editor;
mod event;
mod search;
mod theme;
mod ui;
mod vault;

use std::io::{self, Write};
use std::process::ExitCode;

use clap::{CommandFactory, Parser};
use clap_complete::Shell as CompleteShell;

use crate::cli::{Cli, Shell};

/// Exit code for a usage or argument error, per Section 13.
const EXIT_USAGE: u8 = 2;

fn main() -> ExitCode {
    let cli = Cli::parse();

    if let Err(err) = run(&cli) {
        // The TUI is not running at this point, so stderr is safe to use.
        eprintln!("perga: {err:#}");
        return ExitCode::from(EXIT_USAGE);
    }

    ExitCode::SUCCESS
}

fn run(cli: &Cli) -> anyhow::Result<()> {
    if cli.generate_man {
        let man = clap_mangen::Man::new(Cli::command());
        let mut out = io::stdout().lock();
        man.render(&mut out)?;
        out.flush()?;
        return Ok(());
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
        return Ok(());
    }

    Ok(())
}
