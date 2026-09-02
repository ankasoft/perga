# A scripted session through perga's fixture vault.
#
# Regenerate with:
#
#     cd demo && vhs demo.tape
#
# vhs is https://github.com/charmbracelet/vhs. It needs `ttyd` and `ffmpeg`.
# Everything below is deterministic: the vault is the committed fixture corpus,
# the theme is pinned, and no step depends on wall-clock timing beyond the
# Sleeps, which exist so a viewer can follow what is happening.

Output demo.gif

Set Shell bash
Set FontSize 15
Set Width 1200
Set Height 700
Set Padding 12
Set Theme "Catppuccin Mocha"
Set TypingSpeed 60ms

# A pinned theme and no session, so the recording is the same every time.
Hide
Type "cd ../tests/fixtures/vault && export NO_SESSION=1"
Enter
Type "clear"
Enter
Show

Type "perga --theme dark --no-session ."
Enter
Sleep 2s

# The tree: walk down to a document and open it.
Sleep 500ms
Type "jj"
Sleep 400ms
Type "l"
Sleep 600ms
Type "l"
Sleep 400ms
Type "l"
Sleep 400ms
Type "jl"
Sleep 2s

# Follow an inline link, then come back.
Escape
Sleep 300ms
Type "n"
Sleep 600ms
Enter
Sleep 2s
Type "H"
Sleep 1500ms

# A wiki-link, and the backlinks it produces.
Type "nn"
Sleep 600ms
Enter
Sleep 2s
Type "m4"
Sleep 2500ms

# A project-wide search.
Ctrl+g
Sleep 500ms
Type "token"
Sleep 400ms
Enter
Sleep 2500ms

# The outline, and the sidebar out of the way.
Type "m3"
Sleep 2s
Ctrl+b
Sleep 2s

Type "q"
Sleep 1s
