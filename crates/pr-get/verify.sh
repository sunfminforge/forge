#!/bin/sh
# Checks the refinement types of pr-get, the one crate here, which enables
# Flux (its Cargo.toml has `[package.metadata.flux]`).
# Needs Homebrew's `sunfmin/tap/flux-toolchain`, the newest the tap has: its
# `cargo-flux` brings the `fixpoint`, `z3` and Flux sysroot it was built with,
# and nothing here pins, builds or installs any of them.
#
#   ./verify.sh                 check the crate whole
#   ./verify.sh <name>          check the functions whose path contains a name
#   ./verify.sh <cargo flux arguments>
#   ./verify.sh --list          print the crate's name, and exit
#
# Before the crate it prints `verify: checking .` on stdout.
set -e
cd "$(dirname "$0")"
if [ "${1:-}" = "--list" ]; then
    for manifest in Cargo.toml; do
        if grep -q '^\[package\.metadata\.flux\]' "$manifest"; then
            sed -n 's/^name = "\(.*\)"$/\1/p' "$manifest"
        fi
    done
    exit 0
fi
formula=sunfmin/tap/flux-toolchain
# brew where the PATH has it, or in either place it installs to.
brew=$(command -v brew 2>/dev/null) || brew=
[ -n "$brew" ] || for b in "$HOME/homebrew/bin/brew" /opt/homebrew/bin/brew; do
    [ -x "$b" ] && brew=$b && break
done
if [ -z "$brew" ]; then
    echo "verify: Flux comes from Homebrew's $formula, and no brew is on the PATH, in ~/homebrew or in /opt/homebrew: install Homebrew, then: brew install $formula" >&2
    exit 1
fi
# The tap is read fresh before it is compared with: the newest is the newest
# the tap has now. A host that cannot reach it compares with what it last read.
"$brew" update --quiet >/dev/null 2>&1 ||
    echo "verify: brew update failed: $formula is compared with the tap as this host last read it" >&2
if ! "$brew" list --versions "$formula" 2>/dev/null | grep -q .; then
    echo "verify: $formula is not installed. Install it: brew install $formula" >&2
    exit 1
fi
if "$brew" outdated --quiet "$formula" 2>/dev/null | grep -q .; then
    echo "verify: $formula $("$brew" list --versions "$formula" | cut -d' ' -f2-) is not the newest the tap has. Upgrade it: brew upgrade $formula" >&2
    exit 1
fi
# The tap's cargo-flux comes first, so no cargo-flux, fixpoint or z3 an older
# install left in ~/.cargo/bin is the one run: it puts its own fixpoint and z3
# first. cargo-flux re-invokes `cargo +<its nightly>`, which only rustup's
# cargo understands; rustup starts from its default toolchain.
PATH="$("$brew" --prefix "$formula")/bin:$HOME/.cargo/bin:$PATH"
export PATH
# A solver query of one function past twenty seconds is an error naming it,
# Flux checks on past an error so one run names every slow function, and it
# times each, the file of timings kept out of the tree.
FLUXFLAGS="-Ftimings -Ffixpoint-timeout=20 -Fcatch-bugs -Flog-dir=$PWD/../../target/flux-log${FLUXFLAGS:+ $FLUXFLAGS}"
export FLUXFLAGS
case "${1:-}" in
"" | -* | check) ;;
*) set -- check --only-check="def:$1" ;;
esac
crates=$(LC_ALL=C grep -l '^\[package\.metadata\.flux\]' Cargo.toml)
if [ -z "$crates" ]; then
    echo "verify: no crate enables Flux: a pure crate's Cargo.toml has [package.metadata.flux] with enabled = true" >&2
    exit 1
fi
for manifest in $crates; do
    echo "verify: checking $(dirname "$manifest")"
    cargo flux "$@" -p pr-get --manifest-path "$manifest"
done
