# Contributing to perga

Thanks for your interest. This document covers what you need to know before
opening a pull request.

## Getting set up

```sh
git clone https://github.com/ankasoft/perga
cd perga
git config core.hooksPath .githooks
cargo build
cargo test
```

The `core.hooksPath` line installs the commit-message hook described below. Please
run it once after cloning.

## Before you push

Every commit on `main` must pass all three of these:

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-features
```

CI additionally runs `cargo deny check` and `cargo publish --dry-run`.

## Commit messages

[Conventional Commits](https://www.conventionalcommits.org/en/v1.0.0/):
`feat:`, `fix:`, `docs:`, `refactor:`, `test:`, `perf:`, `build:`, `ci:`, `chore:`,
with an optional scope such as `feat(vault):`.

- Subject in the imperative mood, 72 characters or fewer, no trailing period.
- Body explains *why* when the change is not self-evident. Wrap at 72.
- One logical change per commit.

The `.githooks/commit-msg` hook rejects messages containing AI-assistant
attribution trailers. The same check runs in CI over the commits in a push range.

## Scope

perga has an explicit list of non-goals for 1.0: inline image protocols, a graph
view, a plugin system, remote sources, macOS and Windows support, bookmarks, a
dedicated tag pane, split panes, multiple simultaneous vaults, and telemetry of any
kind. Please open an issue to discuss before implementing anything in that list.

## Language

Everything in this repository is written in English: code, identifiers, comments,
commit messages, documentation, error messages, and CLI help text.

## Licence

By contributing you agree that your contribution is dual licensed under
`MIT OR Apache-2.0`, as stated in the README. There is no CLA and no DCO sign-off
requirement.
