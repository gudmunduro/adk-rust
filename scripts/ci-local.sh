#!/usr/bin/env bash
# Run the CI checks locally, without devenv/Nix.
#
# Every check CI runs is a plain cargo command or repo script underneath
# (devenv only provisions the environment), so this mirrors the ci.yml jobs
# 1:1 for fast local feedback:
#
#   fmt        cargo fmt --all -- --check
#   clippy     cargo clippy --workspace -- -D warnings
#   test       cargo nextest run --workspace --profile ci
#   templates  scripts/check-example-name-collisions.sh
#              scripts/check-doc-examples.sh
#              scripts/check-doc-versions.sh
#              scripts/check-publish-order.sh
#              scripts/check-cargo-adk-templates.sh   (slow — skipped unless --full)
#
# Usage:
#   bash scripts/ci-local.sh           # everything except the slow scaffold check
#   bash scripts/ci-local.sh --full    # include the cargo-adk scaffold compile check
#   bash scripts/ci-local.sh fmt clippy   # run only the named checks
#
# For true workflow emulation (runner image, devenv, caches) use act:
#   act pull_request -j templates      # requires docker; slow first run
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT" || exit 1

FULL=false
ONLY=()
for arg in "$@"; do
  case "$arg" in
    --full) FULL=true ;;
    *) ONLY+=("$arg") ;;
  esac
done

want() {
  [[ ${#ONLY[@]} -eq 0 ]] || printf '%s\n' "${ONLY[@]}" | grep -qx "$1"
}

PASS=()
FAIL=()
run_check() {
  local name="$1"; shift
  echo ""
  echo "━━━ $name ━━━"
  if "$@"; then
    PASS+=("$name")
  else
    FAIL+=("$name")
    echo "❌ $name failed"
  fi
}

if want fmt; then
  run_check fmt cargo fmt --all -- --check
fi

if want clippy; then
  run_check clippy cargo clippy --workspace --all-targets -- -D warnings
fi

if want test; then
  if command -v cargo-nextest >/dev/null 2>&1; then
    run_check test cargo nextest run --workspace --profile ci
  else
    echo "⚠ cargo-nextest not installed (cargo install cargo-nextest) — falling back to cargo test"
    run_check test cargo test --workspace
  fi
fi

if want templates; then
  run_check check-example-name-collisions bash scripts/check-example-name-collisions.sh
  run_check check-doc-examples bash scripts/check-doc-examples.sh
  run_check check-doc-versions bash scripts/check-doc-versions.sh
  run_check check-publish-order bash scripts/check-publish-order.sh
  if $FULL; then
    run_check check-cargo-adk-templates bash scripts/check-cargo-adk-templates.sh
  else
    echo "(skipping check-cargo-adk-templates — pass --full to include the scaffold compile check)"
  fi
fi

echo ""
echo "━━━ summary ━━━"
echo "✅ passed: ${PASS[*]:-none}"
if [[ ${#FAIL[@]} -gt 0 ]]; then
  echo "❌ failed: ${FAIL[*]}"
  exit 1
fi
echo "All local CI checks green."
