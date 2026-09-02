//! `perga` — a terminal Markdown browser.
//!
//! The binary in `main.rs` is a thin wrapper: everything it does lives here, so
//! that the whole application can be driven from integration tests without a
//! terminal. Feed a sequence of [`action::Action`]s to [`app::App::update`],
//! assert on the state, and render to `ratatui::backend::TestBackend` for
//! snapshots.

pub mod action;
pub mod app;
pub mod cli;
pub mod config;
pub mod doc;
pub mod editor;
pub mod event;
pub mod search;
pub mod terminal;
pub mod theme;
pub mod ui;
pub mod vault;
