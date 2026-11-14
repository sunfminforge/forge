#!/bin/sh
# The gate of tools/pr-get: what must pass before it lands, run from
# anywhere as `sh tools/pr-get/gate.sh`.
#
#   1. cargo fmt --check
#   2. Flux, through ./verify.sh, on the crate: it must say
#      `0 trusted; 0 ignored` (./flux-summary.sh reads it and prints it)
#   3. cargo clippy -D warnings
#   4. cargo test, offline: curl is a stand-in, no forge is asked
set -eu
cd "$(dirname "$0")"

# rustup's cargo, on the stable toolchain rust-toolchain.toml names. Flux's
# attributes expand to nothing there; ./verify.sh checks them, with the tap's
# Flux on the nightly it runs on.
PATH="$HOME/.cargo/bin:$HOME/.local/bin:$PATH"
export PATH

cargo fmt --check

# Flux runs first, alone. Its summary must say `0 trusted; 0 ignored`.
FLUXFLAGS=""
export FLUXFLAGS
flux_log=$(mktemp)
trap 'rm -f "$flux_log"' EXIT
flux_status=0
./verify.sh --message-format=json-render-diagnostics >"$flux_log" 2>&1 || flux_status=$?
grep -v '^{' "$flux_log" >&2 || true
[ "$flux_status" -eq 0 ] || exit "$flux_status"
# shellcheck disable=SC2046
./flux-summary.sh "$flux_log" $(./verify.sh --list)

cargo clippy --all-targets --quiet -- -D warnings

cargo test --quiet
echo "gate: green" >&2
