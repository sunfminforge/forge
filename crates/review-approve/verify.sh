#!/bin/sh
# Checks the refinement types of review-approve's pure modules, `plan` and
# `refusal`, with Flux, and fails on any error.
#
# Needs Homebrew's sunfmin/tap/flux-toolchain: its cargo-flux brings the
# fixpoint, z3 and Flux sysroot it was built with; nothing here installs one.
# `run` is the effect module and is not checked: forge takes this tool as one
# crate, so its effect part shares the crate, and Flux stops on clap's derive
# (README.md, "Departures").
set -e
cd "$(dirname "$0")"
formula=sunfmin/tap/flux-toolchain
brew=$(command -v brew 2>/dev/null) || brew=
[ -n "$brew" ] || for b in "$HOME/homebrew/bin/brew" /opt/homebrew/bin/brew; do
    [ -x "$b" ] && brew=$b && break
done
if [ -z "$brew" ] || ! "$brew" list --versions "$formula" >/dev/null 2>&1; then
    echo "verify: Flux comes from Homebrew's $formula, not installed here. Install it: brew install $formula" >&2
    exit 1
fi
PATH="$("$brew" --prefix "$formula")/bin:$HOME/.cargo/bin:$PATH"
export PATH
FLUXFLAGS="-Ftimings -Ffixpoint-timeout=20 -Fcatch-bugs -Flog-dir=$PWD/../../target/flux-log"
export FLUXFLAGS
for module in plan refusal; do
    echo "verify: checking review_approve::$module"
    cargo flux check -p review-approve --manifest-path Cargo.toml --only-check="def:$module::"
done
