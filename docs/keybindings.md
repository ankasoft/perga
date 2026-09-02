# Keybindings

Every binding below comes from one table in `src/config/keymap.rs`. The help
overlay (`?`) is generated from it, this page is checked against it by a test,
and a remap in `[keys]` moves all three at once.

## Contexts

The same key means different things depending on what has focus. Lookup tries
the focused context first, then **Global**.

| Context | When it applies |
|---|---|
| Global | Wherever it is not shadowed by a more specific context |
| Reading | The document viewport has focus and the tab is in read mode |
| Sidebar (focused) | The sidebar has focus |
| Editing | The tab is in edit mode |

In edit mode the text area owns every key the **Editing** context does not
claim, so `j` types a `j` rather than scrolling. In an overlay — help, the
quick switcher, find, a prompt, a confirmation, hint mode — the overlay owns
every key except `Esc`, which closes it, and `Ctrl+C`, which always quits.

## Global

| Key | Action | `[keys]` name |
|---|---|---|
| `q`, `Ctrl+Q` | Quit | `quit` |
| `?` | Help overlay | `toggle_help` |
| `Ctrl+B`, `Ctrl+E` | Toggle the sidebar (`Ctrl+E` for tmux users) | `toggle_sidebar` |
| `Tab` | Focus the next pane | `focus_next` |
| `Shift+Tab` | Focus the previous pane | `focus_prev` |
| `m 1`, `Alt+1` | Sidebar mode: files | `sidebar_mode_files` |
| `m 2`, `Alt+2` | Sidebar mode: search | `sidebar_mode_search` |
| `m 3`, `Alt+3` | Sidebar mode: outline | `sidebar_mode_outline` |
| `m 4`, `Alt+4` | Sidebar mode: links | `sidebar_mode_links` |
| `m m` | Toggle mouse capture | `toggle_mouse` |
| `Ctrl+N` | New file | `new_file` |
| `Ctrl+O` | Quick switcher | `open_quick_switcher` |
| `Ctrl+F` | Find in document | `open_find_in_document` |
| `Ctrl+Shift+F`, `Ctrl+G` | Search the project | `open_project_search` |
| `Ctrl+T` | New tab | `new_tab` |
| `Ctrl+W` | Close tab | `close_tab` |
| `g t`, `Ctrl+PageDown` | Next tab | `next_tab` |
| `g T`, `Ctrl+PageUp` | Previous tab | `prev_tab` |
| `Esc` | Close the overlay, or leave edit mode | `escape` |
| `Ctrl+Z` | Suspend perga (resume with `fg`) | `suspend` |

`Ctrl+C` quits and is deliberately not remappable: it must always be able to
end the program.

## Reading

| Key | Action | `[keys]` name |
|---|---|---|
| `j`, `Down` | Scroll down one line | `scroll_line_down` |
| `k`, `Up` | Scroll up one line | `scroll_line_up` |
| `Ctrl+D` | Scroll down half a page | `scroll_half_page_down` |
| `Ctrl+U` | Scroll up half a page | `scroll_half_page_up` |
| `Space`, `PageDown` | Scroll down a page | `scroll_page_down` |
| `b`, `PageUp` | Scroll up a page | `scroll_page_up` |
| `g g` | Top of the document | `scroll_top` |
| `G` | Bottom of the document | `scroll_bottom` |
| `h` | Scroll left | `scroll_left` |
| `l` | Scroll right | `scroll_right` |
| `{` | Previous heading | `prev_heading` |
| `}` | Next heading | `next_heading` |
| `n` | Focus the next link | `next_link` |
| `N` | Focus the previous link | `prev_link` |
| `Enter` | Follow the focused link | `follow_link` |
| `Ctrl+Enter`, `t Enter` | Follow the focused link in a new tab | `follow_link_in_new_tab` |
| `f` | Label and jump to a link | `hint_mode` |
| `H`, `Alt+Left` | Back | `history_back` |
| `L`, `Alt+Right` | Forward | `history_forward` |
| `R` | Rename the active document | `rename_document` |
| `e`, `i` | Enter edit mode | `enter_edit_mode` |
| `o` | Open in `$EDITOR` | `open_in_external_editor` |
| `y` | Copy the document path | `copy_document_path` |
| `r` | Reload the active document | `reload_document` |

`h` and `l` scroll a clipped code block or a wide table sideways. They move the
tree only when the sidebar has focus.

## Sidebar (focused)

