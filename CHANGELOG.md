# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.5] - 2026-09-03

Three style keys had been defined in the themes and documented since the first
version, and used by nothing. This is them.

### Added

- **`ui.show_heading_markers`**, default `true`. Turning it off hides the `#`
  a heading is written with, leaving colour and weight to mark it. The default
  stays on because `strip_colors` keeps modifiers but drops colour, and every
  heading level in every built-in theme is *only* bold, so without the `#`
  h1 through h6 collapse into each other under `NO_COLOR`. Edit mode always
  shows the source, marker included.

### Fixed

- **A thematic break is drawn as a line.** `---` was printed as three
  characters; it is now a rule across the width, in the theme's `markdown.rule`
  style.

- **A code block's background reaches the edge.** A line's style only paints
  the cells its spans occupy, so a short line left the background ending
  mid-row and the block read as ragged text on a darker strip. Lines are padded
  now; a line wider than the viewport is left for the viewport to clip and
  mark.

- **A block quote is drawn with a bar** in `markdown.blockquote_bar`, instead
  of the `>` that was typed. The bar goes on after wrapping, so it runs down
  every line of the quote, and nesting adds one per level.

- **Print mode reads the configuration.** It built its own `dark` theme and
  never called the config loader, so `--theme`, `theme.name`, `general.wrap`
  and every `[ui]` key were ignored under `--print`, against Section 9.12,
  which says the theme applies there. Configuration warnings go to stderr, so
  they cannot land in the middle of a redirected document.

## [0.1.4] - 2026-09-03

### Added

- **`m t` switches theme without restarting**, cycling `dark`, `light`,
  `high-contrast`, and then whatever themes are in your theme directory. The
  choice is remembered for the next run on that vault, unless you started
  with `--theme`, which is a decision about that run and is not overwritten.

  The session file has recorded the last theme since 0.1.0 and nothing ever
  read it back, because nothing could change the theme at runtime.

### Fixed

- **Colour degradation is no longer crude.** On a terminal that does not
  advertise truecolour in `COLORTERM` (which is many of them), every colour
  is mapped to the ANSI-256 palette. That mapping snapped each channel to the
  colour cube after a per-channel grey test, and it was badly wrong for the
  colours that matter most: the page background became pure black, 63 units off
  its intended value, and the selection background came out twice as light as
  intended, eating the contrast of every foreground drawn on it.

  It now searches the whole palette. The page background is 14 units off
  instead of 63, the selection 16 instead of 70.

- Two colours that still failed contrast once degraded (`code_inline` in both
  themes, and the dark theme's muted grey) were adjusted. The contrast tests
  now measure both palettes, so a terminal without truecolour gets the same
  guarantee as one with it.

## [0.1.3] - 2026-09-03

### Fixed

- **The built-in themes are now readable.** Thirteen keys of the default `dark`
  theme sat at 3.36:1 contrast or below, three of them with `dim` on top. Body
  text was fine; it was everything around the document that could not be
  read: the sidebar's mode row, the path in the title bar, inactive tab labels,
  search result line numbers, unchecked task items, and every "nothing here"
  line.

  Both `dark` and `light` now clear WCAG AA everywhere: 4.5:1 for anything
  carrying text, 3:1 for a border or a rule, measured against the surface it is
  actually drawn on, including the selection background, because a selected
  row keeps its own foreground. Two tests enforce this and name the offending
  key, so it cannot come back.

  `dim` is gone from the muted colours: on most terminals it halves the
  intensity and undoes the contrast the colour was chosen for.

## [0.1.2] - 2026-09-02

### Fixed

- **Typing in edit mode no longer runs commands.** `q` quit perga, `?` opened
  the help overlay, and `m` disappeared as the start of a key sequence, because
  the editing context fell back to the global bindings. It now inherits
  nothing: `Ctrl+S`, `Esc`, `Ctrl+Z`, and `Ctrl+Y` are the whole of it and
  everything else is text. `Ctrl+C` is handled before the keymap, so there is
  always a way out.

  This also means the global bindings (`Ctrl+O`, `Ctrl+T`, `Ctrl+B` and the
  rest) no longer reach the application from edit mode. That is the intended
  trade: inside a buffer, `Ctrl+W` deleting a word matters more than closing a
  tab.

- The status bar and the welcome screen now say how to reach the file tree.
  `Tab` was advertised nowhere, while the only key labelled "sidebar",
  `Ctrl+B`, made the sidebar disappear. `Tab focus the file tree` comes first
  in both, and the toggle is labelled `hide sidebar`.

## [0.1.1] - 2026-09-02

### Fixed

- `perga note.md | head`, and quitting `less -R` early, no longer print
  `writing to stdout: Broken pipe` and exit 1. A closed pipe is the end of the
  job, not a failure.

### Changed

- The installer script puts the binary in `~/.local/bin` rather than
  `~/.cargo/bin`. perga ships a static binary and its audience is terminal
  users, not necessarily Rust users; `~/.local/bin` is on `PATH` out of the box
  on current distributions.

## [0.1.0] - 2026-09-02

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
  follow every tab pointing at a renamed file, and report, rather than
  rewrite, links to the old name.
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
- Three built-in themes (`dark`, `light`, and an ANSI-16 `high-contrast`),
  user themes loaded from files with hot reload, background detection from
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

[Unreleased]: https://github.com/ankasoft/perga/compare/v0.1.5...HEAD
[0.1.5]: https://github.com/ankasoft/perga/compare/v0.1.4...v0.1.5
[0.1.4]: https://github.com/ankasoft/perga/compare/v0.1.3...v0.1.4
[0.1.3]: https://github.com/ankasoft/perga/compare/v0.1.2...v0.1.3
[0.1.2]: https://github.com/ankasoft/perga/compare/v0.1.1...v0.1.2
[0.1.1]: https://github.com/ankasoft/perga/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/ankasoft/perga/releases/tag/v0.1.0
