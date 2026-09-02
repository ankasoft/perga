# perga — Build Specification

**Repository:** `https://github.com/ankasoft/perga`
**Crate name:** `perga`
**Binary name:** `perga`
**License:** `MIT OR Apache-2.0`
**Target platform:** Linux only (x86_64 and aarch64)
**Language:** Rust (stable)

This document is the complete build specification for `perga`. It is written to be
handed to an autonomous coding agent. Read it in full before writing any code.

---

## 0. Operating rules for the implementing agent

These rules are absolute and override any inclination to do otherwise.

1. **Work autonomously to completion.** Do not stop to ask the user questions, do not
   request approval between milestones, and do not wait for input. Work through
   Section 17 (Milestones) from M0 to M11 in order without pausing.
2. **Resolve ambiguity yourself.** If a detail is unspecified, choose the option most
   consistent with the design principles in Section 3, record the decision in
   `docs/decisions.md` with a one-paragraph rationale, and continue.
3. **All repository artefacts in English.** Code, identifiers, comments, commit
   messages, documentation, error messages, CLI help text, man page, README, and
   issue templates. No exceptions — this is a public open-source project with an
   international audience.
4. **All communication with the project owner in Turkish.** Everything you address
   to the owner rather than to the repository is written in Turkish: progress
   reports, milestone completion notices, status summaries, explanations of
   decisions you made, warnings about problems you hit, and the final handover
   summary. If you ever do need to surface something to the owner, do it in Turkish.
   The split is simple: anything that lands in the repository is English, anything
   said to the owner is Turkish. Do not mix the two — do not write Turkish comments
   in code, and do not report progress to the owner in English.
5. **Commit messages must not contain any AI attribution.** Do not write
   `Generated with Claude`, `Co-Authored-By: Claude`, `🤖`, `Co-Authored-By: <any AI>`,
   or any similar trailer, footer, or mention in commit messages, PR bodies, code
   comments, or documentation. Commits must read as if written by a human developer.
6. **Never commit broken code to `main`.** Every commit must pass
   `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test`.
7. **Do not hand-write dependency versions.** Use `cargo add <crate>` so the latest
   compatible version is resolved at build time, then commit the resulting
   `Cargo.toml` and `Cargo.lock`. Version numbers written in this document are
   illustrative minimums, not pins.
8. **Do not publish to crates.io.** Publishing requires the owner's credentials.
   Prepare everything so that `cargo publish` works, verify with
   `cargo publish --dry-run`, and document the manual step in `docs/publishing.md`.
9. **Stay in scope.** Section 4 lists explicit non-goals. Do not implement them, do
   not add dependencies for them, and do not leave stub modules for them.

---

## 1. What perga is

`perga` is a terminal Markdown browser: a full-screen TUI application that opens a
directory of Markdown files and lets the user navigate, read, search, and edit them
with the ergonomics of a document browser rather than a pager.

The mental model is Obsidian's layout reduced to a terminal: a persistent left
sidebar that switches between modes, a document viewport with tabs, and browser-like
back/forward navigation across files.

### Name origin (use this in README.md)

Parchment takes its name from Pergamon, the ancient city in western Anatolia where,
according to Pliny, animal-skin writing surfaces were developed after Egypt
restricted papyrus exports. The Latin *pergamena* and, through it, the English
*parchment* both descend from the city's name. `perga` is named after the root of
the written page.

---

## 2. Why this project exists (context, not to be implemented)

Existing tools and their gaps. Use this to inform design decisions; do not copy code.

| Tool | What it does | Gap perga fills |
|---|---|---|
| `glow` | Recursively finds Markdown files, shows them in a flat list, renders with Glamour | Flat list discards directory hierarchy; list and reader are separate screens so the file list disappears when reading; no link navigation between documents; no history; at the time of writing its interactive mode skips dotted directories such as `.github` (an open upstream issue — verify before repeating this claim in public-facing text) |
| `frogmouth` | Browser-like navigation stack, history, bookmarks, table of contents | Unmaintained; no new releases in over a year; Python startup latency; no persistent file tree |
| `mdcat` | Excellent single-document rendering, inline images | Single-shot renderer, not an interactive browser |
| `mdview`, `md-viewer-py`, `glow-web` | Serve a directory over local HTTP | Leaves the terminal; requires a browser |

The specific combination perga targets and nothing else currently provides:
persistent hierarchical sidebar, cross-file link following with per-tab history,
wiki-link resolution with backlinks, and in-place editing, all in a maintained
single static binary.

---

## 3. Design principles

Apply these when resolving anything this document leaves open.

1. **Reading is the primary mode.** Editing is a mode you enter deliberately and
   leave. The application opens in read mode, always.
2. **Never lose context.** The sidebar does not disappear when a document opens.
   Navigation is additive: history is preserved, tabs are preserved, scroll position
   per tab is preserved.
3. **Instant startup.** Cold start to first painted frame must not exceed 50 ms for a
   directory of 1,000 files. Directory walking and index building happen in
   background threads; the UI paints immediately with partial data.
4. **Never block the UI thread.** All filesystem walking, searching, and index
   building runs off-thread and communicates by channel.
5. **Keyboard first.** Every action must be reachable by keyboard. Mouse support is a
   convenience layer, never the only path.
6. **Terminal-native.** No web view, no browser, no embedded runtime. One static
   binary with no runtime dependencies.
7. **Predictable configuration.** Everything visual is themeable through files;
   everything behavioural is configurable through one TOML file. No hidden defaults
   that cannot be discovered from `docs/configuration.md`.
8. **Fail visibly but never fatally.** A malformed file, a broken link, or an
   unreadable directory renders an inline error and leaves the application usable.

---

## 4. Non-goals for v1 (do not implement)

- **Inline image rendering** (Kitty/iTerm2/Sixel protocols). Architecture must not
  preclude it — see Section 9.2 — but v1 renders images as a text placeholder.
- **Graph view.** A force-directed graph is unreadable at terminal dimensions.
- **Plugin system, scripting, or extension API.**
- **Remote sources.** No fetching Markdown over HTTP, no GitHub URL support, no Git
  integration.
- **macOS and Windows support.** Do not add `#[cfg(target_os)]` branches for them, do
  not add cross-platform dependencies for their sake, do not test for them.
- **Bookmarks / starred files.** Deferred to v2.
- **Tag pane.** Tags are searchable through the search mode; no dedicated panel.
- **Split panes.** One viewport at a time. Tabs, not splits.
- **Multiple vaults open simultaneously.**
- **Telemetry or analytics of any kind.**

---

## 5. Toolchain and dependencies

### 5.1 Toolchain

- Rust stable. Pin an MSRV in `Cargo.toml` (`rust-version`) at the stable release
  current when you start, minus two minors. Verify the MSRV builds in CI.
- Edition 2021 or later.
- `rustfmt` with default settings, checked in CI.
- `clippy` with `-D warnings` in CI.

### 5.2 Dependencies and why each is chosen

Add these with `cargo add`. The feature flags below are mandatory — several exist to
keep static musl builds working.

| Crate | Purpose | Required features / notes |
|---|---|---|
| `ratatui` | TUI framework, layout, widgets | Default features (the crossterm backend is the default) |
| `crossterm` | Terminal backend, raw mode, events, mouse | Default features only. Do **not** enable `event-stream` — it pulls in `futures` and implies an async runtime, which Section 5.2 forbids. Input is read on a dedicated thread (Section 7.1) |
| `pulldown-cmark` | Markdown parsing | Enable GFM extensions: tables, footnotes, strikethrough, task lists |
| `tui-markdown` | Markdown → `ratatui::text::Text` rendering | Use as the baseline renderer. See Section 9.2 for the wrapper you must build around it |
| `tui-textarea` | Editor widget with undo/redo, selection, search | Use the crossterm + ratatui feature combination |
| `syntect` | Code block syntax highlighting | **`default-features = false`, `features = ["default-fancy"]`**. The default `onig` backend links a C library and breaks static musl builds; `fancy-regex` is pure Rust |
| `ignore` | Directory walking | This is ripgrep's walker; gives `.gitignore` awareness and parallel traversal for free |
| `grep-searcher`, `grep-regex`, `grep-matcher` | Cross-file content search | ripgrep's search engine; do not shell out to `rg` |
| `nucleo` | Fuzzy matching for the quick switcher | |
| `notify` | Filesystem watching for live reload | Use the debounced wrapper or debounce yourself; see Section 9.9 |
| `clap` | CLI argument parsing | `derive` feature |
| `clap_mangen` | Man page generation | Build-time or an `xtask`; see Section 16.5 |
| `clap_complete` | Shell completion generation | bash, zsh, fish |
| `serde` | Config and theme deserialization | `derive` |
| `toml` | Config and theme format | |
| `directories` | XDG base directory resolution | |
| `anyhow` | Error handling in application code | |
| `thiserror` | Error types in library code | |
| `tracing`, `tracing-subscriber`, `tracing-appender` | Logging to file | Never log to stdout/stderr while the TUI owns the terminal |
| `unicode-width` | Correct column widths for CJK and emoji | |
| `postcard` | Serialisation of the backlink index cache | Small, fast, `serde`-based; do not use `bincode` 2.x's non-serde API |
| `signal-hook` | SIGTERM/SIGHUP handling for terminal restoration | See Section 13 |
| `insta` | Snapshot testing of rendered frames | dev-dependency |
| `criterion` | Benchmarks for the performance targets | dev-dependency; see Section 15.6 |

**Forbidden dependencies:** anything pulling `openssl`, `native-tls`, `reqwest`, or
any HTTP client (there is no network feature); anything requiring a C toolchain
beyond libc; `tokio` (a single background-thread model with channels is sufficient
and keeps binary size and startup latency down — use `std::thread` and
`std::sync::mpsc` or `crossbeam-channel`).

### 5.3 Release profile

Put this in `Cargo.toml`:

```toml
[profile.release]
lto = "fat"
codegen-units = 1
strip = true
panic = "abort"
```

Target binary size under 8 MB. `syntect`'s bundled syntax and theme dumps are the
largest contributor; if size exceeds 8 MB, trim the bundled syntax set to a curated
list of languages rather than dropping the feature.

---

## 6. Repository layout

