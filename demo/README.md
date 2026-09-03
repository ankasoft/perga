# Demo assets

## `demo.gif`

The recording at the top of the README. It is generated from
[`demo.tape`](demo.tape) by [vhs](https://github.com/charmbracelet/vhs):

```sh
cd demo && vhs demo.tape
```

One command, no arguments, and the result is deterministic: the tape drives
perga through the committed fixture vault at `tests/fixtures/vault/`, pins the
theme with `--theme dark`, and disables session restore so a previous run
cannot change what the next one records.

vhs needs `ttyd` and `ffmpeg` on `PATH`. On Arch:
`pacman -S vhs ttyd ffmpeg`.

The scripted session: open the vault, walk the tree down to `docs/api/auth.md`
and open it, follow an inline link, go back, follow a wiki-link, look at the
backlinks the index found for it, run a project-wide search and open one of its
hits, show the outline, jump to a richer document through the quick switcher,
page through its table, task list, quote and code blocks, cycle the three
built-in themes, and hide the sidebar.

Two things about the tape are easy to get wrong. The sidebar does not have
focus at startup, so `Tab` has to come before any tree navigation and again
before the document keys; and nothing in the tree is selected until the first
`j`, so there is one more of them than there are rows to travel.

Regenerate it whenever the interface changes visibly, and always before a
release; see [`../docs/publishing.md`](../docs/publishing.md).
