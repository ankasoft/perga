# Configuration

perga reads TOML. Every key has a default, so a configuration file is optional
and one that sets a single key inherits the rest.

## Where it comes from

Five layers, each overriding the one before it, key by key:

1. Built-in defaults
2. `$XDG_CONFIG_HOME/perga/config.toml` — usually `~/.config/perga/config.toml`
3. `.perga.toml` in the vault root, if `general.allow_local_config` is true
4. `--config <FILE>`
5. Individual command-line flags

`--no-config` skips layers 2, 3, and 4.

Layers merge rather than replace: a `--config` that sets `ui.sidebar_width`
leaves everything else the earlier layers set exactly as it was.

## A vault-local config is untrusted

`.perga.toml` arrives with any repository you clone, so it may set presentation
and navigation keys and nothing else: the `[ui]`, `[theme]`, `[files]`,
`[wikilinks]`, `[search]`, and `[session]` tables, plus `general.wrap` and
`general.tab_width`.

It may never set `editor.external_command` or any other key that names a
program to run, and `[keys]` remaps from a local file are ignored. Anything
outside the allow list produces a startup warning naming the file, and is
dropped.

Set `general.allow_local_config = false` in your own configuration to ignore
these files entirely.

## Nothing here is ever fatal

An **unknown key** produces a warning and is ignored, so a configuration
written for a newer perga still opens your vault.

An **invalid value** produces a warning naming the key and falls back to the
default *for that key alone* — a mistyped `sidebar_width` costs you the sidebar
width, not the whole `[ui]` table.

A **file that is not valid TOML** is skipped with a warning.

`perga --check-config` prints every warning and exits. `perga
--generate-config` prints the block below, which is a good starting point:

```sh
mkdir -p ~/.config/perga
perga --generate-config > ~/.config/perga/config.toml
```

## Reference

Every key, with its default. This is the same text `--generate-config` prints.

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

## Notes on particular keys

**`files.include_hidden`** is on, which is a deliberate divergence from other
Markdown browsers. A dotted directory in a notes vault holds notes, and hiding
`.github` or `.claude` with no visible override is the behaviour this project
exists to avoid. `.git`, `.hg`, `.svn`, and `.jj` are always excluded and that
is not configurable — a `.git` directory holds tens of thousands of object
files and nothing a reader wants.

**`files.respect_gitignore`** is a property of the walk, so changing it needs a
restart. The other two file toggles are applied when the tree is drawn, which
is why `.` and `a` are instant on a large vault.

**`theme.name = "auto"`** reads `COLORFGBG` and picks `dark` or `light`. Many
terminals do not set it; those get `dark`. If yours is light and quiet about
it, write `theme.name = "light"` once.

**`editor.external_command`** is split on whitespace, so `"code --wait"` works.
The file path is always passed as a separate argument and never goes through a
shell.

**`wikilinks.cache`** stores the backlink index under
`$XDG_CACHE_HOME/perga/`, validated per file against what the tree walk already
reports. Deleting the cache costs one slower start.

**`session.restore`** applies only when perga is opened with no path argument;
a named file or directory is what you asked for.

## Remapping keys

See [keybindings.md](keybindings.md) for the action names and the key syntax.

```toml
[keys]
"toggle_sidebar" = "ctrl+space"
"quit" = ["q", "ctrl+q"]
"scroll_top" = "g g"
```

## Environment variables

| Variable | Effect |
|---|---|
| `NO_COLOR` | Set and non-empty: drop every colour, keep bold and underline |
| `COLORTERM` | `truecolor` or `24bit`: use 24-bit colour, otherwise degrade to ANSI-256 |
| `COLORFGBG` | Read by `theme.name = "auto"` to tell a light terminal from a dark one |
| `EDITOR`, `VISUAL` | What `o` runs when `editor.external_command` is empty |
| `PERGA_LOG` | `debug` enables logging to the file named by `--log` |
| `XDG_CONFIG_HOME` | Where `config.toml` and `themes/` are looked for |
| `XDG_CACHE_HOME` | Where the backlink index is cached |
| `XDG_STATE_HOME` | Where sessions and recovery files are kept |
