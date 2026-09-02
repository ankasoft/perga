#!/usr/bin/env bash
#
# Generate the man page and the shell completions from the built binary.
#
# Run by `dist` while building a release, and by hand before `cargo deb` or
# `cargo generate-rpm`. Both of those read the paths this writes; see
# packaging/README.md.
#
# The binary generates these itself rather than a build script doing it:
# sharing the clap definition with a build script means duplicating or
# `#[path]`-including the CLI module, which is fragile for no benefit.

set -euo pipefail

out=target/assets
mkdir -p "$out/completions"

# Built for the host, because the man page and the completions do not vary by
# target and a cross-built binary cannot be run to produce them.
cargo build --release --quiet
perga=target/release/perga

"$perga" --generate-man > "$out/perga.1"

for shell in bash zsh fish; do
  "$perga" --generate-completions "$shell" > "$out/completions/perga.$shell"
done

echo "wrote $out/perga.1 and $out/completions/perga.{bash,zsh,fish}"
