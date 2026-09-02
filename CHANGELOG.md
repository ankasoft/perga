# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1] — 2026-09-02

### Fixed

- `perga note.md | head`, and quitting `less -R` early, no longer print
  `writing to stdout: Broken pipe` and exit 1. A closed pipe is the end of the
  job, not a failure.

### Changed

- The installer script puts the binary in `~/.local/bin` rather than
  `~/.cargo/bin`. perga ships a static binary and its audience is terminal
  users, not necessarily Rust users; `~/.local/bin` is on `PATH` out of the box
  on current distributions.

## [0.1.0] — 2026-09-02

The first release.

### Added

**Reading**

- Markdown rendering through `pulldown-cmark` and `tui-markdown`, with GFM
  tables, task lists, strikethrough, footnotes, and autolinks.
- Syntax-highlighted fenced code blocks via `syntect`, loaded off-thread so it
  never costs the first frame. Code is clipped rather than wrapped, and `h`/`l`
  scroll it sideways.
- A windowed renderer with an owned-line block cache keyed by content hash:
  opening a document costs what the visible screen costs, and editing one
  paragraph in a 10,000-line document re-renders one block.
- YAML frontmatter is hidden from the body and its `title` becomes the tab
  label. Images render as placeholders, raw HTML as literal dimmed text.
- Non-UTF-8 files open read-only rather than being refused or corrupted; CRLF
  endings and byte-order marks survive a round trip.
- `--print`, and the same output whenever stdout is not a terminal.

**Navigating**

- A persistent hierarchical sidebar with four modes: files, search, outline,
  and links.
- A lazy file tree streamed from a background `ignore` walk, so the first frame
  paints before the walk finishes. Dotted directories are shown by default;
  `.git` and friends never are.
- Inline link following with relative-path, anchor, directory, external, and
  non-Markdown targets all resolved distinctly, and broken ones reported rather
  than guessed at.
- Link hint mode (`f`), home-row labels drawn over the links themselves.
- Per-tab back and forward history that restores the exact scroll offset, with
  a 100-entry cap so a cycle of links cannot grow it without limit.
- Up to twenty tabs, each with its own document, scroll position, history, link
  focus, and find state.
- Wiki-links in all four spellings, resolved through a background backlink
  index that is cached between runs and validated per file. An ambiguous target
  opens a picker rather than picking.
- An outline that highlights the section you are inside as you scroll, sharing
  one slug implementation with anchors so the two cannot disagree.

**Searching**

- Project-wide search with `grep-searcher`: streaming results, smart case, a
  `/pattern/` regex escape hatch, a result cap that says when it was hit, and
  cancellation that leaves no threads behind.
- Incremental find-in-document with every match highlighted and the count in
  the bar.
- A fuzzy quick switcher over the vault, showing the session's recent files
  until something is typed.

**Editing**

- Edit mode on `tui-textarea`, with the cursor round-tripping through the
  offset↔line map so you never lose your place crossing the boundary.
- Atomic saves that preserve permissions, detect a file that changed on disk,
  and never truncate the original.
- File creation and renaming that refuse anything writing outside the vault,
  follow every tab pointing at a renamed file, and report — rather than
  rewrite — links to the old name.
- Recovery files for unsaved text when perga is signalled, offered back the
  next time the document is opened.
- `$EDITOR` handoff that leaves the terminal properly, so a full-screen editor
  works.
- Live reload with a debounced `notify` watcher that never clobbers a dirty
  buffer and ignores perga's own writes.

**Configuring**

- Five-layer configuration where an unknown key or an invalid value costs that
  key alone and warns; `--generate-config` and `--check-config`.
- A vault-local `.perga.toml` restricted to presentation keys, because it
  arrives with any repository you clone.
- Full key remapping from `[keys]`, reflected in the help overlay and the docs.
- Three built-in themes — `dark`, `light`, and an ANSI-16 `high-contrast` — user
  themes loaded from files with hot reload, background detection from
  `COLORFGBG`, ANSI-256 degradation, and `NO_COLOR`.
- One session per vault, restored when perga is opened with no path.

**The shell**

- A blocking event loop with a dedicated input thread: 0% CPU at idle by
  construction.
- Terminal state restored on a normal exit, an error, a panic, `SIGINT`,
  `SIGTERM`, `SIGHUP`, and around `Ctrl+Z`.
- Responsive from 200x60 down to 40x10, with a legible message below that.

### Known gaps

- The terminal background is detected from `COLORFGBG` only; the OSC 11 query
  is not implemented. Set `theme.name = "light"` on a light terminal that does
  not set the variable. The reasoning is in `docs/decisions.md` (D77).
- Deletion is deliberately not offered. Every other file operation can be
  undone; that one cannot.

[Unreleased]: https://github.com/ankasoft/perga/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/ankasoft/perga/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/ankasoft/perga/releases/tag/v0.1.0
