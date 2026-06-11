#!/usr/bin/env bash
# Bump the workspace version everywhere it matters (Cargo.toml, docs, READMEs,
# Rust doc comments), skipping CHANGELOG.md, lock files, and historical content.
# Then re-pin workspace members in Cargo.lock (third-party deps untouched).
#
# Usage: bash scripts/bump-version.sh <new-version> [--dry-run]
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

python3 "$ROOT/scripts/bump-version.py" "$@"

if [[ "$*" != *--dry-run* ]]; then
  echo
  echo "Syncing Cargo.lock workspace members..."
  cargo update --workspace --manifest-path "$ROOT/Cargo.toml"
  bash "$ROOT/scripts/check-doc-versions.sh"
fi