```
perga/
├── Cargo.toml
├── Cargo.lock                  (committed — this is a binary crate)
├── LICENSE-MIT
├── LICENSE-APACHE
├── README.md
├── CHANGELOG.md                (Keep a Changelog format)
├── CONTRIBUTING.md
├── deny.toml                   (cargo-deny — Section 16.6)
├── .github/
│   ├── workflows/
│   │   ├── ci.yml
│   │   ├── release.yml         (generated by `dist init`)
│   │   └── bench.yml           (nightly, non-blocking — Section 15.6)
│   └── ISSUE_TEMPLATE/
│       ├── bug_report.md
│       └── feature_request.md
├── docs/
│   ├── usage.md
│   ├── configuration.md
│   ├── theming.md
│   ├── keybindings.md
│   ├── publishing.md
│   ├── licensing.md
│   ├── architecture.md
│   └── decisions.md
├── packaging/
│   ├── PKGBUILD                (AUR, -bin package)
│   └── README.md               (how to use each packaging artifact)
├── demo/
│   ├── demo.tape               (vhs script — Section 17, M11)
│   ├── demo.gif
│   ├── social-preview.png
│   └── README.md
├── themes/                     (built-in themes, embedded at compile time)
│   ├── dark.toml
│   ├── light.toml
│   └── high-contrast.toml
├── benches/                    (criterion — Section 15.6)
├── tests/
│   ├── common/                 (fixture generation helpers, e.g. the large document)
│   ├── fixtures/
│   │   └── vault/              (test corpus — see Section 15.4)
│   ├── snapshots/              (insta snapshots)
│   ├── navigation.rs
│   ├── wikilinks.rs
│   ├── search.rs
│   ├── config.rs
│   └── render.rs
└── src/
    ├── main.rs                 (CLI entry, terminal setup/teardown, panic hook)
    ├── app.rs                  (App struct, event loop, top-level state)
    ├── event.rs                (input event → Action mapping)
    ├── action.rs               (Action enum — the only way state changes)
    ├── config/
    │   ├── mod.rs
    │   ├── schema.rs           (serde structs + defaults)
    │   └── keymap.rs           (key string parsing, remapping)
    ├── theme/
    │   ├── mod.rs
    │   ├── schema.rs
    │   └── builtin.rs          (include_str! of themes/*.toml)
    ├── doc/
    │   ├── mod.rs
    │   ├── document.rs         (loaded document: source, parsed blocks, links)
    │   ├── render.rs           (block cache, offset→line mapping, wrapping)
    │   ├── outline.rs          (heading extraction)
    │   └── links.rs            (inline link + wiki-link extraction, resolution)
    ├── vault/
    │   ├── mod.rs
    │   ├── walker.rs           (ignore-based traversal, background)
    │   ├── tree.rs             (lazy tree model)
    │   ├── index.rs            (wiki-link + backlink index)
    │   └── watch.rs            (notify integration, debounce)
    ├── search/
    │   ├── mod.rs
    │   ├── content.rs          (grep-searcher project search)
    │   ├── fuzzy.rs            (nucleo quick switcher)
    │   └── in_doc.rs           (find-in-document)
    ├── editor/
    │   ├── mod.rs
    │   └── buffer.rs           (dirty tracking, save, external editor handoff)
    └── ui/
        ├── mod.rs              (frame composition)
        ├── layout.rs           (constraint computation)
        ├── sidebar/
        │   ├── mod.rs
        │   ├── files.rs
        │   ├── search.rs
        │   ├── outline.rs
        │   └── backlinks.rs
        ├── viewport.rs
        ├── welcome.rs          (welcome screen, logo tiers — Section 8.5)
        ├── tabs.rs
        ├── statusbar.rs
        ├── overlay/
        │   ├── mod.rs
        │   ├── help.rs
        │   ├── switcher.rs
        │   ├── find.rs
        │   ├── prompt.rs
        │   └── confirm.rs
        └── hints.rs            (link hint labels)
```

---

## 7. Architecture

### 7.1 State model

Use a strict unidirectional flow. This is the single most important architectural
constraint in this document.

```
crossterm event ──▶ event.rs ──▶ Action ──▶ App::update() ──▶ state mutation
                                                                    │
background thread ──▶ channel message ──▶ Action ────────────────────┤
                                                                    ▼
                                                          ui::render(&state, frame)
```

- `Action` is an enum. **Every** state change goes through `App::update(Action)`.
  No widget mutates state during rendering. Rendering is a pure function of state.
- **Terminal input is read on its own thread** that calls `crossterm::event::read()`
  in a loop and forwards each event into the same channel the workers use. The main
  loop then does a single blocking `recv()` and nothing else: no `poll(timeout)`, no
  `try_recv` spin. This is what makes the 0% idle CPU target in Section 14 achievable
  without special cases. Debouncing belongs inside the watcher thread, not in the
  main loop; the main loop never needs a timer.
- Background workers (walker, indexer, searcher, watcher) send messages over the same
  channel. Every message, from any source, is converted into an `Action` and applied
  through `App::update`. Use `crossbeam-channel` if you need multiple producers with
  bounded capacity and a clean `select!` for the rare case of a second receiver;
  otherwise `std::sync::mpsc` is sufficient.
- This makes the whole application testable without a terminal: feed a sequence of
  `Action`s, assert on state, and render to `ratatui::backend::TestBackend` for
  snapshots.

### 7.2 Core types

Define these early; most of the work is filling them in.

```rust
struct App {
    config: Config,
    theme: Theme,
    vault: Vault,           // tree + index + root path
    tabs: Vec<Tab>,
    active_tab: usize,
    sidebar: Sidebar,       // visible, width, mode, per-mode state
    focus: Focus,           // Sidebar | Viewport | Overlay
    overlay: Option<Overlay>,
    status: StatusLine,
    should_quit: bool,
}

struct Tab {
    doc: Option<Document>,  // None for the welcome screen
    history: History,       // back/forward stacks of Location
    scroll: usize,          // rendered line offset — NOT u16: 100k-line documents overflow it
    hscroll: u16,           // horizontal offset for clipped code blocks and tables
    mode: TabMode,          // Read | Edit
    editor: Option<EditorState>,
    find: Option<FindState>,
}

struct Location {
    path: PathBuf,
    anchor: Option<String>, // heading slug
    scroll: usize,          // restored when navigating back
}

struct Document {
    path: PathBuf,
    source: String,
    mtime: SystemTime,
    blocks: Vec<Block>,     // parsed, with byte ranges
    links: Vec<LinkTarget>, // inline + wiki, with rendered positions
    outline: Vec<Heading>,
    render_cache: RenderCache,
}
```

**Crucial lifetime note.** `tui_markdown::from_str` returns a `Text` that may borrow
from its input string. Do not store borrowed `Text` in your cache — it will fight
every attempt to hold the document and its rendering in the same struct. Convert to
owned `Vec<Line<'static>>` at the cache boundary. Budget time for this; it is the
single most likely place to lose a day to the borrow checker.

### 7.3 Focus model

`Focus` is one of `Sidebar`, `Viewport`, `Overlay`. Rules:

- `Tab` / `Shift+Tab` cycles focus between `Sidebar` (if visible) and `Viewport`.
- When an overlay is open, focus is `Overlay` and all other input is swallowed except
  `Esc` (close) and `Ctrl+C` (quit).
- Focused pane border uses the theme's `border_focused` colour; unfocused uses
  `border`. This must be visually unambiguous — it is the only cue the user has.
- Entering edit mode forces focus to `Viewport` and locks it there until edit mode
  exits.

---

## 8. UI specification

### 8.1 Layout

```
┌─ perga ─────────────────────────────────── docs/api/auth.md ─── 2/7 ─┐
│ ● auth.md │ setup.md │ +                                            │  ← tab bar
├──────────────────────────┬──────────────────────────────────────────┤
│ FILES  search  outline   │                                          │
│ links                    │  # Authentication                        │
│ ─────────────────────────│                                          │
│ ▾ docs/                  │  This service uses Bearer tokens. See    │
│   ▾ api/                 │  setup for the full walkthrough, and     │
│       auth.md         ●  │  Token Rotation for the rotation policy. │
│       webhooks.md        │                                          │
│       errors.md          │  ## Obtaining a token                    │
│   ▸ guides/              │                                          │
│   ▸ .github/             │  curl -X POST /auth/token \              │
│   README.md              │    -d '{"key":"..."}'                    │
│                          │                                          │
│                          │  (links above render underlined in the   │
│                          │   theme's link/wikilink styles; markup   │
│                          │   is never shown in read mode)           │
├──────────────────────────┴──────────────────────────────────────────┤
│ READ  ←→ history  f links  ^b sidebar  ^o switch  ^f find  ? help   │  ← status bar
└─────────────────────────────────────────────────────────────────────┘
```

Rules:

- **Tab bar** is hidden entirely when only one tab is open and
  `ui.always_show_tabs = false` (the default).
- **Sidebar** default width 32 columns, configurable, resizable at runtime with
  `<` / `>` while the sidebar has focus, and by dragging its border with the mouse.
  Width is persisted to the state file. (`Ctrl+Left`/`Ctrl+Right` are *not* used:
  they are word-motion keys inside the editor and would conflict.)
- **Sidebar mode row** shows the four modes; the active one is uppercase and styled
  with `sidebar.mode_active`. Modes wrap to a second line if the sidebar is narrow.
- **Status bar** shows the current tab mode (`READ` / `EDIT`), then a
  context-sensitive hint row. In edit mode it shows a dirty indicator and cursor
  line:column.
- **Title bar** shows the path relative to the vault root, and scroll position as
  `current_line/total_lines` on the right.
- **Responsive behaviour:** below 80 columns the sidebar auto-hides (recoverable with
  `Ctrl+B`, which then overlays it rather than splitting). An overlaid sidebar takes
  focus on open and is dismissed by `Esc`, by `Ctrl+B` again, or automatically when
  it opens a document. Below 20 rows, hide the
  tab bar and collapse the status bar to one line. Never panic on tiny terminals;
  minimum supported size is 40x10, below which render a single "terminal too small"
  message.

### 8.2 Sidebar modes

Switch with the sequences `m1`..`m4` globally (`m` for mode), or `1`..`4` when the
sidebar has focus. `Alt+1`..`Alt+4` are also bound but must not be the only path:
Konsole, GNOME Terminal, and other emulators reserve `Alt+digit` for their own tab
switching and never deliver it to the application.

**1. Files.** Lazy-loading hierarchical tree of the vault.
- Directories collapsed by default except the path to the active document.
- `Enter` / `l` / `→` expands a directory or opens a file.
- `h` / `←` collapses, or jumps to parent when already collapsed.
- Active document marked with `●`.
- Non-Markdown files are hidden by default (`files.show_all = false`); when shown,
  they are dimmed and open externally via `xdg-open`.
- **Dotted directories are included by default** (`files.include_hidden = true`).
  This is a deliberate divergence from `glow`, whose interactive mode excludes
  `.github`, `.claude`, and similar directories with no working override. Respect
  `.gitignore` by default (`files.respect_gitignore = true`) but never hide dotfiles
  merely for being dotfiles.
- **VCS metadata directories are always excluded** regardless of `include_hidden`:
  `.git`, `.hg`, `.svn`, `.jj`. A `.git` directory can contain tens of thousands of
  object files and has no user-facing value in the tree. Hard-code this list; it is
  not configurable.
- `r` on a tree entry renames it (prompt pre-filled with the current name); open
  tabs pointing at the old path follow the rename. Deletion is deliberately not
  offered in v1.
- Sorting configurable: `name`, `mtime`, `size`; directories first, always.

**2. Search.** Results of the last project-wide search.
- Grouped by file, each hit showing line number and the matching line with the match
  span highlighted.
- `Enter` opens the file and scrolls to the hit line.
- Result count and elapsed time in the mode header.

**3. Outline.** Headings of the active document.
- Indented by level, click or `Enter` to scroll the viewport to that heading.
- The heading containing the current scroll position is highlighted, updated live as
  the user scrolls.

**4. Links.** Two sections for the active document:
- **Outgoing** — every resolvable inline and wiki-link, with a marker for broken
  targets.
- **Backlinks** — every document in the vault that links to the active document,
  from the index built in Section 9.6.

### 8.3 Overlays

