#!/usr/bin/env bash
# Prepare the working tree for development and tests.
#
# This is the canonical place for project-local env setup. It is invoked
# automatically by the SessionStart hook in `.claude/settings.json` for
# Claude Code on the web sessions, and can be run manually otherwise.
#
# What it does:
#   1. Fetches git tags from origin. The integration tests in
#      rustloc/tests/cli_integration.rs reference real release tags
#      (v0.14.0, v0.14.2) via `rustloc diff <tag>..<tag>`. Cloud
#      sessions clone without tags, so without this fetch three diff
#      tests fail with "expected commit, got tag" style errors.
#   2. Installs the lefthook pre-commit hooks if lefthook is on PATH.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

if git rev-parse --is-inside-work-tree >/dev/null 2>&1 && git remote get-url origin >/dev/null 2>&1; then
    echo "Fetching git tags from origin..."
    git fetch --tags --quiet origin || echo "warning: git fetch --tags failed (offline?)"
fi

if command -v lefthook >/dev/null 2>&1; then
    echo "Installing lefthook hooks..."
    lefthook install >/dev/null
else
    echo "lefthook not on PATH; skipping hook install"
fi

echo "Dev env ready."
