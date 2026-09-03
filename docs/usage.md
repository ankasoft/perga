# Usage

## Opening a vault

```sh
perga                # the current directory
perga docs/          # a directory, as the vault root
perga README.md      # a file, with its parent directory as the vault root
```

The **vault** is the directory everything is relative to: the tree, the search,
the backlink index, and the paths in the title bar. Opening a file still gives
you a vault — its parent directory — so links out of that file resolve and the
tree has something to show.

Opening with no argument restores the previous session for that directory:
the tabs that were open, where you were in them, and the sidebar's state. A
named path is what you asked for, so it opens that instead.

## The screen

```
┌─ perga ─────────────────────────────────── docs/api/auth.md ─── 2/7 ─┐
│ ● auth.md │ setup.md │ +                                            │  tabs
├──────────────────────────┬──────────────────────────────────────────┤
│ FILES  search  outline   │                                          │
│ links                    │  # Authentication                        │
│ ─────────────────────────│                                          │
│ ▾ docs/                  │  This service uses Bearer tokens. See    │
│   ▾ api/                 │  setup for the full walkthrough.         │
│       auth.md         ●  │                                          │
│   ▸ guides/              │                                          │
│   README.md              │                                          │
├──────────────────────────┴──────────────────────────────────────────┤
│ READ  H/L history  f links  ^b sidebar  ^o switch  ^f find  ? help  │
└─────────────────────────────────────────────────────────────────────┘
```

The tab bar appears with the second tab. Below 80 columns the sidebar stops
splitting the screen and draws over the viewport instead; below 20 rows the tab
bar is hidden and the status bar collapses. The minimum size is 40x10.

`Tab` moves focus between the sidebar and the viewport. The focused pane's
border is drawn in the theme's `border_focused` colour — that is the only cue,
so a theme that makes the two indistinguishable is a broken theme.

## The four sidebar modes

Switch with `m 1` to `m 4` anywhere, or `1` to `4` when the sidebar has focus.
`Alt+1` to `Alt+4` also work where the terminal delivers them.

**Files** is the vault tree. Directories are collapsed except the path to the
document you are reading, which is expanded and marked with `●`. `.` shows and
hides dotted entries — they are shown by default, because a dotted directory in
a notes vault holds notes. `a` shows files that are not Markdown; they are
dimmed, and opening one hands it to the desktop opener. `/` filters by name as
you type.

**Search** holds the last project-wide search, grouped by file with the match
picked out. `Enter` opens a hit at its line. `/` reopens the query for editing.

**Outline** is the active document's headings, indented by level. The heading
you are currently inside is highlighted and follows the scroll position;
`Enter` jumps to the selected one.

**Links** is what the document points at and what points at it. Outgoing links
are marked `✗` when they resolve to nothing. Backlinks come from the index,
which reports its progress here while it builds.

## Reading

`j`/`k` and the arrows scroll a line, `Ctrl+D`/`Ctrl+U` half a page,
`Space`/`b` a page, `g g` and `G` the ends, `{` and `}` the headings.

Code blocks and wide tables are never soft-wrapped — wrapping code destroys
it — so `h` and `l` scroll them sideways and a `…` marks a clipped line.

Scroll position is per tab and survives switching tabs, following a link and
coming back, and reloading a file that changed on disk.

## Following links

`n` and `N` step through the links in reading order, scrolling one into view
when it is off screen. `Enter` follows the focused one. On a dense page `f` is
faster: it labels every visible link with a home-row key, and typing the label
follows it.

What happens next depends on the target:

| Target | What perga does |
|---|---|
| A Markdown file | Opens it, and scrolls to the `#heading` if the link named one |
| `#heading` alone | Scrolls, without reloading the document |
| A directory | Reveals it in the tree |
| `http(s)://`, `mailto:` | Hands it to the desktop opener |
| Any other file | Hands it to the desktop opener |
| Nothing at all | Says `Cannot resolve: …` and creates nothing |

Relative targets resolve against the document's own directory, not the vault
root. A leading `/` is read from the vault root first and the filesystem root
second.

`H` and `L` — or `Alt+Left` and `Alt+Right` — go back and forward, restoring
the exact scroll offset you left. History is per tab, capped at 100 entries,
and going somewhere new after going back discards the forward stack.

## Wiki-links

`[[Page Name]]`, `[[Page Name|Display Text]]`, `[[Page Name#Heading]]`, and
`[[folder/Page Name]]` all work, and resolve through the backlink index:

1. The name read as a path, from the vault root and then from the document's
   own directory.
2. An exact filename match anywhere in the vault.
3. A case-insensitive filename match.

