# A scripted session through perga's fixture vault.
#
# Regenerate with:
#
#     cd demo && vhs demo.tape
#
# vhs is https://github.com/charmbracelet/vhs. It needs `ttyd` and `ffmpeg`.
# Everything below is deterministic: the vault is the committed fixture corpus,
# the theme is pinned and the session is off, so the recording is the same
# every time. The Sleeps exist so a viewer can follow what is happening.
#
# Note that the sidebar does not have focus at startup, so `Tab` comes before
# any tree navigation, and again to hand focus back to the document.

Output demo.gif

Set Shell bash
Set FontSize 15
Set Width 1200
Set Height 700
Set Padding 12
Set Theme "Catppuccin Mocha"
Set TypingSpeed 60ms

Hide
Type "cd ../tests/fixtures/vault"
Enter
Type "clear"
Enter
Show

Type "perga --theme dark --no-session ."
Enter
Sleep 2s

# The file tree: down to docs/api/auth.md and open it. Nothing is selected
# until the first `j`, so there is one more of them than there are rows to
# travel.
Tab
Sleep 900ms
Type "jj"
Sleep 500ms
Type "l"
Sleep 700ms
Type "j"
Sleep 400ms
Type "l"
Sleep 700ms
Type "jj"
Sleep 500ms
Type "l"
Sleep 2s

# Focus the document, follow an inline link, come back.
Tab
Sleep 500ms
Type "n"
Sleep 700ms
Enter
Sleep 2s
Type "H"
Sleep 1500ms

# A wiki-link, and the backlinks the index found for it.
Type "nn"
Sleep 800ms
Enter
Sleep 2s
Type "m4"
Sleep 2500ms

# A project-wide search, and one of its hits.
Ctrl+g
Sleep 500ms
Type "token"
Sleep 400ms
Enter
Sleep 2s
Tab
Sleep 400ms
Type "j"
Sleep 400ms
Enter
Sleep 2s

# The outline of what is open.
Type "m3"
Sleep 2s

# The quick switcher, on to a document with more in it.
Ctrl+o
Sleep 600ms
Type "gfm"
Sleep 600ms
Enter
Sleep 2500ms

# Tables, task lists, a quote bar, highlighted code. Paged rather than
# scrolled, so each of them is on screen long enough to be read.
Type "jjjj"
Sleep 2500ms
Type "jjjjj"
Sleep 2500ms
Type "jjjjjj"
Sleep 2500ms

# Themes, without restarting.
Type "mt"
Sleep 1800ms
Type "mt"
Sleep 1800ms
Type "mt"
Sleep 1500ms

# The whole width for the document.
Ctrl+b
Sleep 2500ms

Type "q"
Sleep 1s
