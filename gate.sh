#!/bin/sh
# The gate of guild/forge: what must pass before anything lands on main.
# docs/agents/afkconvoy.md names it. It passes on the empty repo; add each check
# this repo needs as it grows.
set -e
cd "$(dirname "$0")"
echo "gate: green" >&2
