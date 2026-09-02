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

The scripted session: open the vault, walk the tree, open a document, follow an
inline link, go back, follow a wiki-link, look at its backlinks, run a
project-wide search, show the outline, and hide the sidebar.

Regenerate it whenever the interface changes visibly, and always before a
release — see [`../docs/publishing.md`](../docs/publishing.md).

## `social-preview.png`

The 1280x640 image that link previews use on aggregators and chat clients.
Without one, the repository shows a generic placeholder.

**It must be uploaded by hand.** GitHub does not read it from the repository:
go to *Settings → General → Social preview* and upload the file there. Nothing
in CI can do this, and the file being committed here is not enough.

### Regenerating it

The image is a real perga frame — captured through the same renderer the
application uses, not a mockup — typeset with ImageMagick. Two steps:

```sh
# 1. Capture a 120x22 frame of perga reading the fixture vault.
cargo test --test preview -- --ignored

# 2. Typeset it, and overlay the wordmark.
cd demo
magick -background '#1e1e2e' -fill '#cdd6f4' \
  -font JetBrainsMonoNerdFont-Regular -pointsize 18 \
  label:@social-preview-frame.txt -resize 1120x frame.png
magick -size 1280x640 xc:'#181825' \
  frame.png -geometry +80+118 -composite \
  -font JetBrainsMonoNerdFont-Bold -pointsize 54 -fill '#89b4fa' \
  -gravity northwest -annotate +80+34 'perga' \
  -font JetBrainsMonoNerdFont-Regular -pointsize 20 -fill '#6c7086' \
  -gravity northwest -annotate +262+62 'a terminal Markdown browser' \
  social-preview.png
rm frame.png
```

Any monospace font works; the geometry above is tuned for JetBrains Mono at
120 columns. `social-preview-frame.txt` is committed so the second step can be
re-run without a Rust toolchain.

The text is typeset in one colour rather than in the theme's, because the
capture is plain text — the layout, the tree, and the document are genuinely
perga's output, and the palette is the `dark` theme's background and body
colours. A photographic screenshot of a real terminal is a fine substitute if
you have one; keep the size at exactly 1280x640.
