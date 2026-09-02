# Licensing

perga is dual licensed under **`MIT OR Apache-2.0`**, the de facto standard for the
Rust ecosystem. `rustc` itself, `ratatui`, `crossterm`, and `pulldown-cmark` all use
the same pair. Apache-2.0 contributes an explicit patent grant; MIT contributes
brevity and GPL compatibility.

The full texts are in [`LICENSE-MIT`](../LICENSE-MIT) and
[`LICENSE-APACHE`](../LICENSE-APACHE). The SPDX expression `MIT OR Apache-2.0`
appears in `Cargo.toml`.

## What this means for users

You may use perga under the terms of either licence, at your option. You do not
have to choose in advance, and you do not have to tell anyone which one you picked.

## What this means for packagers

- Install **both** licence files. Every packaging artefact in `packaging/` does
  this; if you write a new one, match it.
- The SPDX expression for distribution metadata is `MIT OR Apache-2.0`.
- There is no `NOTICE` file. Apache-2.0 requires preserving one if it exists; it
  does not require creating one, and perga does not have one.
- There are no per-file licence headers. The two licence files plus the SPDX
  expression are the complete statement.

## What this means for contributors

By contributing you agree that your contribution is dual licensed as above. This is
the standard Rust formulation, stated in the README's Contribution section.

There is **no CLA and no DCO sign-off requirement**.

## Dependency licences

perga's dependency tree is restricted to a permissive allow list enforced in CI by
[`cargo-deny`](https://github.com/EmbarkStudios/cargo-deny). The list lives in
[`deny.toml`](../deny.toml) at the repository root.

Regenerate a dependency licence report with:

```sh
cargo deny check licenses
```

To see every licence actually present in the tree:

```sh
cargo deny list
```

If a new dependency introduces a licence outside the allow list, the policy is to
replace the dependency rather than widen the list. The one documented exception is
`CC0-1.0`, carried by `notify`; the rationale is recorded in
[`decisions.md`](decisions.md).

`deny.toml` also enforces:

- **Advisories** — any RUSTSEC vulnerability fails the build. Unmaintained-crate
  advisories are reported for direct dependencies only.
- **Bans** — no TLS stack, no HTTP client, no async runtime, no `onig`. perga has
  no network feature and must not grow one by accident.
- **Sources** — crates.io only. No git dependencies.