| Key | Action | `[keys]` name |
|---|---|---|
| `j`, `Down` | Move the selection down | `sidebar_down` |
| `k`, `Up` | Move the selection up | `sidebar_up` |
| `l`, `Right`, `Enter` | Open the selection | `sidebar_activate` |
| `h`, `Left` | Collapse a directory, or go to its parent | `sidebar_back` |
| `1` | Sidebar mode: files | `sidebar_mode_files` |
| `2` | Sidebar mode: search | `sidebar_mode_search` |
| `3` | Sidebar mode: outline | `sidebar_mode_outline` |
| `4` | Sidebar mode: links | `sidebar_mode_links` |
| `.` | Toggle hidden files | `tree_toggle_hidden` |
| `a` | Toggle non-Markdown files | `tree_toggle_all_files` |
| `/` | Filter the tree, or edit the last search query | `tree_filter` |
| `r` | Rename the selected entry | `tree_rename` |
| `>` | Widen the sidebar | `sidebar_widen` |
| `<` | Narrow the sidebar | `sidebar_narrow` |

The four movement keys mean whatever the showing mode makes of them: a tree row
in files mode, a heading in outline mode, a search hit in search mode.

## Editing

Deliberately sparse. Everything not listed goes to the text area, which brings
its own undo, selection, and word motions.

| Key | Action | `[keys]` name |
|---|---|---|
| `Ctrl+S` | Save | `save` |
| `Esc` | Leave edit mode | `escape` |
| `Ctrl+Z` | Undo | `undo` |
| `Ctrl+Y` | Redo | `redo` |

## Overlays

These are fixed and not remappable; an overlay is a mode, and a mode with
user-defined exits is a mode people get stuck in.

| Overlay | Keys |
|---|---|
| Help | `j`/`k` scroll, `g`/`Home` to the top, `PageUp`/`PageDown`, `Esc`/`q`/`?` close |
| Quick switcher | typing filters, `Up`/`Down`/`Tab`/`Ctrl+P`/`Ctrl+N` move, `Enter` opens, `Ctrl+Enter` opens in a new tab, `Esc` cancels |
| Find in document | typing searches, `Enter`/`Down`/`Ctrl+N` next, `Shift+Enter`/`Up`/`Ctrl+P` previous, `Esc` closes |
| Prompt | `Ctrl+U` clears, `Ctrl+W` deletes a word, `Enter` accepts, `Esc` cancels |
| Confirmation | the keys the dialog names, and `Esc` to do nothing |
| Hint mode | type a label to follow it, `Backspace` corrects, `Esc` cancels |
| Disambiguation | `j`/`k` move, `1`–`9` pick directly, `Enter` opens, `Esc` cancels |

## Terminals that cannot deliver a key

Several bindings depend on the kitty keyboard protocol, which not every
emulator speaks. Each has an always-available alternative, and the help overlay
lists both.

| Wanted | Fallback | Why |
|---|---|---|
| `Ctrl+Enter` | `t Enter` | Legacy terminals cannot distinguish `Ctrl+Enter` from `Enter` |
| `Ctrl+Shift+F` | `Ctrl+G` | `Ctrl+Shift+<letter>` is not reportable without the protocol |
| `Alt+Left` / `Alt+Right` | `H` / `L` | tmux and several emulators intercept `Alt+arrow` |
| `Alt+1`–`Alt+4` | `m 1`–`m 4`, or `1`–`4` in the sidebar | Konsole and GNOME Terminal reserve `Alt+digit` for their own tabs |
| `Ctrl+B` | `Ctrl+E` | `Ctrl+B` is tmux's default prefix |

## Remapping

Any action can be rebound in `[keys]` by the name in the third column:

```toml
[keys]
"toggle_sidebar" = "ctrl+space"
"quit" = ["q", "ctrl+q"]     # a list binds several keys
"scroll_top" = "g g"         # a sequence is space-separated tokens
```

A remap **replaces** an action's defaults rather than adding to them, so a key
can be freed. Modifiers are `ctrl+`, `alt+`, `shift+`, lowercase and in that
order. Named keys are `enter`, `esc`, `tab`, `backtab`, `space`, `backspace`,
`delete`, `insert`, `home`, `end`, `pageup`, `pagedown`, `up`, `down`, `left`,
`right`, and `f1`–`f12`. Anything else is the character itself.

Never write `"gt"` for a sequence — it is ambiguous with a named key and is
rejected with a warning. Write `"g t"`.

When two actions end up on one key the last definition wins and a warning names
the one that was shadowed. `perga --check-config` prints every such warning.
