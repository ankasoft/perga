# Architecture

## The one constraint

```
crossterm event ──▶ event.rs ──▶ Action ──▶ App::update() ──▶ state mutation
                                                                    │
background thread ──▶ channel message ──▶ Action ────────────────────┤
                                                                    ▼
                                                          ui::render(&state, frame)
```

`Action` is an enum, and `App::update` is the only place application state
changes. Rendering is a pure function of state: every widget takes `&App`, and
nothing in `src/ui` mutates anything.

That is what makes the whole application testable without a terminal. Feed a
sequence of actions, assert on state, and render to `ratatui`'s `TestBackend`
for a snapshot, which is what every test in `tests/` does.

## The event loop

Terminal input is read on its own thread that calls `crossterm::event::read()`
in a loop and forwards each event down a channel. Every background worker,
the vault walker, the backlink indexer, the project searcher, the filesystem
watcher, sends into the same channel. The main loop does one blocking `recv()`
and nothing else.

No `poll(timeout)`, no `try_recv` spin, no timer. That is what makes 0% idle
CPU true by construction rather than by tuning. Debouncing filesystem events
happens on the watcher's thread, where it belongs.

## Modules

| Module | What it owns |
|---|---|
| `main.rs` | Process startup, the non-TUI outputs, the panic hook |
| `app.rs` | `App`, `Tab`, the event loop, every state transition |
| `action.rs` | The `Action` enum |
| `event.rs` | Which keymap context a press is resolved in |
| `config/` | The five-layer chain, the keymap table, sessions |
| `theme/` | Theme files, the built-in themes, colour degradation |
| `doc/` | Parsing, rendering, the offset↔line map, links, headings |
| `vault/` | The walk, the tree, the backlink index, the watcher |
| `search/` | Project search, fuzzy matching, find-in-document |
| `editor/` | The buffer, atomic saving, creating and renaming |
| `ui/` | Frame composition. Reads state; never writes it |

## The three hard parts

### Owned lines at the cache boundary

`tui_markdown::from_str` returns a `Text` borrowing its input. Holding the
source string, the parsed blocks, and the rendered lines in one struct fights
the borrow checker until the lines become `Vec<Line<'static>>` at the cache
boundary, which is what `doc::render` does, once, on the way in.

The cache is keyed by `(hash of the block's source, width, whether syntax
highlighting had loaded)`, never by byte range. Inserting one character shifts
the range of every block after it; hashing the text means a block that only
moved is still a hit. Editing one paragraph in a 10,000-line document
re-renders one block, and there is a test that asserts exactly that.

### The offset↔line map

Five features need to translate between a byte offset in the source and a line
on screen: anchor scrolling, find, outline synchronisation, link positioning,
and the edit-mode cursor round trip. It is a deliberate structure in
`doc::render` (a sorted list of `(rendered line, source offset)` origins, one
binary search serving both directions) and not a side effect of rendering.
When it emerges accidentally, all five features go subtly wrong in different
ways.

Block heights are only known after rendering at a given width, which makes
"which blocks are visible" circular. It is resolved by rendering in order and
keeping a running prefix sum, in chunks, so jumping to the end of a very large
document costs several frames rather than one freeze. Until the whole document
is measured the total is unknown, the scrollbar is indeterminate, and scrolling
is clamped to what *has* been measured.

### Background work

Every worker follows one pattern: it owns an `Arc<AtomicBool>` cancellation
flag, it reports through a sink the caller supplies, and the handle cancels on
drop. The sink turns each report into an `Action` and sends it down the one
channel. Nothing on a worker thread touches application state.

That is why a new search does not accumulate threads, why switching vaults does
not leave a walker reading a filesystem nobody is waiting on, and why the first
frame paints from an empty tree with the tree filling in behind it.

## Where the layers meet

- **The walk feeds the tree and the index.** The walk reports every file with
  its mtime and size; the tree draws them, and the index uses the same report
  to decide which files its cache does not already cover.
- **One slug function** serves anchors, the outline, `{`/`}`, and
  `[[Page#Heading]]`. Two would disagree.
- **One link extractor** serves the viewport, the links sidebar, and the
  backlink index, so a forward link and a backlink cannot disagree about where
  a link points.
- **One text input** serves the tree filter, the find bar, the prompts, and the
  quick switcher, so `Ctrl+W` means the same thing everywhere.

## Testing

`src/**/tests` covers the logic; `tests/*.rs` drives the application through
`Action` sequences and asserts on state or on a rendered frame. Snapshots use
`insta` at 120x40, 80x24, and 40x10, and are stored as plain text rather than
styled dumps so a diff is readable by a human.

`benches/` measures the targets in the specification with `criterion`. No
wall-clock number gates CI: shared runners have unpredictable performance, and
a flaky assertion gets "fixed" by loosening it until it proves nothing.

`docs/decisions.md` records every ambiguity resolved while building this, with
the reasoning. Read it before changing something that looks arbitrary.