Several matches open a picker rather than a guess. A name nothing answers to
offers to create the file — the one place perga offers to create anything from
a link. A broken *inline* link never does.

The index builds in the background on startup and is cached between runs, so a
second start on the same vault reparses only what changed. Until it is ready,
following a wiki-link says `Still indexing…` rather than guessing.

## Searching

`Ctrl+F` finds within the document: incremental, case-insensitive unless the
query contains a capital, with every match highlighted and the count in the
bar. `Enter` and `Shift+Enter` cycle. Find state is per tab.

`Ctrl+G` (or `Ctrl+Shift+F`) searches the whole vault. Results stream in as
they are found; a new query cancels the one running. The query is literal
unless `search.regex` is on or it is written between slashes — `/tok[e]n/`. An
invalid pattern is reported in the sidebar, never a crash.

`m t` switches theme without restarting, cycling the three built-ins and then
your own. The choice is remembered for the next run on that vault.

`Ctrl+O` is the quick switcher: fuzzy matching over every Markdown file in the
vault, with the matched characters highlighted. With nothing typed it lists the
files you have opened this session, most recent first. `Ctrl+Enter` opens in a
background tab.

## Tabs

`Ctrl+T` opens one, `Ctrl+W` closes it, `g t` and `g T` switch. Twenty is the
maximum; past that the active tab is reused and perga says so. Closing the last
tab quits, asking first if anything is unsaved.

Each tab owns its own document, scroll position, history, link focus, and find
state. Nothing crosses between them.

## Editing

`e` or `i` enters edit mode on the active document. The cursor lands on the
source line you were reading, and leaving puts the rendered view back where the
cursor was — you do not lose your place in either direction.

`Ctrl+S` saves. `Esc` leaves, asking first if there are unsaved edits. The tab
label and the status bar both show `●` while there are.

Saving writes through a temporary file in the same directory and renames it
into place, so an interrupted save leaves the old file rather than half of the
new one. File permissions are preserved, and CRLF endings and a byte-order mark
survive the round trip. If the file changed on disk since it was opened, perga
asks before overwriting; it never does so silently.

`o` hands the file to `$EDITOR` (or `editor.external_command`), leaving the
terminal entirely so a full-screen editor works, and reloads when it exits.

## Creating and renaming

`Ctrl+N` prompts for a path relative to the vault root, pre-filled with the
directory you are in. `.md` is appended when there is no extension, missing
directories are created, and the new file opens in a new tab in edit mode.

`R` renames the active document; `r` renames the selected tree entry. Both take
a *name*, not a path — moving a note between directories is a different
operation. Every tab pointing at the old path follows the rename, and the tree
and the index are updated.

Links in *other* documents are deliberately not rewritten. Rewriting links
across a vault is easy to get wrong and impossible to undo, so perga tells you
how many documents now point at the old name and leaves finding them to the
search mode.

**There is no delete.** Every other operation here can be undone; that one
cannot.

## Live reload

perga watches the vault. A document that changes on disk while you are reading
it reloads in place, keeping your position. A document that changes while you
have unsaved edits does not: the status bar says
`File changed on disk (r to reload)` and the buffer is left alone. `r` reloads
on demand at any time.

On a vault large enough to exceed the platform's watch limit, watching is given
up with a warning and `r` becomes the way to refresh.

## Print mode

```sh
perga --print README.md          # ANSI-styled, to stdout
perga README.md | less -R        # the same, because stdout is not a terminal
perga --print --wrap 72 note.md  # at a fixed width
```

No alternate screen, no input, no mouse. The width is the terminal's when
stdout is a TTY, otherwise `--wrap` if given, otherwise 80 columns. `NO_COLOR`
is honoured. A directory in print mode is an error.

## Other flags

| Flag | What it does |
|---|---|
| `-c`, `--config <FILE>` | Use this config file |
| `--no-config` | Ignore every config file |
| `-t`, `--theme <NAME>` | Override the configured theme |
| `--sidebar <MODE>` | Start in this sidebar mode |
| `--no-sidebar` | Start with the sidebar hidden |
| `-a`, `--all` | Show non-Markdown files in the tree |
| `--no-gitignore` | Do not respect `.gitignore` |
| `-w`, `--wrap <COLUMNS>` | Hard-wrap at this width |
| `--no-session` | Neither restore nor save the session |
| `--no-mouse` | Start with mouse capture off |
| `--check-config` | Validate the configuration and exit |
| `--generate-config` | Print the default configuration |
| `--generate-man` | Print the man page |
| `--generate-completions <SHELL>` | Print completions for bash, zsh, or fish |
| `--log <FILE>` | Write debug logs there |

`PERGA_LOG=debug` also enables logging. Logs never go to stdout or stderr while
the TUI is running.