- **Help (`?`)** — full keybinding reference, generated from the keymap so it can
  never drift from the actual bindings. Reflects user remappings.
- **Quick switcher (`Ctrl+O`)** — fuzzy file finder over the whole vault, `nucleo`
  matching, matched characters highlighted, `Enter` opens in current tab,
  `Ctrl+Enter` (or `Tab` `Enter` on legacy terminals) opens in a new tab. **With an
  empty query the list shows recent files, most recent first**, from the session's
  recent list; typing switches to fuzzy results over the whole vault. Typing a path
  that matches nothing shows a final `Create "<query>.md"` entry that creates the
  file (Section 9.11).
- **Find in document (`Ctrl+F`)** — incremental, highlights all matches inline,
  `Enter` / `Shift+Enter` cycles, match count in the bar, `Esc` closes and clears.
- **Confirm dialog** — used for discarding unsaved edits, overwriting on save when
  the file changed on disk, creating a file from a broken wiki-link, and restoring a
  recovery file.
- **Prompt** — a single-line text input used for new file paths, rename, and the
  project search query. Supports `Ctrl+U` to clear, `Ctrl+W` to delete a word, and
  `Tab` path completion against the vault for the file prompts.

### 8.4 Link hint mode

Pressing `f` in read mode overlays a short label on every visible link. Labels are
assigned in home-row order — `a s d f g h j k l` first, then two-letter combinations
of the same set — so the most reachable keys go to the links nearest the top of the
viewport. Typing a label follows that link. `Esc` cancels.
This is faster than cycling with `n`/`N` on dense documents; implement both.

### 8.5 Welcome screen and ASCII logo

The welcome screen is what a tab shows when `Tab.doc` is `None` — on launch in an
empty directory, and on every `Ctrl+T`. It is the only place in the application where
branding appears.

**Where the logo must not appear:** there is no startup splash (the 50 ms
first-frame target in Section 14 makes any deliberate delay indefensible), no
persistent banner in the title bar (vertical rows are the scarcest resource in a
TUI), nothing in the help overlay, and nothing in `--version` output, which must stay
machine-parseable as `perga <version>`.

**Implementation constraints:**

- Store the art as `const &str` in `ui/welcome.rs`. Do not generate it at runtime and
  do not add a figlet-style dependency.
- Use only the Unicode block elements `█`, `▀`, `▄`, and space. These render in
  effectively every monospace font. Do not use Braille patterns or uncommon
  box-drawing combinations — they produce tofu on terminals without the glyphs.
- Colour comes from the theme key `ui.logo` (Section 11.1). No hardcoded colours. The
  logo must remain legible under the `high-contrast` ANSI-16 theme.
- Three size tiers, selected automatically from the available viewport width. This is
  mandatory: a 40-column banner overflows a 40-column terminal and corrupts the
  frame.
- Centre the logo horizontally and place the block at roughly one third of the
  viewport height, not vertically centred — text centred slightly high reads better.

**Large tier — viewport width ≥ 56:**

```
██████  ███████ ██████   ██████   █████
██   ██ ██      ██   ██ ██       ██   ██
██████  █████   ██████  ██   ███ ███████
██      ██      ██   ██ ██    ██ ██   ██
██      ███████ ██   ██  ██████  ██   ██
```

**Medium tier — viewport width 36 to 55:**

```
█▀▀█ █▀▀ █▀▀█ █▀▀▀ █▀▀█
█▄▄█ █▀▀ █▄▄▀ █ ▀█ █▄▄█
█    ▀▀▀ ▀  ▀ ▀▀▀▀ ▀  ▀
```

**Minimal tier — viewport width < 36:** the word `perga` styled with `ui.title`,
with a single horizontal rule beneath it. No block art.

**Below the logo**, in every tier, render the version and a short onboarding block
styled with `ui.logo_subtitle`, so the empty screen doubles as onboarding:

```
                          perga 0.1.0

                   Ctrl+O   find a document
                   Ctrl+B   toggle sidebar
                        ?   all keybindings
```

In the minimal tier, reduce this to the version line plus `? for help`. When the
viewport is shorter than 12 rows, drop the logo entirely and show only the
onboarding lines.

### 8.6 Terminal capabilities and input handling

Several default bindings in this document only work on terminals that implement the
**kitty keyboard protocol** (kitty, WezTerm, foot, Ghostty, recent Alacritty, and
others). On legacy terminals, `Ctrl+Enter` is indistinguishable from `Enter`,
`Shift+Enter` from `Enter`, and `Ctrl+Shift+<letter>` from `Ctrl+<letter>`. The
application must be fully usable on both.

- At startup, call crossterm's `PushKeyboardEnhancementFlags` with
  `DISAMBIGUATE_ESCAPE_CODES | REPORT_ALTERNATE_KEYS`, guarded by
  `supports_keyboard_enhancement()`. Pop the flags on exit and in the panic hook.
- **Every binding that depends on the enhanced protocol has a plain fallback that is
  always bound**, so the help overlay lists both. The pairs are: `Ctrl+Enter` ↔ the
  sequence `t` `Enter`; `Shift+Enter` (previous find match) ↔ `N`; `Ctrl+Shift+F` ↔
  `Ctrl+G`; `Alt+←`/`Alt+→` ↔ `H`/`L`; `Alt+1`..`Alt+4` ↔ `m1`..`m4`;
  `Ctrl+PageUp`/`Ctrl+PageDown` ↔ `gT`/`gt`.
- **`Ctrl+B` is the tmux prefix.** A large fraction of terminal users will never see
  it reach perga. Keep it as the default for Obsidian parity, bind `Ctrl+E` as an
  always-available alias, and give the tmux conflict its own paragraph near the top
  of `docs/keybindings.md` and in the README's quick-start.
- **Bracketed paste** must be enabled (`EnableBracketedPaste`). In edit mode a
  `Event::Paste` is inserted into the textarea as a single operation and a single
  undo step. Without this, a paste arrives as hundreds of individual key events, is
  slow, and interacts badly with any auto-indentation.
- **Mouse capture is configurable** (`ui.mouse`, default `true`) and toggleable at
  runtime with the sequence `mm`. (`Ctrl+M` is not usable for this: it is
  indistinguishable from `Enter` on every terminal.) When capture is on, the
  terminal's own text selection is unavailable; document that most terminals allow
  `Shift+drag` to bypass application mouse capture. When capture is off, wheel
  scrolling still works on terminals that translate wheel events to arrow keys in
  alternate-screen mode.
- **Mouse targets when capture is on:** click a tree entry to select, double-click to
  open; click a tab to switch, middle-click to close; click a link in the viewport to
  follow it; click an outline entry to scroll; drag the sidebar border to resize;
  wheel over any pane scrolls that pane.
- **Key sequences** (`gg`, `gt`, `m1`, `t` `Enter`, `mm`): the first key enters a
  pending state shown in the status bar (`g…`). There is no timeout — the sequence
  completes on the next key or is cancelled by `Esc` or by any key that does not
  continue a known sequence, which is then processed normally. Prefix keys therefore
  never have a standalone meaning: this is why "top of document" is `gg` and not `g`.
- **Flow control:** raw mode disables `IXON`, so `Ctrl+S` and `Ctrl+Q` reach the
  application. Verify this in the terminal setup test rather than assuming it.
- **`NO_COLOR`:** when the `NO_COLOR` environment variable is set and non-empty,
  render with no colour attributes at all (bold, dim, underline, and reverse remain).
  This follows the no-color.org convention.

---

## 9. Feature specifications

Each subsection ends with acceptance criteria. Every criterion must be verifiable by
a test or a documented manual check.

### 9.1 CLI

```
perga [OPTIONS] [PATH]

Arguments:
  [PATH]  File or directory to open. A file opens that file with its parent
          directory as the vault root. A directory opens it as the vault root.
          Defaults to the current directory.

Options:
  -c, --config <FILE>     Use this config file instead of the default location
      --no-config         Ignore all config files, use built-in defaults
  -t, --theme <NAME>      Override the configured theme
      --sidebar <MODE>    Initial sidebar mode: files|search|outline|links
      --no-sidebar        Start with the sidebar hidden
  -a, --all               Show non-Markdown files in the tree
      --no-gitignore      Do not respect .gitignore
  -w, --wrap <COLUMNS>    Hard-wrap the document at this width (0 = fit viewport)
  -p, --print             Render the file to stdout with ANSI styling and exit;
                          no TUI. Implied when stdout is not a terminal.
      --check-config      Validate config and theme files, print warnings, exit
      --generate-config   Print the default configuration to stdout
      --generate-completions <SHELL>  Print completions for bash|zsh|fish
      --generate-man                  Print the man page to stdout
      --no-session        Do not restore or save the session for this run
      --no-mouse          Start with mouse capture disabled
      --log <FILE>        Write debug logs to this file
  -h, --help
  -V, --version
```

Acceptance criteria:
- [ ] `perga` with no arguments opens the current directory.
- [ ] `perga README.md` opens that file with `.` as vault root.
- [ ] `perga /nonexistent` exits with code 2 and a clear message on stderr, with the
      terminal left in a clean state.
- [ ] `--generate-completions` and `--generate-man` write to stdout and exit 0
      without initialising the TUI.
- [ ] `-V` prints `perga <version>`.
- [ ] `perga README.md | cat` produces styled output on stdout with no TUI escape
      sequences (no alternate screen, no cursor movement), and `perga -p README.md`
      does the same when stdout is a terminal.
- [ ] `--check-config` exits 0 on a valid config, 1 on an invalid one, and names
      every offending key.

### 9.2 Markdown rendering

Build a `doc::render` module wrapping `tui-markdown`. Do not call `from_str` on the
whole document every frame.

Requirements:
- Parse once with `pulldown_cmark::Parser::new_ext(...).into_offset_iter()` so every
  event carries its byte range in the source.
- Group events into `Block`s (paragraph, heading, list, code block, table,
  blockquote, thematic break) each with a `Range<usize>` byte span.
- Render blocks individually and cache the result keyed by
  `(hash_of_block_source, render_width)`. Store owned `Vec<Line<'static>>`.
  **Do not key the cache by byte range**: inserting one character shifts the byte
  range of every subsequent block and would invalidate most of the cache on every
  keystroke. Hashing the block's source text makes unchanged blocks hit the cache
  regardless of where they moved. Including the width means a terminal resize
  invalidates correctly, since wrapping changes the rendered lines.
- After an edit or reload, re-parse to get the new block list, then look each block
  up in the cache by content hash; only blocks whose text actually changed render.
- Only render blocks intersecting the visible line window, plus one screen of
  overscan above and below.
- **Block heights are only known after rendering at a given width**, which makes
  "which blocks are visible" circular. Resolve it with a per-document
  `Vec<Option<u16>>` of block heights at the current width plus a prefix sum that is
  filled in as blocks render. Scrolling forward renders blocks on demand. Operations
  that need heights of blocks not yet rendered — `G`, an anchor deep in the document,
  restoring a saved scroll offset, or the scrollbar's total — trigger rendering of the
  intervening blocks; for very large documents do that in chunks of a few hundred
  blocks per frame so the UI stays responsive, and show the scrollbar as
  indeterminate until the total is known. Once rendered, heights come from the cache
  and this cost is paid once per width.
