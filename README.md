# perga

A terminal Markdown browser: a full-screen TUI that opens a directory of Markdown
files and lets you navigate, read, search, and edit them with the ergonomics of a
document browser rather than a pager.

A persistent hierarchical sidebar, a document viewport with tabs, browser-like
back/forward history per tab, wiki-links with backlinks, and in-place editing — in
one static binary with no runtime dependencies.

> **Status:** in development toward 0.1.0.

## Name origin

Parchment takes its name from Pergamon, the ancient city in western Anatolia where,
according to Pliny, animal-skin writing surfaces were developed after Egypt
restricted papyrus exports. The Latin *pergamena* and, through it, the English
*parchment* both descend from the city's name. `perga` is named after the root of
the written page.

## Platform

Linux only, x86_64 and aarch64.

## Quick start

```sh
perga            # open the current directory
perga docs/      # open a directory as the vault root
perga README.md  # open a file, with its parent directory as the vault root
```

Press `?` for the full keybinding reference.

### A note for tmux users

`Ctrl+B` toggles the sidebar, and it is also the default tmux prefix — tmux will
consume it before perga ever sees it. `Ctrl+E` is bound to the same action and is
always available. See [docs/keybindings.md](docs/keybindings.md).

## Documentation

- [Usage](docs/usage.md)
- [Configuration](docs/configuration.md)
- [Keybindings](docs/keybindings.md)
- [Theming](docs/theming.md)
- [Architecture](docs/architecture.md)
- [Licensing](docs/licensing.md)

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
