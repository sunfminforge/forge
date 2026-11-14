#!/bin/sh
# The gate of guild/forge: what must pass before anything lands on main.
# docs/agents/afkconvoy.md names it. Each tool's tool.toml must be as
# `guild forge publish` reads it; add each check the tools need as they grow.
set -e
cd "$(dirname "$0")"
guild forge check .
echo "gate: green" >&2