- Fenced code blocks are **never soft-wrapped**: wrapping code destroys its meaning.
  Lines longer than the viewport are clipped with a `…` indicator, and the viewport
  supports horizontal scrolling with `h` / `l` (these keys are free in the viewport;
  they belong to the tree only when the sidebar has focus). Horizontal offset is per
  tab and resets to zero on document change.
- Maintain a bidirectional map between source byte offsets and rendered line numbers.
  This map is required by: anchor navigation, find-in-document, outline sync, link
  positions, and edit-mode cursor round-tripping. Build it as a first-class
  structure, not as an afterthought.
- GFM support: tables (Unicode box-drawing borders, respecting the delimiter row's
  alignment), task lists (`☐`/`☑`), strikethrough, footnotes, autolinks.
- Fenced code blocks: syntax highlighted with `syntect`, language from the info
  string, falling back to no highlighting on unknown languages. Cache highlighted
  output per `(language, content_hash)`.
- **Load syntect lazily and off-thread.** `SyntaxSet::load_defaults_newlines()`
  decompresses an embedded dump and costs on the order of 50–100 ms — on its own that
  consumes the entire first-frame budget in Section 14. Start loading it on a
  background thread at startup; until it is ready, render code blocks unhighlighted
  with the `code_block_bg` style, then re-render them once the set arrives. A single
  frame of unhighlighted code is acceptable; a 100 ms blank screen is not.
- Raw HTML: render as literal dimmed text. Do not attempt to interpret it.
- Math: preserve delimiters as literal text.
- **Images: render as a placeholder** — `[image: alt text]` styled with the theme's
  `image_placeholder`. Isolate this in a single function
  (`render_image_placeholder`) so a future protocol-based renderer can replace it
  without touching the rest of the pipeline.
- YAML frontmatter: detect a leading `---` block, hide it from the rendered body, and
  expose `title` for the tab label if present. **Do not add `serde_yaml`** — that
  crate is archived and unmaintained. For v1 a minimal line-oriented extractor that
  reads top-level `key: value` scalars is sufficient and dependency-free; if a real
  YAML parser is ever needed, use `serde_yaml_ng`. Record the choice in
  `docs/decisions.md`.
- **Non-UTF-8 files:** read with `String::from_utf8_lossy`, render normally, but
  disable edit mode for that document and show `Read-only: file is not valid UTF-8`
  in the status bar. Saving a lossily decoded file would silently corrupt it. A UTF-8
  BOM is stripped for rendering and restored on save.
- Soft-wrap long lines at the viewport width using `unicode-width` for correct CJK
  and emoji columns, unless `--wrap` sets a hard width.

Acceptance criteria:
- [ ] A 100,000-line document scrolls at a sustained 60 fps with no visible stutter.
- [ ] Opening a 5 MB Markdown file paints the first frame in under 100 ms.
- [ ] Snapshot tests cover: nested lists, a table with mixed alignment, a fenced code
      block in Rust and in an unknown language, a blockquote containing a list, a
      task list, footnotes, and frontmatter.
- [ ] Editing one paragraph in a 10,000-line document re-renders only that block
      (assert via a render-count instrumentation hook behind `#[cfg(test)]`).
- [ ] A terminal resize re-renders every visible block exactly once and no
      off-screen block.
- [ ] A 400-column line inside a fenced code block is clipped, not wrapped, and
      `l` scrolls it horizontally.

### 9.3 Viewport and scrolling

- `j`/`k`, `↓`/`↑` — one line
- `Ctrl+D` / `Ctrl+U` — half page
- `PageDown` / `PageUp`, `Space` / `b` — full page
- `gg` / `G` — top / bottom (`g` alone is a prefix and never acts on its own)
- `{` / `}` — previous / next heading
- `h` / `l` — horizontal scroll (code blocks and tables wider than the viewport)
- Mouse wheel — three lines, configurable
- Scroll position is per tab and restored on tab switch and on history navigation.

### 9.4 Tabs

- `Ctrl+T` new tab (welcome screen), `Ctrl+W` close tab, `Ctrl+PageDown` /
  `Ctrl+PageUp` and `gt` / `gT` to switch, `Alt+1`..`Alt+9` conflict with sidebar
  modes so do not bind them to tabs.
- Closing the last tab quits, prompting if any tab is dirty.
- Tab label: frontmatter `title` if present, otherwise file stem, truncated to 20
  columns with a middle ellipsis. Dirty tabs prefixed with `●`.
- Maximum 20 tabs; beyond that, reuse the active tab and show a status message.
- Each tab owns its own independent history stack.

### 9.5 Link navigation and history

This is the feature that distinguishes perga. Get it right.

- **Inline links** — `[text](target)`. Resolve `target` relative to the *containing
  document's* directory, not the vault root.
- **Anchors** — `../api/auth.md#obtaining-a-token` and bare `#section` within the
  current document. Slugify headings GitHub-style: lowercase (Unicode-aware), spaces
  to hyphens, strip punctuation and symbols but **keep all Unicode letters and
  digits** — `## Kurulum Kılavuzu` must become `kurulum-kılavuzu`, not
  `kurulum-klavuzu`. Deduplicate collisions with `-1`, `-2`. This single function
  serves anchors, the outline, and `[[Page#Heading]]` wiki-link fragments.
- **External links** — `http(s)://` and `mailto:` open with `xdg-open` in a detached
  process. Never block the UI. If `xdg-open` is missing, show the URL in the status
  bar and offer to copy it via OSC 52. **Pass the URL as a single `argv` element via
  `std::process::Command`; never route it through a shell.** Link targets are
  untrusted input from arbitrary Markdown files.
- **Directory targets** — reveal in the file tree instead of opening.
- **Non-Markdown targets** — open with `xdg-open`.
- **Broken links** — style with `link_broken`, and on activation show
  `Cannot resolve: <target>` in the status bar. Never crash, never create files.
- **Navigation:** `n` / `N` cycle links in reading order (scrolling to bring the
  focused link into view), `Enter` follows the focused link, `f` enters hint mode.
- **History:** per tab, `Alt+←` back and `Alt+→` forward, with `H` and `L` as
  always-available aliases (tmux and several terminals intercept `Alt+arrow`). Following a link pushes the
  current `Location` including scroll offset. Going back restores that exact scroll
  offset, not the top of the document. Following a new link after going back
  truncates the forward stack. History depth capped at 100 entries per tab.
- `Ctrl+Enter` on a link opens the target in a new background tab. Because legacy
  terminals cannot distinguish `Ctrl+Enter` from `Enter` (see Section 8.6), the
  sequence `t` `Enter` does the same thing everywhere.
- **Broken wiki-links are the exception to "never create files":** activating an
  unresolved `[[wiki-link]]` shows a confirm overlay offering to create the target
  as a new file (see Section 9.11). Activating a broken *inline* link never
  offers creation.

Acceptance criteria:
- [ ] Following a relative link two directories up and one down resolves correctly.
- [ ] Back after a link follow restores the previous scroll position exactly.
- [ ] Forward stack is truncated when a new navigation occurs after going back.
- [ ] An anchor-only link scrolls without reloading the document.
- [ ] A link to a file outside the vault root opens correctly and does not corrupt
      the tree or index.
- [ ] Cyclic links (A → B → A) do not grow memory unboundedly.

### 9.6 Wiki-links and backlinks

- Syntax: `[[Page Name]]`, `[[Page Name|Display Text]]`, `[[Page Name#Heading]]`,
  and `[[folder/Page Name]]`.
- Resolution order with `wikilinks.resolution = "path-first"` (the default):
  1. Exact relative path from the vault root.
  2. Exact filename match anywhere in the vault (unique match wins).
  3. Case-insensitive filename match.
  4. If multiple candidates remain, show a disambiguation overlay listing them.

  `"filename-first"` swaps steps 1 and 2. The `#Heading` fragment, when present, is
  resolved against the target document with the same slug function as inline
  anchors.
- Extensions searched: `wikilinks.extensions`, default `["md", "markdown"]`.
- **Backlink index:** a map from canonical path → set of `(source_path, line, context
  snippet)`. Built in a background thread on startup by walking the vault and
  extracting links from every Markdown file. Incrementally updated on file change
  events. Cached to `$XDG_CACHE_HOME/perga/<vault-hash>/index.bin`, serialised with
  `postcard`, with a format version field.
- **Cache validation is per file, not per vault.** Directory mtimes do not change
  when a nested file changes, and an "aggregate mtime" would itself require a full
  walk. Store `(relative_path, mtime, size)` for every indexed file. On startup the
  tree walker (which runs anyway) reports each file it sees; compare against the
  cached entry and re-parse only files that are new or whose mtime/size differ. Drop
  entries for files that no longer exist. Discard the whole cache on a format
  version mismatch.
- The index must never block the UI. Until it is ready, the Links sidebar mode shows
  `Indexing… (n/m files)` with live progress.
- `wikilinks.enabled = false` disables parsing and indexing entirely, and the Links
  mode then shows only outgoing inline links.

Acceptance criteria:
- [ ] A vault of 10,000 files indexes in under 3 seconds on a warm page cache.
- [ ] The UI remains responsive throughout indexing (verified by the first-frame
      benchmark in `benches/`; not a CI-gating assertion — see Section 15.6).
- [ ] Editing a file and saving updates its backlinks within 500 ms.
- [ ] Ambiguous wiki-link targets produce a disambiguation overlay, not a silent pick.
- [ ] Deleting a file removes it from the index and marks inbound links broken.

### 9.7 Project-wide search

- `Ctrl+Shift+F` (with `Ctrl+G` as an alias, since legacy terminals cannot deliver
  `Ctrl+Shift` combinations — see Section 8.6) opens a search prompt. Results
  populate the Search sidebar mode.
- With the Search sidebar mode focused, `/` reopens the prompt pre-filled with the
  last query so it can be edited and re-run.
- Engine: `grep-searcher` with `grep-regex`. Literal by default; `search.regex = true`
  or a `/pattern/` prompt prefix enables regex.
- Smart case by default: case-insensitive unless the query contains an uppercase
  character.
- Search respects the same ignore rules as the tree.
- Streaming results: display hits as they arrive, do not wait for completion. Cap at
  `search.max_results` (default 1,000) and indicate truncation.
- A new query cancels the in-flight search. Use an atomic cancellation flag checked
  by the searcher's sink.

Acceptance criteria:
- [ ] Searching a 10,000-file vault shows first results within 200 ms.
- [ ] Cancelling mid-search leaves no orphaned threads (verify thread count).
- [ ] Regex mode with an invalid pattern shows an inline error, not a panic.

### 9.8 Editor mode

- `e` enters edit mode on the active document. `i` also enters (Vim familiarity).
- Implemented with `tui-textarea`: undo/redo, selection, word motions, and its own
  search are provided; do not reimplement them.
- Line numbers in the gutter, current line highlighted.
- `Ctrl+S` saves. `Esc` returns to read mode; if dirty, show a confirm overlay with
  Save / Discard / Cancel.
- Dirty state is per tab, shown in the tab label and the status bar.
- On save: write atomically (write to a temporary file in the same directory, then
  `rename`) so a crash cannot truncate the user's file. Preserve the original file
  permissions.
