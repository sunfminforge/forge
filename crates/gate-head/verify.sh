#!/bin/sh
# Checks the refinement types of gate-head's pure module, `decide`,
# with Flux, and fails on any error.
#
# Needs Homebrew's sunfmin/tap/flux-toolchain: its cargo-flux brings the
# fixpoint, z3 and Flux sysroot it was built with; nothing here installs one.
# `run`, `git` and `guild` are the effect modules and are not checked: forge
# takes this tool as one crate, so its effect part shares the crate (README.md,
# "Departures").
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
for module in decide; do
    echo "verify: checking gate_head::$module"
    cargo flux check -p gate-head --manifest-path Cargo.toml --only-check="def:$module::"
done
