# Theming

A theme is a TOML file of style tables. Every key is optional and every key
perga needs has a value in the built-in `dark` theme, so a theme file only has
to say what it changes.

## Choosing one

```toml
[theme]
# auto | dark | light | high-contrast | <a filename in theme.dir>
name = "auto"
# Where user themes live. Empty means $XDG_CONFIG_HOME/perga/themes.
dir = ""
# The syntect theme for fenced code blocks.
code_theme = "base16-ocean.dark"
```

`--theme <name>` overrides it for one run.

`auto` reads the `COLORFGBG` environment variable — set by rxvt, Konsole, and
several others — and picks `dark` or `light`. Terminals that do not set it get
`dark`.

## The three built-in themes

**`dark`** is the default and the base every other theme inherits from.

**`light`** is a genuinely light palette rather than an inversion. Body text is
`#1c1e26` on `#fdfdfb`, a contrast ratio of 15.8:1.

Both `dark` and `light` clear WCAG AA everywhere: **4.5:1 for anything that
carries text, 3:1 for a border or a rule**, measured against the surface it is
actually drawn on — including the selection background, because a selected row
keeps its own foreground, and including the ANSI-256 palette a terminal without
truecolour receives. Tests in `src/theme/mod.rs` enforce this and name the key
that fails, so a theme change cannot quietly make the interface unreadable.

If you write your own theme, the same test will not check it. `contrast_ratio`
is public if you want to.

**`high-contrast`** uses the sixteen ANSI colours and nothing else, so it
renders identically on a terminal with no 256-colour or truecolour support and
follows whatever palette you have configured there. Nothing in it relies on
colour alone: every distinction also carries bold, dim, underline, or reverse
video.

## Colour degradation

When `COLORTERM` does not claim `truecolor` or `24bit`, every `#rrggbb` value
is mapped to its nearest ANSI-256 colour, searched over the whole palette — the
6×6×6 cube *and* the 24-step grey ramp. Sending 24-bit escapes to a terminal
that cannot read them would print them as text.

Plenty of terminals support truecolour without advertising it in `COLORTERM`,
so this path is common. The contrast guarantee below covers it: both built-in
themes are measured as written *and* as degraded.

`NO_COLOR`, set and non-empty, drops every colour and keeps every modifier, so
bold headings stay bold. It is applied last and wins over everything.

## Writing one

Put `mine.toml` in `$XDG_CONFIG_HOME/perga/themes/` and set
`theme.name = "mine"`. perga watches that directory: save the file and the
running application picks it up, which makes authoring a theme a matter of
`:w` and looking at the screen.

Start from the smallest thing that changes what you want:

```toml
# ~/.config/perga/themes/mine.toml
name = "mine"

[markdown]
h1 = { fg = "#ff5f5f", bold = true }
h2 = { fg = "#ffaf5f", bold = true }
link = { fg = "#5fafff", underline = true }

[ui]
border_focused = { fg = "#5fafff" }
```

Everything not mentioned comes from `dark`.

### Colours

Three forms, all accepted anywhere a colour is:

| Form | Example | Notes |
|---|---|---|
| Hex | `"#89b4fa"` | Degraded to ANSI-256 on a terminal without truecolour |
| ANSI index | `12` or `"12"` | 0–255 |
| ANSI name | `"bright_blue"` | The sixteen names, with an optional `bright_` prefix |

The names are `black`, `red`, `green`, `yellow`, `blue`, `magenta`, `cyan`,
`white`, and each of those with `bright_`. Prefer names in a theme meant to
work everywhere: they follow the palette the user chose in their terminal.

### Modifiers

Every style table accepts `bold`, `italic`, `underline`, `dim`, `reversed`, and
`crossed_out`, each `true` or `false`. `false` is meaningful: it turns off a
modifier the base theme set.

```toml
[markdown]
h1 = { fg = "green", bold = false }   # green, and not bold, unlike dark's h1
```

### Every key

**`[ui]`** — the chrome.

| Key | Where it is used |
|---|---|
| `background` | Painted behind the whole frame |
| `border` | An unfocused pane's border |
| `border_focused` | The focused pane's border — the only cue for focus |
| `title` | The application name in the title bar, and key names in hints |
| `status_bar` | The status bar's ground |
| `status_mode` | The `READ`/`EDIT` badge, and a confirmation's keys |
| `status_warning` | A warning message, and a confirmation's border |
| `status_error` | An error message |
| `selection` | The selected row in any list, and a drawn caret |
| `scrollbar` | The viewport's scrollbar |
| `logo` | The welcome screen's wordmark |
| `logo_subtitle` | The line under it, the scroll position, and overlay hints |

**`[tabs]`** — `active`, `inactive`, `dirty`.

**`[sidebar]`** — `directory`, `file`, `file_active`, `file_other` (a file
perga does not render, and every "nothing here" line), `mode_active`,
`mode_inactive`, `match` (a filter or search hit), `line_number`.

**`[markdown]`** — `h1`, `h2`, `h3`, `h4`, `h5`, `h6`, `text`, `emphasis`, `strong`,
`strikethrough`, `blockquote`, `blockquote_bar`, `code_inline`,
`code_block_bg`, `link`, `link_focused`, `link_broken`, `link_external`,
`wikilink`, `list_marker`, `task_done`, `task_todo`, `table_border`,
`table_header`, `rule`, `footnote`, `image_placeholder`, `html`,
`frontmatter`.

**`[hints]`** — `label`, the key drawn over a link in hint mode.

### Code blocks

Fenced code is highlighted by `syntect`, which has its own themes. Name one
with the top-level `code_theme` key in a theme file, or `theme.code_theme` in
your configuration. The bundled set is `base16-ocean.dark`,
`base16-eighties.dark`, `base16-mocha.dark`, `base16-ocean.light`,
`InspiredGitHub`, and `Solarized (dark)` / `Solarized (light)`.

A theme meant for a light terminal should say so; `base16-ocean.dark` on a pale
background is unreadable.

## Checking one

`perga --check-config` reports a theme that cannot be found or cannot be
parsed. A theme that fails to load leaves `dark` in place and warns — perga
opens the vault either way.

Two things worth checking by hand, because no test can:

- Focused and unfocused borders must be **unambiguously** different. It is the
  only indication of where your keys are going.
- On an ANSI-16 terminal, run with `high-contrast` and confirm nothing depends
  on a colour that terminal does not have.