- **Conflict detection:** record mtime at load. If mtime changed at save time, show a
  confirm overlay offering Overwrite / Reload-and-discard / Cancel. Never silently
  overwrite.
- On save: re-parse, re-render affected blocks, update the outline, and update the
  backlink index for that file.
- `soft_wrap` in edit mode follows `editor.wrap`, default off (edit the real lines).
- **Position round-tripping via the offset↔line map:** entering edit mode places the
  cursor on the source line corresponding to the first visible rendered line, so the
  user lands where they were reading. Leaving edit mode scrolls the rendered view so
  the line containing the cursor is visible, positioned at the same relative screen
  row where possible. The user must never lose their place crossing the boundary in
  either direction.
- `o` opens the file in `$EDITOR` (or `editor.external_command`), suspending the TUI
  properly: leave raw mode and the alternate screen, restore terminal state, run the
  editor synchronously, then re-enter and reload the file. This must work correctly
  when the editor itself is a TUI application.
- `editor.autosave` (default `false`): when true, save on leaving edit mode and every
  `editor.autosave_interval_secs`.

Acceptance criteria:
- [ ] A crash during save (simulate with a fault injection point behind `#[cfg(test)]`)
      never leaves a truncated file.
- [ ] External `$EDITOR` handoff with `vim` returns to a correctly restored perga.
- [ ] Editing then navigating away prompts, and Cancel keeps the user in place.
- [ ] File permissions and ownership are preserved across save.
- [ ] Editing a file with CRLF line endings preserves them.

### 9.9 Live reload

- Watch the vault root recursively with `notify`.
- Debounce events by 200 ms (coalesce bursts from editors that write-rename).
- On change to the **active document**: if not dirty, reload and preserve scroll
  position and focused link index as closely as possible. If dirty, show a
  non-blocking status bar notice `File changed on disk (r to reload)` and do not
  clobber the buffer.
- On change elsewhere: update the tree and incrementally update the index.
- **Ignore perga's own writes.** After saving, record `(path, new_mtime)`; when the
  watcher reports that path with that mtime, drop the event. Otherwise every save
  triggers a reload race against the freshly cleared dirty flag.
- On the vault root being deleted or becoming unreadable: show an error state, keep
  the application alive.
- `r` (viewport focused) forces a reload of the active document.
- Watching is disabled by `watch.enabled = false`, and automatically disabled with a
  status warning if the platform inotify watch limit is hit (a large vault can exceed
  `max_user_watches`; detect the error and degrade to manual reload rather than
  failing).

### 9.10 Session persistence

Persist **one session file per vault root**, at
`$XDG_STATE_HOME/perga/sessions/<vault-root-hash>.toml` (fall back to
`$XDG_DATA_HOME/perga/`). A single global session file would let two vaults
overwrite each other's state. Each file carries a format version field and is
discarded on mismatch. Contents:
- Open tabs (paths and scroll offsets), active tab index
- Sidebar visibility, width, and mode
- Last active theme if overridden at runtime
- Recent files list, capped at 50

Restored only when perga is launched with no `PATH` argument; the vault root is then
the current directory and its hash selects the session file. `session.restore = false`
disables. Corrupt session files are ignored with a log line, never a crash. The
recent-files list is also the source for the quick switcher's empty-query results
(Section 8.3).

### 9.11 Creating and renaming files

An editor without a way to create a new note is half a feature. This is the Obsidian
workflow that must work end to end.

- **`Ctrl+N`** opens a single-line prompt for a path relative to the vault root,
  pre-filled with the directory of the active document (or the selected tree
  directory when the sidebar has focus). `.md` is appended when no extension is
  given. Intermediate directories are created. The file is created empty (or with
  `---\ntitle: <stem>\n---\n` when `editor.new_file_frontmatter = true`), opened in a
  new tab, and edit mode is entered immediately.
- **From a broken wiki-link:** activating `[[Some Page]]` that resolves to nothing
  shows a confirm overlay: `Create "Some Page.md" in <dir>?` with the directory
  defaulting to `wikilinks.new_file_dir` (empty means the active document's
  directory). Confirming creates and opens it in edit mode, and the source
  document's link is now resolved without any change to it.
- **From the quick switcher:** a query matching no file offers a final
  `Create "<query>.md"` entry (Section 8.3).
- **Rename:** `r` on a tree entry, or `R` in the viewport for the active document,
  prompts with the current name pre-filled. On confirm: rename atomically, update
  every open tab pointing at the old path, update the tree, and update the index so
  backlinks to the renamed file resolve. **Do not rewrite links in other documents**
  in v1 — Obsidian does this, but doing it wrong corrupts a vault; instead, show a
  status message with the count of documents that now have a broken link to the old
  name, and let the search mode find them.
- Refuse to create or rename onto an existing path; refuse paths that escape the
  vault root after normalisation; refuse names containing path separators in the
  rename prompt.
- **Deletion is not in v1.** It is irreversible, and every other operation here is
  not. Revisit in v2 with a trash-directory design.

Acceptance criteria:
- [ ] `Ctrl+N` with `notes/ideas` creates `notes/ideas.md`, creating `notes/` if
      absent, and lands in edit mode with the cursor at line 1.
- [ ] Following a broken `[[X]]`, confirming creation, and pressing back returns to
      the source document where `[[X]]` is now styled as resolved.
- [ ] Renaming a file open in two tabs updates both tab labels and both paths.
- [ ] Renaming to a path outside the vault (`../../etc/x`) is refused with a clear
      message.

### 9.12 Print mode (no TUI)

`perga --print FILE` and `perga FILE` with a non-terminal stdout render the document
through the same block pipeline and write ANSI-styled text to stdout, then exit. This
serves `perga README.md | less -R`, quick previews, and scripting, and is the
behaviour users of `glow` and `mdcat` expect from a Markdown tool. Width is the
terminal's when stdout is a TTY (with `-p`), otherwise `--wrap` if set, otherwise 80.
Theme and `NO_COLOR` apply. No alternate screen, no cursor control, no mouse, no
input reading. A directory argument in print mode is an error (exit 2).

---

## 10. Configuration

Location, in precedence order (later overrides earlier):
1. Built-in defaults
2. `$XDG_CONFIG_HOME/perga/config.toml` (i.e. `~/.config/perga/config.toml`)
3. `.perga.toml` in the vault root, if `general.allow_local_config = true` (default
   `true`). **A vault-local config is untrusted input** — it arrives with any cloned
   repository. It may set only presentation and navigation keys (`[ui]`, `[theme]`,
   `[files]`, `[wikilinks]`, `[search]`, `[session]`, `general.wrap`,
   `general.tab_width`). It must never be able to set `editor.external_command` or
   any other key that names a program to execute, and `[keys]` remaps from a local
   file are ignored. Keys outside the allow list produce a startup warning naming
   the file and are dropped.
4. `--config <FILE>`
5. Individual CLI flags

`--no-config` skips 2, 3, and 4.

Unknown keys produce a warning in the status bar on startup and are otherwise
ignored — never a hard error, so that a config written for a newer version still
works. Invalid *values* (a bad colour, an unparseable key binding) produce a warning
naming the key and fall back to the default for that key only.

### 10.1 Complete default config

Ship this verbatim as `docs/configuration.md`'s reference block and as the output of
a `perga --generate-config` flag (add this flag; it is trivial and much appreciated).

```toml
[general]
# Directory opened when no PATH argument is given.
start_path = "."
# Read .perga.toml from the vault root (presentation keys only — see Section 10).
allow_local_config = true
# Follow symlinks while walking the vault.
follow_symlinks = false
# Hard wrap width; 0 fits the viewport.
wrap = 0
# Expand tabs in source to this many spaces when rendering.
tab_width = 4

[ui]
sidebar_visible = true
sidebar_width = 32
# files | search | outline | links
sidebar_default_mode = "files"
# Show the tab bar even with a single tab open.
always_show_tabs = false
show_line_numbers = false
show_status_bar = true
# Lines scrolled per mouse wheel notch.
mouse_scroll_lines = 3
# Capture the mouse. Off leaves terminal text selection available.
mouse = true
# Auto-hide the sidebar below this terminal width.
narrow_threshold = 80

[files]
# Include dotted directories such as .github and .claude.
include_hidden = true
# Honour .gitignore, .ignore, and global gitignore.
respect_gitignore = true
# Show files that are not Markdown.
show_all = false
# name | mtime | size
sort = "name"
sort_reverse = false
# Additional extensions treated as Markdown.
extensions = ["md", "markdown", "mdx"]
# Note: .git, .hg, .svn, and .jj are always excluded and this is not configurable.

[theme]
# auto | dark | light | high-contrast | <filename in theme.dir>
# auto picks dark or light from the terminal background (Section 11.3).
name = "auto"
# Extra theme directory; defaults to $XDG_CONFIG_HOME/perga/themes
dir = ""
# syntect theme for fenced code blocks.
code_theme = "base16-ocean.dark"

[wikilinks]
enabled = true
extensions = ["md", "markdown"]
# path-first | filename-first
resolution = "path-first"
# Build the backlink index at startup.
index_on_start = true
# Cache the index between runs.
cache = true
# Where files created from a broken [[wiki-link]] go; empty = active document's dir.
new_file_dir = ""

[search]
max_results = 1000
regex = false
# Case-insensitive unless the query contains uppercase.
smart_case = true
# Include non-Markdown files in project search.
all_files = false

[editor]
# Command for `o`; $EDITOR is used when empty.
external_command = ""
tab_size = 4
insert_spaces = true
wrap = false
autosave = false
autosave_interval_secs = 30
# Show whitespace characters.
show_whitespace = false
# Seed new files created with Ctrl+N with a frontmatter title.
new_file_frontmatter = false

[watch]
enabled = true
debounce_ms = 200

[session]
restore = true
max_recent = 50

[keys]
# Remap any action. See docs/keybindings.md for action names and key syntax.
# Example:
# "toggle_sidebar" = "ctrl+space"
# "quit" = ["q", "ctrl+q"]
# "scroll_top" = "g g"          # sequences are space-separated tokens
```

### 10.2 Key binding syntax

- Modifiers: `ctrl+`, `alt+`, `shift+`, combinable, lowercase, in that order.
- Named keys: `enter`, `esc`, `tab`, `backtab`, `space`, `backspace`, `delete`,
  `insert`, `home`, `end`, `pageup`, `pagedown`, `up`, `down`, `left`, `right`,
  `f1`..`f12`.
- Characters as themselves: `q`, `?`, `/`.
- Sequences are **space-separated tokens**: `"g g"`, `"g t"`, `"g T"`, `"m 1"`,
  `"t enter"`. A single token is one key; two or more tokens are a sequence. Never
  write `"gt"` — that is ambiguous with a named key and is rejected with a warning.
- An action may map to a list of bindings.
- Conflicts: last definition wins, and a warning names the shadowed action.
- The help overlay is generated from the resolved keymap, so remappings are always
  reflected.

---

## 11. Theming

Themes are TOML files. The three built-ins are embedded with `include_str!` from
`themes/`. User themes are loaded from `theme.dir` (default
`$XDG_CONFIG_HOME/perga/themes/`) by filename without extension.

### 11.1 Theme schema

Every key is optional; missing keys inherit from the `dark` built-in. Colours accept
`#rrggbb`, an ANSI index `0`-`255`, or a named ANSI colour
(`black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`, `white`, and their
`bright_` variants). Style tables accept `fg`, `bg`, `bold`, `italic`, `underline`,
`dim`, `reversed`, `crossed_out`.

```toml
name = "dark"
# Optional: which syntect theme to use for code blocks; overrides theme.code_theme.
code_theme = "base16-ocean.dark"

[ui]
background      = { bg = "#1e1e2e" }
border          = { fg = "#45475a" }
border_focused  = { fg = "#89b4fa" }
title           = { fg = "#cdd6f4", bold = true }
status_bar      = { fg = "#a6adc8", bg = "#181825" }
status_mode     = { fg = "#1e1e2e", bg = "#89b4fa", bold = true }
status_warning   = { fg = "#f9e2af" }
status_error     = { fg = "#f38ba8" }
selection       = { bg = "#313244" }
scrollbar       = { fg = "#45475a" }
logo            = { fg = "#89b4fa", bold = true }
logo_subtitle   = { fg = "#6c7086" }

[tabs]
active   = { fg = "#cdd6f4", bg = "#313244", bold = true }
inactive = { fg = "#6c7086" }
dirty    = { fg = "#f9e2af" }

[sidebar]
directory     = { fg = "#89b4fa", bold = true }
file          = { fg = "#cdd6f4" }
file_active   = { fg = "#a6e3a1", bold = true }
file_other    = { fg = "#6c7086", dim = true }
mode_active   = { fg = "#1e1e2e", bg = "#89b4fa", bold = true }
mode_inactive = { fg = "#6c7086" }
match         = { fg = "#f9e2af", bold = true }
line_number   = { fg = "#6c7086" }

[markdown]
h1 = { fg = "#f38ba8", bold = true }
h2 = { fg = "#fab387", bold = true }
h3 = { fg = "#f9e2af", bold = true }
h4 = { fg = "#a6e3a1", bold = true }
h5 = { fg = "#89b4fa", bold = true }
h6 = { fg = "#cba6f7", bold = true }
text            = { fg = "#cdd6f4" }
emphasis        = { italic = true }
strong          = { bold = true }
strikethrough   = { crossed_out = true }
blockquote      = { fg = "#a6adc8", italic = true }
blockquote_bar  = { fg = "#585b70" }
code_inline     = { fg = "#f5c2e7", bg = "#313244" }
code_block_bg   = { bg = "#181825" }
link            = { fg = "#89b4fa", underline = true }
link_focused    = { fg = "#1e1e2e", bg = "#89b4fa" }
link_broken     = { fg = "#f38ba8", crossed_out = true }
link_external   = { fg = "#94e2d5", underline = true }
wikilink        = { fg = "#cba6f7", underline = true }
list_marker     = { fg = "#fab387" }
task_done       = { fg = "#a6e3a1" }
task_todo       = { fg = "#6c7086" }
table_border    = { fg = "#45475a" }
table_header    = { fg = "#cdd6f4", bold = true }
rule            = { fg = "#45475a" }
footnote        = { fg = "#94e2d5" }
image_placeholder = { fg = "#94e2d5", italic = true }
html            = { fg = "#6c7086", dim = true }
frontmatter     = { fg = "#6c7086", dim = true }

[hints]
label = { fg = "#1e1e2e", bg = "#f9e2af", bold = true }
```

### 11.2 Built-in themes to ship

- **`dark`** (default) — the palette above.
- **`light`** — a genuinely usable light theme, not an inversion. Verify contrast
  ratios are at least 4.5:1 for body text.
- **`high-contrast`** — pure black/white with ANSI-16 colours only, so it works on
  terminals without truecolour and serves accessibility needs. Must not rely on any
  256-colour or truecolour value.

### 11.3 Theme behaviour

- `--theme <name>` overrides the config.
- With `theme.name = "auto"` (the default), detect the terminal background
  (`COLORFGBG`, then an OSC 11 query with a 50 ms timeout) and pick `dark` or
  `light`. If detection fails or times out, use `dark`. The query must not delay the
  first frame: paint with `dark`, and swap to `light` on the next frame if the reply
  says so.
- Truecolour detection via `COLORTERM`; when absent, degrade gracefully by mapping
  hex colours to the nearest ANSI-256 value rather than failing.
- Reload themes at runtime when the theme file changes (the watcher is already
  running; extend it to the theme directory). This makes theme authoring pleasant.
- Document the full schema in `docs/theming.md` with a complete worked example of
  writing a custom theme from scratch.

---

## 12. Keybindings reference

Ship this table in `docs/keybindings.md` and derive the help overlay from the same
source of truth (a static table of `(Action, default_binding, description)`).

**Global**

| Key | Action |
|---|---|
| `q`, `Ctrl+Q` | Quit (prompts if any tab is dirty) |
| `?` | Help overlay |
| `Ctrl+B`, `Ctrl+E` | Toggle sidebar (`Ctrl+E` for tmux users) |
| `<` / `>` | Resize sidebar (sidebar focused) |
| `Tab` / `Shift+Tab` | Cycle focus |
| `m1`..`m4`, `Alt+1`..`Alt+4` | Sidebar mode: files / search / outline / links |
| `mm` | Toggle mouse capture |
| `Ctrl+N` | New file |
| `Ctrl+O` | Quick switcher |
| `Ctrl+F` | Find in document |
| `Ctrl+Shift+F`, `Ctrl+G` | Search project |
| `Ctrl+T` / `Ctrl+W` | New / close tab |
| `gt` / `gT`, `Ctrl+PageDown` / `Ctrl+PageUp` | Next / previous tab |
| `Esc` | Close overlay, or leave edit mode |

**Reading**

| Key | Action |
|---|---|
| `j` `k` `↓` `↑` | Scroll line |
| `Ctrl+D` / `Ctrl+U` | Half page |
| `Space` / `b`, `PageDown` / `PageUp` | Full page |
| `gg` / `G` | Top / bottom |
| `h` / `l` | Horizontal scroll |
| `{` / `}` | Previous / next heading |
| `n` / `N` | Next / previous link |
| `Enter` | Follow focused link |
| `Ctrl+Enter`, `t` `Enter` | Follow link in new tab |
| `f` | Link hint mode |
| `H` / `L`, `Alt+←` / `Alt+→` | History back / forward |
| `R` | Rename active document |
| `e`, `i` | Enter edit mode |
| `o` | Open in `$EDITOR` |
| `y` | Copy document path (OSC 52) |
| `r` | Reload active document |

**Sidebar (focused)**

| Key | Action |
|---|---|
| `j` `k` | Move selection |
| `l` `→` `Enter` | Expand / open |
| `h` `←` | Collapse / go to parent |
| `1`..`4` | Switch mode |
| `.` | Toggle hidden files |
| `a` | Toggle non-Markdown files |
| `/` | Filter tree by name (files mode) / edit last query (search mode) |
| `r` | Rename selected entry |

**Editing**

| Key | Action |
|---|---|
| `Ctrl+S` | Save |
| `Esc` | Leave edit mode |
| `Ctrl+Z` / `Ctrl+Y` | Undo / redo |
| Standard text editing motions | Provided by `tui-textarea` |

**Key ownership in edit mode.** `tui-textarea`'s default input map uses Emacs-style
bindings — `Ctrl+N`/`Ctrl+P` (line down/up), `Ctrl+F`/`Ctrl+B` (char forward/back),
`Ctrl+E`/`Ctrl+A` (line end/start), `Ctrl+W` (delete word), `Ctrl+U`, `Ctrl+K`,
`Ctrl+G` — which collide with perga's global bindings for new file, find, sidebar,
close tab, and search. Resolve this explicitly rather than letting whichever handler
runs first win: **in edit mode, the editor owns every key except** `Esc`, `Ctrl+S`,
`Ctrl+Q`, and `Ctrl+Z`/`Ctrl+Y`. Global actions are unavailable until the user
leaves edit mode. Document this in `docs/keybindings.md` under its own heading so
nobody files a bug that `Ctrl+W` did not close the tab while editing.

**`Ctrl+Z` outside edit mode** sends the process `SIGTSTP` after restoring the
terminal, so `fg` resumes perga with the screen intact — the behaviour terminal users
expect from a full-screen program. On `SIGCONT`, re-enter raw mode and the alternate
screen and redraw.

---

## 13. Error handling and logging

- **Never leave the terminal broken.** Install a panic hook that restores the
  terminal (leave alternate screen, disable raw mode, disable mouse capture and
  bracketed paste, pop keyboard enhancement flags, show cursor) *before* printing
  the panic, and do the same on any error path out of `main`. Test this by injecting
  a panic behind a hidden debug flag. Note that `panic = "abort"` in the release
  profile does not prevent this: the panic hook runs before the abort. It does mean
  `catch_unwind` is unavailable, which is fine — nothing in this design relies on it.
- **Handle `SIGTERM`, `SIGHUP`, and `SIGINT`** with `signal-hook`: restore the
  terminal exactly as above and exit with code 130 for `SIGINT`, 143 for `SIGTERM`.
  Handle `SIGTSTP`/`SIGCONT` for suspend and resume as described in Section 12.
  A user who runs `kill` on a stuck perga must not be left with a broken shell.
  Before exiting on a signal, write any dirty editor buffers to
  `$XDG_STATE_HOME/perga/recovery/<vault-hash>/<path-hash>.md` and offer to restore
  them the next time the same document is opened.
- Use `anyhow::Result` in application code with `.context()` at every I/O boundary.
- Use `thiserror` enums for `doc`, `vault`, `config`, and `theme` module errors.
- Logging with `tracing`, written to a file only — never to stdout or stderr while
  the TUI is active. Default: no logging. `--log <FILE>` or `PERGA_LOG=debug` enables.
- User-facing errors go to the status bar with a severity style; the log gets the
  full chain.
- Exit codes: `0` success, `1` runtime error, `2` usage/argument error.

---

## 14. Performance targets

Verify these with `criterion` benchmarks in `benches/` and the ignored timing tests
described in Section 15.6. They are targets to design toward and to measure, not
assertions that gate CI.

| Metric | Target |
|---|---|
| Cold start to first frame, 1,000-file vault | < 50 ms |
| Cold start to first frame, 10,000-file vault | < 100 ms (tree partially populated) |
| Full backlink index, 10,000 files | < 3 s, off-thread |
| Open a 5 MB document | < 100 ms to first frame |
| Scroll a 100,000-line document | sustained 60 fps |
| First search results, 10,000 files | < 200 ms |
| Idle CPU | 0% (block on input; no polling loop) |
| Resident memory, 10,000-file vault | < 150 MB |

Note the idle CPU requirement: the dedicated input thread and single blocking
`recv()` in Section 7.1 satisfy it by construction. A `poll(16ms)` loop on the main
thread burns a core, drains laptop batteries, and will be the first issue filed.

---

## 15. Testing requirements

### 15.1 Unit tests
Every module with logic gets tests. Particular attention to:
- Link resolution: relative paths, `..` traversal, anchors, absolute paths, paths
  outside the vault, URL-encoded targets, Windows-style separators in the source.
- Heading slugification, including duplicate headings and punctuation.
- Wiki-link resolution for all four syntaxes and all resolution orders.
- Key binding string parsing, including invalid input and sequences.
- Config precedence: all five layers, with partial overrides.
- Theme deserialization and inheritance from defaults.
- The byte-offset ↔ line-number map, including after edits.

### 15.2 Snapshot tests
Use `insta` with `ratatui::backend::TestBackend` at several sizes (120x40, 80x24,
40x10). Snapshot the full frame for: welcome screen, document with sidebar open,
sidebar hidden, each of the four sidebar modes, each overlay, edit mode, link hint
mode, terminal-too-small state, and each of the three built-in themes.

### 15.3 Integration tests
Drive the app by feeding `Action` sequences and asserting on state:
- Open → follow link → back → assert scroll restored.
- Open → edit → navigate away → assert confirm overlay.
- Search → open result → assert scrolled to hit line.
- Index → rename a file on disk → assert backlinks updated.
- Two tabs with independent histories → assert no cross-contamination.

### 15.4 Test fixtures
Build `tests/fixtures/vault/` containing, at minimum:
- Nested directories three levels deep
- A dotted directory (`.github/`) with a Markdown file in it
- A `.gitignore` that ignores one directory, with a Markdown file in it
- Documents with: every GFM feature, deeply nested lists, a wide table, CJK text,
  emoji, a 50,000-line document **generated at test time by a helper in
  `tests/common/` and excluded from Git** (committing it bloats every clone), a file
  with CRLF endings, a file with no
  trailing newline, an empty file, a file that is only frontmatter
- Broken inline links and broken wiki-links
- An ambiguous wiki-link target (same filename in two directories)
- A symlink loop (to verify the walker does not hang)
- A file with a name containing spaces, unicode, and a `#`

### 15.5 CI
`.github/workflows/ci.yml` on push and PR:
- `cargo fmt --all -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test --all-features`
- Build for `x86_64-unknown-linux-gnu` and `x86_64-unknown-linux-musl`
- Build with the pinned MSRV
- `cargo publish --dry-run`
- `cargo deny check` — licences, advisories, bans, and sources (see Section 16.6).
  Install with `taiki-e/install-action` or `cargo-binstall`, not `cargo install`,
  to keep CI fast.
- Cache with `Swatinem/rust-cache` or equivalent
- **No timing assertions in this workflow.** See Section 15.6.

### 15.6 Benchmarks and performance verification
The performance targets in Section 14 are verified with `criterion` benchmarks in
`benches/`, plus integration tests that measure time but are marked `#[ignore]` and
run with `cargo test -- --ignored` in a separate, non-blocking `bench.yml` workflow
that runs nightly and on demand. Shared CI runners have unpredictable performance;
gating `main` on wall-clock assertions produces flaky failures, and flaky failures
get "fixed" by loosening the assertion until it proves nothing. The nightly job
uploads results as an artifact and posts a warning, never a failure, when a target
regresses by more than 20%.

---

## 16. Packaging

### 16.1 Cargo.toml metadata
Required for publishing and for downstream packagers:
`name`, `version`, `edition`, `rust-version`, `description`, `documentation`,
`homepage`, `repository`, `license = "MIT OR Apache-2.0"`, `keywords` (markdown,
tui, terminal, viewer, notes), `categories` (command-line-utilities,
text-processing), `readme`, `exclude` (tests/fixtures, .github, packaging — keep the
published crate small).

### 16.2 Static builds
Primary release targets:
- `x86_64-unknown-linux-musl`
- `aarch64-unknown-linux-musl`

musl avoids the `GLIBC_2.xx not found` class of bug on older distributions. Verify
the `syntect` feature configuration from Section 5.2 — the default `onig` backend
will fail these builds.

### 16.3 dist
Run `dist init` (the tool formerly named `cargo-dist`) and configure:
- The two musl targets above
- Shell installer, tarball artifacts, checksums
- `aarch64` cross-compilation via `cargo-zigbuild`
- GitHub Releases as the host, triggered by pushing a `v*` tag
- Attestations enabled

Commit the generated `.github/workflows/release.yml` unmodified apart from
documented changes.

### 16.4 Distribution packages
Create in `packaging/`:
- **`PKGBUILD`** for an AUR `perga-bin` package that downloads the release tarball
  for the host architecture and installs the binary, man page, completions, and both
  licence files. Include `sha256sums` placeholders and a note in
  `packaging/README.md` on updating them per release.
- **`cargo-deb` metadata** in `Cargo.toml` (`[package.metadata.deb]`) with correct
  section (`utils`), priority, extended description, and asset list including man
  page and completions.
- **`cargo-generate-rpm` metadata** similarly.

Do not attempt Snap or Flatpak. Their default confinement prevents reading arbitrary
directories, which is the core function of this application; `classic` confinement
requires a review process that file-browsing tools generally fail.

### 16.5 Man page and completions
Generate at runtime only, via `--generate-man` and `--generate-completions <shell>`
(no `build.rs`: sharing the `clap` definition with a build script means duplicating
or `#[path]`-including the CLI module, which is fragile for no benefit). The release
pipeline and every packaging artefact invoke the freshly built binary to produce the
man page and the bash, zsh, and fish completions, and install them to the standard
locations. Ship them in every release tarball. Packagers ask for these first;
retrofitting them later is tedious.

### 16.6 Licensing and legal hygiene

perga is dual licensed under `MIT OR Apache-2.0`. This is the de facto standard for
the Rust ecosystem — `rustc` itself, `ratatui`, `crossterm`, and `pulldown-cmark` all
use it. Apache-2.0 contributes an explicit patent grant; MIT contributes brevity and
GPL compatibility. The user picks whichever suits them. Do not substitute a different
licence, and do not add a third.

For context, the comparable projects are also permissive: `glow` and `frogmouth` are
MIT, `mdcat` is MPL-2.0.

Required artefacts:

- **`LICENSE-MIT`** — full MIT text, copyright line `Copyright (c) 2026 ankasoft`.
- **`LICENSE-APACHE`** — full, unmodified Apache License 2.0 text. Do not paste the
  appendix boilerplate into source file headers; the SPDX expression in `Cargo.toml`
  plus these two files is sufficient.
- **`Cargo.toml`** — `license = "MIT OR Apache-2.0"` as an SPDX expression. Do not
  use `license-file`; crates.io and downstream packaging tools parse the expression.
- **README licence section**, verbatim in this form:

  ```markdown
  ## License

  Licensed under either of

  - Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
  - MIT license ([LICENSE-MIT](LICENSE-MIT))

  at your option.

  ### Contribution

  Unless you explicitly state otherwise, any contribution intentionally submitted
  for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
  dual licensed as above, without any additional terms or conditions.
  ```

  That contribution paragraph is the standard Rust formulation and removes any need
  for a CLA. **Do not add a CLA or a DCO sign-off requirement.** For a project this
  size they deter contributors and buy nothing.
- **No `NOTICE` file.** Apache-2.0 requires preserving one if it exists; it does not
  require creating one. Omit it.
- **No per-file licence headers.** Keep source files clean.
- **`deny.toml`** at the repository root, enforced in CI:
  - `[licenses]` (American spelling — this is the actual `cargo-deny` key) — an allow list covering the permissive set actually present in the
    dependency tree (MIT, Apache-2.0, Apache-2.0 WITH LLVM-exception, BSD-2-Clause,
    BSD-3-Clause, ISC, Unicode-3.0, Zlib, MPL-2.0). If a new dependency introduces a
    licence outside the list, replace the dependency rather than widening the list,
    and record the decision in `docs/decisions.md`.
  - `[advisories]` — deny on any RUSTSEC vulnerability, warn on unmaintained crates.
  - `[bans]` — deny multiple major versions of the same crate where avoidable; deny
    `openssl`, `openssl-sys`, `native-tls`, and any HTTP client outright, matching
    the forbidden-dependency rule in Section 5.2.
  - `[sources]` — allow crates.io only; deny git dependencies.
- **`docs/licensing.md`** — one short page stating the licence, what it means for
  users and for packagers, and how to regenerate a dependency licence report with
  `cargo deny check licenses`. Distribution packagers read this first.

---

## 17. Milestones

Work through these in order. Each milestone ends with a commit (or a short series of
commits) and must satisfy its definition of done before moving on. Do not skip ahead,
and do not stop for approval between milestones.

### M0 — Repository foundation
- `cargo init --bin`, crate named `perga`.
- Licensing artefacts per Section 16.6: `LICENSE-MIT`, `LICENSE-APACHE`, the SPDX
  expression in `Cargo.toml`, `deny.toml`, and `docs/licensing.md`. No CLA, no
  `NOTICE`, no per-file headers.
- `README.md` (initial version — expanded in M11), `CHANGELOG.md`,
  `CONTRIBUTING.md`, `.gitignore`.
- Full `Cargo.toml` metadata per Section 16.1 and the release profile per Section 5.3.
- `.github/workflows/ci.yml` per Section 15.5.
- Module skeleton per Section 6 with `//!` doc comments stating each module's
  responsibility. No stub functions for non-goals.
- `docs/decisions.md` created with the first entry recording the dependency choices.
- **A commit-message guard.** Add `.githooks/commit-msg` that rejects any message
  matching (case-insensitively) `claude`, `anthropic`, `co-authored-by`, `generated
  with`, `🤖`, or `ai-assisted`, and set `git config core.hooksPath .githooks` in the
  repository. Add the same check as a CI step over the commits in the push range.
  Rule 0.5 is enforced mechanically, not by memory.

**Done when:** CI is green including `cargo deny check` and the commit-message
check, `cargo publish --dry-run` succeeds, and `cargo run -- --version` prints the
version and exits.

### M1 — Terminal shell and event loop
- Terminal setup/teardown, alternate screen, raw mode, mouse capture, bracketed
  paste, keyboard enhancement flags with capability detection (Section 8.6).
- Panic hook and signal handlers restoring the terminal (Section 13).
- `Action` enum, `App::update`, blocking event loop with 0% idle CPU.
- **The keymap infrastructure in full:** the static `(Action, default_bindings,
  description)` table, key-string parsing, sequence handling with the pending-key
  state, and the help overlay generated from the table. Bindings are the built-in
  defaults only at this stage; loading user remaps from the config file is M10. Build
  this now so that M2–M9 bind keys through the table rather than hardcoding
  `KeyCode` matches that M10 would have to rip out.
- **The `Theme` struct and the embedded `dark` theme**, with every style key from
  Section 11.1 resolved to a `ratatui::style::Style`. Loading themes from files and
  the other two built-ins are M10. Every widget from M1 onward takes its styles from
  `Theme`; nothing hardcodes a colour.
- Layout skeleton: title bar, sidebar placeholder, viewport placeholder, status bar.
- Focus cycling, `Ctrl+B` sidebar toggle, `q` quit, responsive breakpoints including
  the too-small state.
- Welcome screen per Section 8.5, including all three logo tiers, tier selection by
  viewport width, the onboarding block, and the short-viewport fallback.

**Done when:** the shell runs, resizes cleanly from 200x60 down to 30x8, uses 0% CPU
at idle, and restores the terminal after both `q` and an injected panic. Snapshot
tests at three sizes, plus dedicated welcome-screen snapshots at widths 100, 48, and
30 and at a height of 10 to prove each logo tier and the fallback render without
overflow.

### M2 — Markdown rendering pipeline
- `pulldown-cmark` block parsing with byte ranges.
- `tui-markdown` wrapper with the owned-line block cache and the offset↔line map.
- Viewport with all scroll bindings, scrollbar, and windowed rendering.
- `syntect` code highlighting with the mandated feature flags, loaded lazily
  off-thread.
- Frontmatter handling, image placeholders, GFM features, non-UTF-8 handling, code
  block clipping and horizontal scroll.
- `--print` / non-TTY mode: the same pipeline writing ANSI to stdout at the
  terminal's width (or 80 columns when unknown) and exiting.

**Done when:** the fixture corpus renders correctly at all three snapshot sizes, the
render-count instrumentation proves only visible blocks are rendered, and the
`scroll_large_document` benchmark exists and runs (its result is recorded in
`docs/decisions.md`, not asserted).

### M3 — Vault walking and the files sidebar
- `ignore`-based background walker streaming results to the UI.
- Lazy tree model, expand/collapse, sorting, hidden and non-Markdown toggles.
- Opening a file from the tree, active-file marker, tree filter (`/`).
- Auto-expansion of the path to the active document.

**Done when:** with a 10,000-file generated vault the first frame is painted before
the walker finishes (assert ordering, not wall-clock time), the `first_frame`
benchmark exists, and dotted directories appear by default.

### M4 — Link navigation and history
- Inline link extraction with rendered positions.
- Resolution of relative paths, anchors, directories, external and non-Markdown
  targets, and broken links.
- `n`/`N` cycling, `Enter` following, `f` hint mode.
- Per-tab history with exact scroll restoration and forward-stack truncation.

**Done when:** every acceptance criterion in Section 9.5 passes as an automated test.

### M5 — Tabs
- Tab bar, creation, closing, switching, labels with dirty markers, the 20-tab cap.
- Per-tab independent history, scroll, and find state.
- `Ctrl+Enter` background tab opening.

**Done when:** the cross-contamination integration test in Section 15.3 passes.

### M6 — Outline mode and in-document find
- Heading extraction, slugification, indented outline rendering.
- Live sync of the highlighted heading with scroll position.
- `Ctrl+F` incremental find with inline highlighting and match cycling.

**Done when:** anchor links from M4 navigate through the same slug implementation the
outline uses (one implementation, not two).

### M7 — Wiki-links and backlinks
- All four wiki-link syntaxes, both resolution orders, the disambiguation overlay.
- Background index builder with live progress, cache to `$XDG_CACHE_HOME`, and
  incremental updates.
- Links sidebar mode with outgoing and backlink sections.

**Done when:** every acceptance criterion in Section 9.6 passes, and the index cache
demonstrably shortens warm start.

### M8 — Search
- Project search with `grep-searcher`, streaming results, cancellation, smart case,
  regex mode.
- Search sidebar mode with grouped results and jump-to-line.
- Quick switcher with `nucleo`.

**Done when:** every acceptance criterion in Section 9.7 passes, including the
thread-leak check.

### M9 — Editor, file operations, and live reload
- `tui-textarea` integration, gutter, dirty tracking, confirm overlays, paste as a
  single undo step, edit↔read position round-tripping.
- File creation and renaming per Section 9.11, including creation from a broken
  wiki-link and from the quick switcher.
- Signal-triggered recovery files and their restoration prompt.
- Atomic save with permission preservation and mtime conflict detection.
- `$EDITOR` handoff with correct terminal suspend/restore.
- `notify` watcher with debounce, inotify-limit degradation, and dirty-buffer
  protection.

**Done when:** every acceptance criterion in Section 9.8 passes, including the
fault-injected save test, and `o` with `vim` round-trips cleanly; every acceptance
criterion in Section 9.11 passes; a paste of 5,000 lines into the editor completes in
one undo step in under 100 ms.

### M10 — Configuration, theming, session
- Full config schema, five-layer precedence, `--generate-config`, `--check-config`,
  tolerant handling of unknown keys and invalid values.
- Loading user keymap remaps from config into the M1 keymap table.
- The `light` and `high-contrast` themes, user theme loading from files, hot reload,
  terminal background detection, truecolour degradation, `NO_COLOR`.
- Session persistence and restoration.

**Done when:** a config that remaps ten actions and a hand-written custom theme both
work end to end; snapshot tests cover all three themes; a corrupt config and a
corrupt session file both degrade gracefully.

### M11 — Documentation, packaging, release
- `README.md`: what it is, the name origin from Section 1, an honest comparison
  table, installation for every method, quick start, keybinding summary, link to
  full docs, and the licence and contribution block exactly as written in
  Section 16.6.
- **Demo recording.** Add a `demo/` directory containing a `vhs` tape script that
  drives perga through a scripted session against `tests/fixtures/vault/`: open the
  vault, expand the tree, open a document, follow an inline link, go back, follow a
  wiki-link, view backlinks, run a project search, toggle the sidebar. Commit the
  tape and the generated GIF, and embed the GIF at the top of `README.md`. The tape
  must be deterministic and regenerable with a single command documented in
  `demo/README.md`. For a terminal application this asset is the single largest
  factor in whether anyone tries it.
- **Social preview asset.** Produce a 1280x640 PNG at `demo/social-preview.png`: a
  terminal screenshot of perga with the wordmark overlaid. This is the image link
  previews use on aggregators and chat clients; without it the repository shows a
  generic placeholder. Commit the file and note in `demo/README.md` that it must be
  uploaded manually under the repository's Settings → Social preview, since that is
  not settable from the repository contents.
- Do not put the ASCII logo at the top of `README.md`; that position belongs to the
  demo GIF.
- The comparison table must be factual and non-disparaging: state what each tool
  does and what perga adds, never what a competitor "fails" to do.
- `docs/usage.md`, `docs/configuration.md`, `docs/theming.md`,
  `docs/keybindings.md`, `docs/architecture.md`, `docs/publishing.md`,
  `docs/licensing.md`.
- `CHANGELOG.md` with a complete `0.1.0` entry.
- Man page and completions generation, verified.
- `dist init` and configuration, `packaging/PKGBUILD`, deb and rpm metadata.
- Tag `v0.1.0` and verify the release workflow produces installable artifacts for
  both architectures.
- `docs/publishing.md` must contain a release checklist: bump version, update
  `CHANGELOG.md`, regenerate the demo GIF, tag, wait for the release workflow,
  update `packaging/PKGBUILD` checksums, `cargo publish`.

**Environment caveat.** Some steps here need capabilities the agent may not have:
pushing tags, watching a GitHub Actions run, running a fresh container, or having
`vhs` installed. Do everything that is possible in the environment. For anything that
is not, prepare it completely (the tape file, the workflow, the tag command), verify
as far as possible locally (`dist build` for the host target, `dist plan`), and
record precisely what could not be verified and how the owner verifies it in
`docs/decisions.md`. Do not report a step as done that was not run.

**Done when:** a fresh container can install from the release tarball and run
`perga --version` (or, where that is impossible, `dist build` succeeds locally and the
gap is recorded); `docs/` covers every configurable key and every keybinding with no
gaps; `cargo publish --dry-run` succeeds.

---

## 18. Git workflow

- Work on `main` directly, or on short-lived branches merged with `--no-ff` — either
  is acceptable. Do not leave long-running branches.
- **Conventional Commits** format: `feat:`, `fix:`, `docs:`, `refactor:`, `test:`,
  `perf:`, `build:`, `ci:`, `chore:`. Scope where useful: `feat(vault): …`.
- Subject line in the imperative mood, 72 characters or fewer, no trailing period.
- Body explains *why* when the change is not self-evident. Wrap at 72.
- **No AI attribution of any kind** in any commit message, body, or trailer. See
  Section 0.5. This applies equally to PR descriptions and the changelog.
- One logical change per commit. Do not squash an entire milestone into one commit;
  aim for roughly 3-10 commits per milestone.
- Tag releases `v0.1.0` following SemVer.

---

## 19. Definition of done for the whole project

- [ ] All eleven milestones complete with their individual criteria met.
- [ ] `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` all clean.
- [ ] Every performance target in Section 14 measured and met.
- [ ] Every acceptance criterion in Section 9 covered by an automated test.
- [ ] All three built-in themes verified on a truecolour terminal and on an
      ANSI-16-only terminal.
- [ ] Static musl binaries built for both architectures and verified to run on a
      distribution with an old glibc.
- [ ] Man page and shell completions for bash, zsh, and fish generated and installed
      by every packaging artifact.
- [ ] `docs/` documents every config key, every keybinding, every theme key, and the
      full theme authoring workflow.
- [ ] `docs/decisions.md` records every ambiguity resolved during implementation.
- [ ] `cargo deny check` passes for licences, advisories, bans, and sources.
- [ ] Both licence files present, SPDX expression in `Cargo.toml`, and the README
      licence and contribution block matches Section 16.6 verbatim.
- [ ] Every packaging artefact installs both licence files.
- [ ] All three welcome-screen logo tiers render without overflow at their boundary
      widths, and remain legible under the `high-contrast` ANSI-16 theme.
- [ ] The `vhs` demo tape regenerates the README GIF with one documented command.
- [ ] No AI attribution anywhere in the repository history or contents.
- [ ] No non-goal from Section 4 implemented, and no dependency present that exists
      only to serve one.
- [ ] Terminal state correctly restored on normal exit, on error exit, on panic, and
      on `SIGTERM`/`SIGHUP`/`SIGINT`.
- [ ] Every binding that requires the kitty keyboard protocol has a plain fallback,
      and the help overlay lists both.
- [ ] The application is fully operable inside `tmux` with default tmux settings.
- [ ] `perga FILE | cat` produces clean ANSI output with no TUI control sequences.
- [ ] No performance assertion gates the main CI workflow.

---

## 20. Notes to the implementer

Three places account for most of the risk in this project. Budget accordingly.

1. **The owned-vs-borrowed rendering cache** (Section 7.2). `tui-markdown` hands back
   text borrowing its input. Every attempt to hold the source string, the parsed
   blocks, and the rendered lines in one struct will fight the borrow checker until
   you convert to owned lines at the cache boundary. Decide this on day one of M2;
   discovering it in M9 means rewriting the document layer.

2. **The offset↔line map** (Section 9.2). Five separate features depend on it: anchor
   scrolling, find highlighting, outline synchronisation, link positioning, and edit
   mode cursor round-tripping. Build it as a deliberate, tested data structure. If it
   emerges accidentally as a side effect of rendering, all five features will be
   subtly wrong in different ways.

3. **Background work and UI responsiveness** (Section 3.3, 3.4). The walker, indexer,
   searcher, and watcher all run off-thread and all report progress. Establish the
   channel-to-`Action` pattern in M1 and use it uniformly. Retrofitting async
   behaviour onto synchronous code paths is the usual reason terminal applications
   feel sluggish.

4. **The input layer** (Section 8.6 and the key-ownership rules in Section 12).
   Terminal input is where the neat design meets thirty years of incompatible
   emulators. Every advanced binding needs a fallback, edit mode needs an explicit
   ownership rule, sequences need a pending state, and paste needs bracketed mode.
   Build the key table and the sequence machine in M1 exactly as specified and route
   every later feature through it; the moment two places match on `KeyCode` directly,
   the conflicts described in Section 12 start appearing as unreproducible bugs.
